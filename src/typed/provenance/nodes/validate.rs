//! Independent correspondence checks over actual IR, exact source children and
//! lexical scopes. This pass validates receipts; it never repairs or lowers.
use super::trace::{RawOrigin, SourceIndex};
use super::*;
use crate::{
    ast::Pattern,
    source_map::AstStep,
    typed::{CallArg, CopyPattern, ExprKind, FieldPlace, LocalId, VectorOp},
};

type ScopeUses = HashMap<(usize, usize), usize>;
type Binders = HashMap<(usize, usize), LocalId>;

pub(super) fn lexical(
    index: &SourceIndex<'_>,
    instances: &instances::Instances,
    budget: &mut BodyBudget,
) -> Result<ScopeUses, String> {
    enum Event<'a> {
        Visit(usize),
        Bind(&'a str, usize),
        Restore(&'a str, Option<usize>),
        Arm(usize),
    }
    let mut uses = HashMap::new();
    for (instance, definition) in instances.list.iter().enumerate() {
        let root = index
            .root(AstDeclaration::Function(definition.source))
            .ok_or_else(|| fail("missing lexical function root"))?;
        let Some(AstNode::Function(function)) = index.node(root) else {
            return Err(fail("invalid lexical function root"));
        };
        let mut scope = HashMap::new();
        for (ordinal, param) in function.params.iter().enumerate() {
            budget.records(1)?;
            budget.work(1)?;
            scope.insert(
                param.name.as_str(),
                index
                    .child(root, AstStep::Parameter(ordinal))
                    .ok_or_else(|| fail("missing lexical parameter"))?,
            );
        }
        budget.records(1)?;
        let mut pending = vec![Event::Visit(
            index
                .child(root, AstStep::FunctionBody)
                .ok_or_else(|| fail("missing lexical body"))?,
        )];
        while let Some(event) = pending.pop() {
            budget.work(1)?;
            match event {
                Event::Bind(name, source) => {
                    scope.insert(name, source);
                }
                Event::Restore(name, previous) => {
                    if let Some(previous) = previous {
                        scope.insert(name, previous);
                    } else {
                        scope.remove(name);
                    }
                }
                Event::Arm(source) => {
                    let Some(AstNode::MatchArm(_)) = index.node(source) else {
                        return Err(fail("invalid lexical arm"));
                    };
                    let pattern = index
                        .child(source, AstStep::ArmPattern)
                        .ok_or_else(|| fail("missing lexical pattern"))?;
                    budget.records(1)?;
                    let mut patterns = vec![pattern];
                    let mut restores = Vec::new();
                    while let Some(slot) = patterns.pop() {
                        budget.work(1)?;
                        match index.node(slot) {
                            Some(AstNode::Pattern(Pattern::Binding(name))) => {
                                budget.records(1)?;
                                restores
                                    .push(Event::Restore(name, scope.insert(name.as_str(), slot)));
                            }
                            Some(AstNode::Pattern(Pattern::Constructor(_, fields))) => {
                                budget.records(fields.len())?;
                                for i in (0..fields.len()).rev() {
                                    patterns.push(
                                        index
                                            .child(slot, AstStep::PatternField(i))
                                            .ok_or_else(|| fail("missing lexical pattern child"))?,
                                    );
                                }
                            }
                            Some(AstNode::Pattern(
                                Pattern::Wildcard
                                | Pattern::Int(_)
                                | Pattern::Bool(_)
                                | Pattern::Unit,
                            )) => {}
                            _ => return Err(fail("invalid lexical pattern node")),
                        }
                    }
                    budget.records(restores.len() + 1)?;
                    pending.extend(restores);
                    pending.push(Event::Visit(
                        index
                            .child(source, AstStep::ArmBody)
                            .ok_or_else(|| fail("missing lexical arm body"))?,
                    ));
                }
                Event::Visit(source) => {
                    let Some(AstNode::Expression(expr)) = index.node(source) else {
                        return Err(fail("invalid lexical expression"));
                    };
                    match expr {
                        Expr::Let(name, _, _) | Expr::LetMut(name, _, _) => {
                            budget.records(4)?;
                            pending.push(Event::Restore(name, scope.get(name.as_str()).copied()));
                            pending.push(Event::Visit(
                                index
                                    .child(source, AstStep::LetBody)
                                    .ok_or_else(|| fail("missing lexical let body"))?,
                            ));
                            pending.push(Event::Bind(name, source));
                            pending.push(Event::Visit(
                                index
                                    .child(source, AstStep::LetValue)
                                    .ok_or_else(|| fail("missing lexical initializer"))?,
                            ));
                        }
                        Expr::Match(_, arms) => {
                            budget.records(arms.len() + 1)?;
                            for i in (0..arms.len()).rev() {
                                pending.push(Event::Arm(
                                    index
                                        .child(source, AstStep::Arm(i))
                                        .ok_or_else(|| fail("missing lexical arm"))?,
                                ));
                            }
                            pending.push(Event::Visit(
                                index
                                    .child(source, AstStep::MatchValue)
                                    .ok_or_else(|| fail("missing lexical match value"))?,
                            ));
                        }
                        _ => {
                            let name = match expr {
                                Expr::Var(name)
                                | Expr::Inout(name)
                                | Expr::Move(name)
                                | Expr::Field(name, _)
                                | Expr::InoutField(name, _)
                                | Expr::Assign(name, _)
                                | Expr::FieldAssign(name, _, _) => Some(name.as_str()),
                                _ => None,
                            };
                            if let Some(binding) = name.and_then(|name| scope.get(name)).copied() {
                                budget.records(1)?;
                                uses.insert((instance, source), binding);
                            }
                            let count = match expr {
                                Expr::Binary(..) | Expr::While(..) => 2,
                                Expr::If(..) => 3,
                                Expr::Assign(..) | Expr::FieldAssign(..) => 1,
                                Expr::Call(_, args) => args.len(),
                                Expr::Record(_, fields) => fields.len(),
                                _ => 0,
                            };
                            budget.records(count)?;
                            budget.edges(count)?;
                            let steps = AstNode::Expression(expr).child_steps();
                            for step in steps.into_iter().rev() {
                                pending.push(Event::Visit(
                                    index
                                        .child(source, step)
                                        .ok_or_else(|| fail("missing lexical child"))?,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(uses)
}

pub(super) fn bindings(
    trace: &mut Trace<'_>,
    uses: &ScopeUses,
    actual: &[Option<TypedNodeRef<'_>>],
) -> Result<Binders, String> {
    let mut binders = HashMap::new();
    let mut locals = HashSet::new();
    for receipt in &trace.bindings {
        trace.budget.records(2)?;
        trace.budget.work(1)?;
        if binders
            .insert((receipt.instance, receipt.source), receipt.local)
            .is_some()
            || !locals.insert(receipt.local)
        {
            return Err(fail("duplicate source binder/local receipt"));
        }
        match trace.source_index.node(receipt.source) {
            Some(AstNode::Parameter(_))
            | Some(AstNode::Expression(Expr::Let(..) | Expr::LetMut(..)))
            | Some(AstNode::Pattern(Pattern::Binding(_))) => {}
            _ => return Err(fail("receipt is not a source binder")),
        }
    }
    let mut used_binders = HashSet::new();
    trace.budget.records(binders.len())?;
    for (fragment, node) in trace.fragments.iter().zip(actual) {
        trace.budget.work(1)?;
        let RawOrigin::Source { source, rule } = fragment.origin else {
            continue;
        };
        let node = node.ok_or_else(|| fail("binding validation missing actual fragment"))?;
        let declared = match node {
            TypedNodeRef::Parameter((local, _, _)) => Some(*local),
            TypedNodeRef::Expression(ExprNode {
                kind: ExprKind::Let { local, .. },
                ..
            }) => Some(*local),
            TypedNodeRef::Pattern(CopyPattern::Binding(local)) => Some(*local),
            _ => None,
        };
        if let Some(local) = declared {
            used_binders.insert((fragment.instance, source));
            if binders.get(&(fragment.instance, source)) != Some(&local) {
                return Err(fail(
                    "typed binder differs from construction/source receipt",
                ));
            }
        }
        if rule == DirectRule::ProjectedUse {
            continue;
        }
        let used = match node {
            TypedNodeRef::Expression(ExprNode {
                kind: ExprKind::Read(local) | ExprKind::Assign(local, _),
                ..
            }) => Some(*local),
            TypedNodeRef::Expression(ExprNode {
                kind: ExprKind::FieldRead(place) | ExprKind::FieldAssign(place, _),
                ..
            }) => Some(place.root),
            TypedNodeRef::Argument(CallArg::ReadCopy(local) | CallArg::InoutCopy(local)) => {
                Some(*local)
            }
            TypedNodeRef::Argument(CallArg::ReadField(place) | CallArg::InoutField(place)) => {
                Some(place.root)
            }
            _ => None,
        };
        if let Some(local) = used {
            let binder = uses
                .get(&(fragment.instance, source))
                .ok_or_else(|| fail("typed local use lacks active lexical binding"))?;
            if binders.get(&(fragment.instance, *binder)) != Some(&local) {
                return Err(fail(
                    "typed local root differs from active lexical source binding",
                ));
            }
        }
    }
    if used_binders.len() != binders.len() {
        return Err(fail("unreachable source binder receipt"));
    }
    Ok(binders)
}

fn expected_step(role: TypedChildRole) -> Option<AstStep> {
    Some(match role {
        TypedChildRole::Parameter(i) => AstStep::Parameter(i),
        TypedChildRole::FunctionBody => AstStep::FunctionBody,
        TypedChildRole::LetValue => AstStep::LetValue,
        TypedChildRole::LetBody => AstStep::LetBody,
        TypedChildRole::BinaryLeft => AstStep::BinaryLeft,
        TypedChildRole::BinaryRight => AstStep::BinaryRight,
        TypedChildRole::Condition => AstStep::Condition,
        TypedChildRole::Then => AstStep::Then,
        TypedChildRole::Else => AstStep::Else,
        TypedChildRole::LoopBody => AstStep::WhileBody,
        TypedChildRole::AssignmentValue => AstStep::AssignValue,
        TypedChildRole::BuiltinOperand(i)
        | TypedChildRole::ConstructorField(i)
        | TypedChildRole::CallArgument(i) => AstStep::Argument(i),
        TypedChildRole::MatchScrutinee => AstStep::MatchValue,
        TypedChildRole::MatchArm(i) => AstStep::Arm(i),
        TypedChildRole::ArmPattern => AstStep::ArmPattern,
        TypedChildRole::ArmBody => AstStep::ArmBody,
        TypedChildRole::PatternField(i) => AstStep::PatternField(i),
        TypedChildRole::RecordField(i) => AstStep::RecordField(i),
        TypedChildRole::Capacity => AstStep::Argument(0),
        TypedChildRole::FunctionRoot
        | TypedChildRole::Result
        | TypedChildRole::ArgumentValue
        | TypedChildRole::RecordValue => return None,
    })
}
fn anchor(origin: &RawOrigin) -> Option<usize> {
    match origin {
        RawOrigin::Source { source, .. } => Some(*source),
        RawOrigin::Generated { reason, relations } => {
            let role = match reason {
                GenerationReason::CaptureEnvironmentBinding
                | GenerationReason::CaptureProduct
                | GenerationReason::CaptureProductField => RelationRole::IntroducedForBinding,
                GenerationReason::ReadArgumentTemporary
                | GenerationReason::ShapeMaterialization
                | GenerationReason::ShapeRecordField
                | GenerationReason::CaptureProjection
                | GenerationReason::ProjectedCaptureTemporary => RelationRole::CausedByUse,
                GenerationReason::InferredResultType | GenerationReason::AnnotatedResultType => {
                    return None
                }
            };
            relations
                .iter()
                .find_map(|r| match (&r.target, r.role == role) {
                    (RawTarget::Source(s), true) => Some(*s),
                    _ => None,
                })
        }
    }
}
pub(super) fn route(
    trace: &mut Trace<'_>,
    instance: usize,
    mut source: usize,
    fragment: usize,
) -> Result<(), String> {
    let origin = &trace
        .fragments
        .get(fragment)
        .ok_or_else(|| fail("invalid routed fragment"))?
        .origin;
    if let RawOrigin::Generated { relations, .. } = origin {
        trace.budget.work(relations.len())?;
    }
    let actual = anchor(origin).ok_or_else(|| fail("output lacks required source route"))?;
    loop {
        trace.budget.work(1)?;
        if source == actual {
            return Ok(());
        }
        trace.budget.work(1)?;
        let erased = trace
            .binding_sources
            .get(&(instance, source))
            .and_then(|&i| trace.shape_bindings.get(i))
            .filter(|r| r.environment.is_none())
            .ok_or_else(|| fail("output source differs from pinned source child"))?;
        if erased.output != Some(fragment) || erased.body != Some(fragment) {
            return Err(fail(
                "erased let did not preserve the same continuation fragment",
            ));
        }
        source = erased.continuation;
    }
}
pub(super) fn routing(
    trace: &mut Trace<'_>,
    instances: &instances::Instances,
) -> Result<(), String> {
    for (instance, definition) in instances.list.iter().enumerate() {
        let source = trace
            .source_index
            .root(AstDeclaration::Function(definition.source))
            .ok_or_else(|| fail("missing function source root"))?;
        route(trace, instance, source, trace.roots[instance])?;
    }
    for i in 0..trace.shape_bindings.len() {
        let binding = &trace.shape_bindings[i];
        let (instance, source, body) = (
            binding.instance,
            binding.continuation,
            binding
                .body
                .ok_or_else(|| fail("unfinished capture continuation"))?,
        );
        route(trace, instance, source, body)?;
    }
    for ordinal in 0..trace.fragments.len() {
        trace.budget.work(1)?;
        let fragment = &trace.fragments[ordinal];
        let instance = fragment.instance;
        let RawOrigin::Source { source, .. } = fragment.origin else {
            continue;
        };
        // Borrow each edge briefly; no clone of the entire child list.
        for i in 0..fragment.children.len() {
            let (role, child) = trace.fragments[ordinal].children[i];
            if role == TypedChildRole::Result {
                continue;
            }
            let expected = if matches!(
                role,
                TypedChildRole::ArgumentValue | TypedChildRole::RecordValue
            ) {
                source
            } else {
                let step = expected_step(role).ok_or_else(|| fail("invalid source child role"))?;
                trace
                    .source_index
                    .child(source, step)
                    .ok_or_else(|| fail("typed child has no matching pinned source child"))?
            };
            route(trace, instance, expected, child)?;
        }
    }
    Ok(())
}

fn field_matches(
    expr: &Expr,
    place: &FieldPlace,
    checked: &CheckedProgram<'_>,
    typed: &TypedMain,
) -> bool {
    checked.field_info(expr).is_some_and(|info| {
        info.len() == place.path.len()
            && info.iter().zip(&place.path).all(|(source, step)| {
                typed.adts.get(step.record).is_some_and(|record| {
                    record.name == source.record
                        && record.record_fields.is_some()
                        && record.variants[0].fields.len() == source.arity
                        && step.field == source.index
                })
            })
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn direct(
    source: AstNode<'_>,
    rule: DirectRule,
    node: TypedNodeRef<'_>,
    instance: usize,
    source_slot: usize,
    checked: &CheckedProgram<'_>,
    typed: &TypedMain,
    instances: &instances::Instances,
    requests: &HashMap<(usize, usize), usize>,
    budget: &mut BodyBudget,
    types: &mut instances::Trace,
) -> Result<(), String> {
    budget.work(1)?;
    let matched = match (rule, source, node) {
        (
            DirectRule::Function,
            AstNode::Function(source),
            TypedNodeRef::Function { params, .. },
        ) => source.params.len() == params.len(),
        (
            DirectRule::Parameter,
            AstNode::Parameter(source),
            TypedNodeRef::Parameter((_, _, mode)),
        ) => source.mode == *mode,
        (DirectRule::Arm, AstNode::MatchArm(_), TypedNodeRef::Arm(_)) => true,
        (DirectRule::RecordField, AstNode::Expression(_), TypedNodeRef::RecordField { .. }) => true,
        (
            DirectRule::EmbeddedCapacity,
            AstNode::Expression(Expr::Int(n)),
            TypedNodeRef::Capacity(capacity),
        ) => usize::try_from(*n).ok() == Some(*capacity),
        (DirectRule::Pattern, AstNode::Pattern(source), TypedNodeRef::Pattern(actual)) => {
            match (source, actual) {
                (Pattern::Wildcard, CopyPattern::Wildcard)
                | (Pattern::Binding(_), CopyPattern::Binding(_))
                | (Pattern::Unit, CopyPattern::Unit) => true,
                (Pattern::Int(a), CopyPattern::Int(b)) => a == b,
                (Pattern::Bool(a), CopyPattern::Bool(b)) => a == b,
                (
                    Pattern::Constructor(name, fields),
                    CopyPattern::Constructor {
                        adt,
                        variant,
                        fields: values,
                    },
                ) => typed
                    .adts
                    .get(*adt)
                    .and_then(|a| a.variants.get(*variant))
                    .is_some_and(|v| &v.name == name && fields.len() == values.len()),
                _ => false,
            }
        }
        (DirectRule::Argument, AstNode::Expression(source), TypedNodeRef::Argument(actual)) => {
            match (source, actual) {
                (_, CallArg::Value(_))
                | (Expr::Var(_), CallArg::ReadCopy(_))
                | (Expr::Inout(_), CallArg::InoutCopy(_)) => true,
                (Expr::Field(..), CallArg::ReadField(place))
                | (Expr::InoutField(..), CallArg::InoutField(place)) => {
                    field_matches(source, place, checked, typed)
                }
                _ => false,
            }
        }
        (
            DirectRule::ProjectedUse,
            AstNode::Expression(Expr::Field(..)),
            TypedNodeRef::Expression(ExprNode {
                kind: ExprKind::FieldRead(_),
                ..
            }),
        ) => true,
        (DirectRule::Expression, AstNode::Expression(source), TypedNodeRef::Expression(actual)) => {
            match (source, &actual.kind) {
                (Expr::Int(a), ExprKind::Int(b)) => a == b,
                (Expr::Str(a), ExprKind::Str(b)) => a == b,
                (Expr::Bool(a), ExprKind::Bool(b)) => a == b,
                (Expr::Unit, ExprKind::Unit) | (Expr::Var(_), ExprKind::Read(_)) => true,
                (Expr::Binary(a, ..), ExprKind::Binary(b, ..)) => a == b,
                (Expr::Let(..), ExprKind::Let { mutable: false, .. })
                | (Expr::LetMut(..), ExprKind::Let { mutable: true, .. })
                | (Expr::If(..), ExprKind::If(..))
                | (Expr::While(..), ExprKind::While(..))
                | (Expr::Assign(..), ExprKind::Assign(..)) => true,
                (Expr::Field(..), ExprKind::FieldRead(place))
                | (Expr::FieldAssign(..), ExprKind::FieldAssign(place, _)) => {
                    field_matches(source, place, checked, typed)
                }
                (
                    Expr::Record(name, fields),
                    ExprKind::Record {
                        adt,
                        fields: values,
                    },
                ) => {
                    typed
                        .adts
                        .get(*adt)
                        .is_some_and(|a| a.name == *name && a.record_fields.is_some())
                        && checked.record_info(source).is_some_and(|i| {
                            i.field_indices
                                .iter()
                                .copied()
                                .eq(values.iter().map(|(i, _)| *i))
                        })
                        && fields.len() == values.len()
                }
                (
                    Expr::Var(name),
                    ExprKind::Constructor {
                        adt,
                        variant,
                        fields,
                    },
                ) => {
                    fields.is_empty()
                        && typed
                            .adts
                            .get(*adt)
                            .and_then(|a| a.variants.get(*variant))
                            .is_some_and(|v| v.name == *name)
                }
                (
                    Expr::Call(name, args),
                    ExprKind::Constructor {
                        adt,
                        variant,
                        fields,
                    },
                ) => {
                    args.len() == fields.len()
                        && typed
                            .adts
                            .get(*adt)
                            .and_then(|a| a.variants.get(*variant))
                            .is_some_and(|v| v.name == *name)
                }
                (Expr::Call(_, args), ExprKind::Call(callee, values)) => {
                    args.len() == values.len()
                        && requests.get(&(instance, source_slot)) == Some(callee)
                }
                (Expr::Call(name, args), ExprKind::ByteLen(_)) => {
                    name == "byte_len" && args.len() == 1
                }
                (Expr::Call(name, args), ExprKind::ByteAt(..)) => {
                    name == "byte_at" && args.len() == 2
                }
                (Expr::Call(name, args), ExprKind::BytesNew(n)) => {
                    name == "bytes_new"
                        && matches!(args.as_slice(),[Expr::Int(v)] if usize::try_from(*v).ok()==Some(*n))
                }
                (Expr::Call(name, args), ExprKind::BytesLen(_)) => {
                    name == "bytes_len" && args.len() == 1
                }
                (Expr::Call(name, args), ExprKind::BytesGet(..)) => {
                    name == "bytes_get" && args.len() == 2
                }
                (Expr::Call(name, args), ExprKind::BytesSet(..)) => {
                    name == "bytes_set" && args.len() == 3
                }
                (Expr::Call(name, args), ExprKind::Vector(op, values)) => {
                    VectorOp::named(name) == Some(*op) && args.len() == values.len()
                }
                (Expr::Match(_, arms), ExprKind::CopyMatch { arms: values, .. }) => {
                    arms.len() == values.len()
                }
                _ => false,
            }
        }
        _ => false,
    };
    if !matched {
        return Err(fail(format!(
            "source rule mismatch at instance {instance}, source {source_slot}"
        )));
    }
    if let (AstNode::Expression(expr), TypedNodeRef::Expression(actual)) = (source, node) {
        let expected = super::super::checked_type(
            checked,
            expr,
            &instances.list[instance].substitution,
            types,
        )?;
        if !super::super::signature_matches(&expected, actual.ty, typed, types)? {
            return Err(fail(
                "direct expression type differs from checked instance type",
            ));
        }
    }
    Ok(())
}
