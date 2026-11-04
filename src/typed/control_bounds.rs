//! Extension-only preflight. Existing closed-program budgets are unchanged.
//! Tree depth is checked iteratively before recursive consumers. Scope costs are
//! conservative reservations for all possible locals, including dead branches.
use super::*;
use crate::typed::{AdtType, CopyPattern, ResultRepr};

const MAX_EXPRESSION_DEPTH: usize = 128;

impl Witness {
    pub(super) fn depth(&self, budget: &mut Budget) -> Result<(), String> {
        budget.charge(self.nodes.len().checked_mul(2).ok_or(LIMIT)?)?;
        let mut depths = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let depth = match node.parent {
                None => 0usize,
                Some((parent, _)) if parent.0 < index => depths[parent.0] + 1,
                _ => return Err(INVALID.into()),
            };
            if depth > MAX_EXPRESSION_DEPTH {
                return Err(LIMIT.into());
            }
            depths.push(depth);
        }
        Ok(())
    }
}

fn layout_bound(adts: &[AdtType], budget: &mut Budget) -> Result<usize, String> {
    budget.charge(adts.len().checked_mul(2).ok_or(LIMIT)?)?;
    for adt in adts {
        budget.charge(adt.variants.len())?;
        for variant in &adt.variants {
            budget.charge(variant.fields.len())?;
        }
    }
    // Existing layout validation charges at least 64 bytes per state-tree node.
    Ok(crate::typed::adts::layout_costs(adts)?
        .into_iter()
        .max()
        .unwrap_or(64)
        / 64
        + 1)
}

#[derive(Default)]
struct Storage {
    nodes: usize,
    bindings: usize,
    forks: usize,
    names: usize,
    paths: usize,
}
impl Storage {
    fn reserve(&self, layout: usize, budget: &mut Budget) -> Result<(), String> {
        let local = self
            .bindings
            .checked_mul(layout.checked_add(8).ok_or(LIMIT)?)
            .ok_or(LIMIT)?;
        let scope = local
            .checked_add(self.names)
            .and_then(|n| n.checked_add(self.paths))
            .ok_or(LIMIT)?;
        // Four passes cover snapshots, joins and state comparisons. Every read
        // also reserves a complete state walk, even when it only reads an atom.
        let snapshots = self.forks.checked_mul(scope).ok_or(LIMIT)?;
        let visits = self
            .nodes
            .checked_add(self.bindings)
            .and_then(|n| n.checked_mul(layout))
            .ok_or(LIMIT)?;
        budget.charge(
            snapshots
                .checked_add(visits)
                .and_then(|n| n.checked_mul(4))
                .ok_or(LIMIT)?,
        )
    }
}

impl SourceFacts<'_, '_> {
    pub(in crate::typed) fn check_depth(&self) -> Result<(), String> {
        self.witness.depth(&mut Budget::default())
    }
    pub(in crate::typed) fn preflight_lower(
        &self,
        instances: &crate::typed::instances::Instances,
        adts: &[AdtType],
    ) -> Result<(), String> {
        let mut budget = Budget::default();
        self.witness.depth(&mut budget)?;
        budget.charge(self.witness_work.checked_mul(2).ok_or(LIMIT)?)?;
        let layout = layout_bound(adts, &mut budget)?;
        budget.charge(instances.list.len())?;
        for instance in &instances.list {
            budget.charge(self.witness.nodes.len())?;
            let mut storage = Storage {
                bindings: instance.params.len(),
                ..Storage::default()
            };
            for node in self
                .witness
                .nodes
                .iter()
                .filter(|node| node.source == instance.source)
            {
                storage.nodes += 1;
                match &node.kind {
                    SourceKind::Let(_, _, _) => storage.bindings += 1,
                    SourceKind::Call(_, _, _) => storage.forks += 1,
                    SourceKind::Binary(_, _, _) => storage.forks += 2,
                    SourceKind::If(_, _, _) => storage.forks += 3,
                    SourceKind::While(_, _) => storage.forks += 3,
                    SourceKind::Match(_, arms) => {
                        budget.charge(arms.len())?;
                        storage.forks += arms.len().checked_mul(3).ok_or(LIMIT)? + 1;
                        // All canonical patterns are a safe upper bound per
                        // match. Precharge that bound before scanning/cloning.
                        budget.charge(self.witness.patterns.len())?;
                        storage.bindings += self.witness.patterns.len();
                    }
                    SourceKind::Field(_, path)
                    | SourceKind::InoutField(_, path)
                    | SourceKind::FieldAssign(_, path, _) => storage.paths += path.len(),
                    _ => {}
                }
            }
            budget.charge(self.witness.bindings.len())?;
            for binding in &self.witness.bindings {
                // Names are only cloned in source lowering. Count all source
                // names, including shadowed and other-instance names, safely.
                storage.names = storage.names.checked_add(binding.name.len()).ok_or(LIMIT)?;
            }
            storage.reserve(layout, &mut budget)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(in crate::typed) enum Tree<'a> {
    Value(&'a ExprNode),
    NoNormal(&'a NoNormalExpr),
}
impl<'a> Tree<'a> {
    fn child_bound(self) -> usize {
        match self {
            Self::NoNormal(node) => match &node.kind {
                NoNormalKind::Call(_, args) => args.len(),
                NoNormalKind::Match { arms, .. } => arms.len().saturating_add(1),
                NoNormalKind::If { .. } => 3,
                _ => 2,
            },
            Self::Value(node) => match &node.kind {
                ExprKind::Call(_, args) => args.len(),
                ExprKind::ForeignCall(_, args) => args.len(),
                ExprKind::CopyMatch { arms, .. } => arms.len().saturating_add(1),
                ExprKind::Match { arms, .. } => arms.len().saturating_add(1),
                ExprKind::Record { fields, .. } => fields.len(),
                ExprKind::Constructor { fields, .. } => fields.len(),
                ExprKind::Vector(_, args) => args.len(),
                _ => 3,
            },
        }
    }

    pub(in crate::typed) fn children(self, mut add: impl FnMut(Tree<'a>)) {
        match self {
            Self::NoNormal(node) => match &node.kind {
                NoNormalKind::Call(_, args) => {
                    for arg in args {
                        if let CallArg::Value(v) | CallArg::ReadTemporary(v) = arg {
                            add(Self::Value(v));
                        }
                    }
                }
                NoNormalKind::Let { value, body, .. } => {
                    add(Self::Value(value));
                    add(Self::NoNormal(body));
                }
                NoNormalKind::NoStorageLet { value, body, .. } => {
                    add(Self::NoNormal(value));
                    add(Self::NoNormal(body));
                }
                NoNormalKind::If { condition, yes, no } => {
                    add(Self::Value(condition));
                    add(Self::NoNormal(yes));
                    add(Self::NoNormal(no));
                }
                NoNormalKind::Match { value, arms } => {
                    add(Self::Value(value));
                    for arm in arms {
                        add(Self::NoNormal(&arm.body));
                    }
                }
            },
            Self::Value(node) => match &node.kind {
                ExprKind::NoStorageLet { value, body, .. } => {
                    add(Self::NoNormal(value));
                    add(Self::Value(body));
                }
                ExprKind::ForeignCall(_, args) => {
                    for arg in args {
                        if let ForeignArg::Value(v) = arg {
                            add(Self::Value(v));
                        }
                    }
                }
                ExprKind::Call(_, args) => {
                    for arg in args {
                        if let CallArg::Value(v) | CallArg::ReadTemporary(v) = arg {
                            add(Self::Value(v));
                        }
                    }
                }
                ExprKind::Vector(_, args) | ExprKind::Constructor { fields: args, .. } => {
                    for value in args {
                        add(Self::Value(value));
                    }
                }
                ExprKind::Record { fields, .. } => {
                    for (_, value) in fields {
                        add(Self::Value(value));
                    }
                }
                ExprKind::CopyMatch { value, arms } => {
                    add(Self::Value(value));
                    for arm in arms {
                        add(Self::Value(&arm.body));
                    }
                }
                ExprKind::Match { value, arms } => {
                    add(Self::Value(value));
                    for arm in arms {
                        add(Self::Value(&arm.body));
                    }
                }
                ExprKind::OwnerLet { init, body, .. } => {
                    if let OwnerInit::Alloc(value)
                    | OwnerInit::BytesAlloc(value)
                    | OwnerInit::Call(value)
                    | OwnerInit::Expr(value) = init
                    {
                        add(Self::Value(value));
                    }
                    add(Self::Value(body));
                }
                ExprKind::FieldAssign(_, value)
                | ExprKind::ForeignConvert(_, value)
                | ExprKind::IsNull(value)
                | ExprKind::TryAlloc { input: value, .. }
                | ExprKind::OwnerBytesGet(_, value)
                | ExprKind::OwnerWrite(_, value)
                | ExprKind::Assign(_, value)
                | ExprKind::ByteLen(value)
                | ExprKind::BytesLen(value) => add(Self::Value(value)),
                ExprKind::OwnerBytesSet(_, a, b)
                | ExprKind::Let {
                    value: a, body: b, ..
                }
                | ExprKind::Binary(_, a, b)
                | ExprKind::While(a, b)
                | ExprKind::ByteAt(a, b)
                | ExprKind::BytesGet(a, b) => {
                    add(Self::Value(a));
                    add(Self::Value(b));
                }
                ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
                    add(Self::Value(a));
                    add(Self::Value(b));
                    add(Self::Value(c));
                }
                _ => {}
            },
        }
    }
}
fn pattern(
    pattern: &CopyPattern,
    depth: usize,
    storage: &mut Storage,
    budget: &mut Budget,
) -> Result<(), String> {
    budget.charge(1)?;
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(LIMIT.into());
    }
    match pattern {
        CopyPattern::Binding(_) => storage.bindings += 1,
        CopyPattern::Constructor { fields, .. } => {
            budget.charge(fields.len())?;
            for field in fields {
                pattern_walk(field, depth + 1, storage, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn pattern_walk(
    p: &CopyPattern,
    depth: usize,
    storage: &mut Storage,
    budget: &mut Budget,
) -> Result<(), String> {
    pattern(p, depth, storage, budget)
}

pub(in crate::typed) fn preflight_ir(program: &TypedMain) -> Result<(), String> {
    let mut budget = Budget::default();
    let layout = layout_bound(&program.adts, &mut budget)?;
    budget.charge(program.functions.len())?;
    for function in &program.functions {
        budget.charge(function.params.len())?;
        let mut storage = Storage {
            bindings: function.params.len(),
            ..Storage::default()
        };
        budget.charge(1)?;
        let root = match &function.body {
            Computation::Value(v) => Tree::Value(v),
            Computation::NoNormal(v) => Tree::NoNormal(v),
        };
        let mut pending = vec![(root, 0usize)];
        while let Some((tree, depth)) = pending.pop() {
            budget.charge(1)?;
            if depth > MAX_EXPRESSION_DEPTH {
                return Err(LIMIT.into());
            }
            storage.nodes += 1;
            budget.charge(tree.child_bound().checked_mul(2).ok_or(LIMIT)?)?;
            tree.children(|child| pending.push((child, depth + 1)));
            match tree {
                Tree::NoNormal(node) => match &node.kind {
                    NoNormalKind::Let { .. } => storage.bindings += 1,
                    NoNormalKind::Call(_, args) => {
                        storage.forks += 1;
                        call_paths(args, &mut storage, &mut budget)?;
                    }
                    NoNormalKind::If { .. } => storage.forks += 3,
                    NoNormalKind::Match { arms, .. } => {
                        budget.charge(arms.len())?;
                        storage.forks += arms.len().checked_mul(3).ok_or(LIMIT)? + 1;
                        for arm in arms {
                            pattern(&arm.pattern, 0, &mut storage, &mut budget)?;
                        }
                    }
                    _ => {}
                },
                Tree::Value(node) => match &node.kind {
                    ExprKind::Let { .. } => storage.bindings += 1,
                    ExprKind::Call(_, args) => {
                        storage.forks += 1;
                        call_paths(args, &mut storage, &mut budget)?;
                    }
                    ExprKind::If(..) => storage.forks += 3,
                    ExprKind::Binary(_, _, _) => storage.forks += 2,
                    ExprKind::While(..) => storage.forks += 3,
                    ExprKind::CopyMatch { arms, .. } => {
                        budget.charge(arms.len())?;
                        storage.forks += arms.len().checked_mul(3).ok_or(LIMIT)? + 1;
                        for arm in arms {
                            pattern(&arm.pattern, 0, &mut storage, &mut budget)?;
                        }
                    }
                    _ => {}
                },
            }
        }
        storage.reserve(layout, &mut budget)?;
    }
    Ok(())
}
fn call_paths(args: &[CallArg], storage: &mut Storage, budget: &mut Budget) -> Result<(), String> {
    budget.charge(args.len())?;
    for arg in args {
        if let CallArg::ReadField(place) | CallArg::InoutField(place) = arg {
            storage.paths = storage.paths.checked_add(place.path.len()).ok_or(LIMIT)?;
        }
    }
    Ok(())
}

// An unproved old IR must still be scanned without recursing on its Rust stack.
// The extension path invokes bounded preflight first. The legacy path keeps its
// existing accepted size limits and only uses this scan to detect forbidden tags.
pub(in crate::typed) fn has_extension(program: &TypedMain) -> bool {
    if program.logical.control.is_some()
        || program.functions.iter().any(|f| {
            matches!(f.result, ResultRepr::NoNormal(_))
                || matches!(f.body, Computation::NoNormal(_))
        })
    {
        return true;
    }
    for function in &program.functions {
        let Computation::Value(root) = &function.body else {
            return true;
        };
        let mut pending = vec![Tree::Value(root)];
        while let Some(tree) = pending.pop() {
            match tree {
                Tree::NoNormal(_) => return true,
                Tree::Value(value) if matches!(value.kind, ExprKind::NoStorageLet { .. }) => {
                    return true
                }
                _ => tree.children(|child| pending.push(child)),
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::{Type as RuntimeType, TypedFunction};
    use crate::{check, parser};

    fn program(body: ExprNode) -> TypedMain {
        TypedMain {
            logical: Default::default(),
            native: Default::default(),
            allocation_contract: None,
            adts: vec![],
            functions: vec![TypedFunction {
                params: vec![],
                result: ResultRepr::Value(body.ty),
                body: Computation::Value(body),
                param_cleanup: vec![],
            }],
            entry: 0,
        }
    }
    #[test]
    fn source_depth_is_bounded_in_retained_witness_nodes() {
        let parsed = parser::parse("fn main() { 42 }").unwrap();
        let checked = check::analyze(&parsed).unwrap();
        let mut facts = SourceFacts::build(&checked).unwrap();
        // Public text syntax already limits nesting to 64. The frozen witness
        // boundary must independently reject malformed deeper retained trees.
        for parent in 0..132 {
            facts.witness.nodes.push(SourceNode {
                source: 0,
                parent: Some((NodeId(parent), Role::Left)),
                elaborated_local: false,
                ty: Type::Int,
                kind: SourceKind::Int(0),
            });
        }
        assert!(facts
            .check_depth()
            .unwrap_err()
            .contains("CONTROL-COMPLEXITY"));
    }
    #[test]
    fn actual_ir_depth_and_scope_snapshots_are_precharged() {
        let mut value = ExprNode {
            ty: RuntimeType::Int,
            kind: ExprKind::Int(0),
        };
        for _ in 0..132 {
            value = ExprNode {
                ty: RuntimeType::Int,
                kind: ExprKind::ByteLen(Box::new(value)),
            };
        }
        let retained = ExprNode {
            ty: RuntimeType::Int,
            kind: ExprKind::If(
                Box::new(ExprNode {
                    ty: RuntimeType::Bool,
                    kind: ExprKind::Bool(true),
                }),
                Box::new(ExprNode {
                    ty: RuntimeType::Int,
                    kind: ExprKind::Int(42),
                }),
                Box::new(value),
            ),
        };
        assert!(preflight_ir(&program(retained))
            .unwrap_err()
            .contains("CONTROL-COMPLEXITY"));
        let mut value = program(ExprNode {
            ty: RuntimeType::Unit,
            kind: ExprKind::Vector(
                crate::typed::VectorOp::Load,
                (0..300)
                    .map(|_| ExprNode {
                        ty: RuntimeType::Int,
                        kind: ExprKind::Call(0, vec![]),
                    })
                    .collect(),
            ),
        });
        value.functions[0].params = (0..300)
            .map(|i| (crate::typed::LocalId(i), RuntimeType::Int, ParamMode::Value))
            .collect();
        assert!(preflight_ir(&value)
            .unwrap_err()
            .contains("CONTROL-COMPLEXITY"));
    }
    #[test]
    fn malformed_witness_parent_cycles_are_rejected_without_recursion() {
        let parsed = parser::parse("fn main() { 42 }").unwrap();
        let checked = check::analyze(&parsed).unwrap();
        let mut facts = SourceFacts::build(&checked).unwrap();
        facts.witness.nodes[0].parent = Some((NodeId(0), Role::Left));
        assert!(facts.check_depth().is_err());
    }
}
