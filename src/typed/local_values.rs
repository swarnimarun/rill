//! Capture-and-shape elaboration of the checker's nonexpansive local schemes.
//!
//! A shape is a compile-time product/tag tree, never a value at an arbitrary
//! concrete type. Every concrete leaf is evaluated once at the original let
//! into a fresh immutable Copy slot. Aliases share those slots. At each use,
//! the requested closed type determines concrete constructors and projections;
//! the ordinary IR verifier then checks every operation and capture identity.
//! This is a lowering construction, not a proof that source and IR coincide.
use super::*;
use std::rc::Rc;

pub(super) const MAX_WORK: usize = 100_000;
pub(super) const MAX_CAPTURES: usize = 256;
const MAX_DEPTH: usize = 128;
const COMPLEXITY: &str = "E-TYPED-LOCAL-COMPLEXITY: local shape elaboration limit exceeded";
pub(super) const CAPTURE_NAME: &str = "$local_capture";
const INVALID: &str = "E-TYPED-LOCAL: invalid polymorphic value shape";

pub(super) struct Budget {
    remaining: usize,
    captures: usize,
    copy_types: HashSet<Type>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            remaining: MAX_WORK,
            captures: 0,
            copy_types: HashSet::new(),
        }
    }
}
impl Budget {
    fn charge(&mut self) -> Result<(), String> {
        self.charge_many(1)
    }
    fn charge_many(&mut self, count: usize) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(count).ok_or(COMPLEXITY)?;
        Ok(())
    }
    fn capture(&mut self) -> Result<(), String> {
        if self.captures >= MAX_CAPTURES {
            return Err(COMPLEXITY.into());
        }
        self.captures += 1;
        Ok(())
    }
}

pub(super) enum Shape {
    Capture {
        local: LocalId,
        ty: Type,
    },
    Constructor {
        nominal: String,
        name: String,
        fields: Vec<Rc<Shape>>,
        height: usize,
    },
    Record {
        nominal: String,
        fields: Vec<(usize, Rc<Shape>)>,
        height: usize,
    },
}

impl Shape {
    fn height(&self) -> usize {
        match self {
            Self::Capture { .. } => 0,
            Self::Constructor { height, .. } | Self::Record { height, .. } => *height,
        }
    }
}
fn product_height<'a>(fields: impl Iterator<Item = &'a Rc<Shape>>) -> Result<usize, String> {
    let height = fields.map(|field| field.height()).max().unwrap_or(0) + 1;
    if height > MAX_DEPTH {
        return Err(COMPLEXITY.into());
    }
    Ok(height)
}

impl Lower<'_, '_> {
    pub(super) fn local_shape(&self, expr: &Expr) -> Option<Rc<Shape>> {
        self.checked
            .local_use(expr)
            .and_then(|id| self.shapes.get(&id))
            .cloned()
    }

    pub(super) fn polymorphic_let(
        &mut self,
        expr: &Expr,
        identity: usize,
    ) -> Result<ExprNode, String> {
        let Expr::Let(name, value, body) = expr else {
            return Err(INVALID.into());
        };
        let mut captures = Vec::new();
        let shape = self.capture_shape(value, &mut captures, 0)?;
        // One concrete capture product avoids introducing a nested Let per
        // leaf. It has exactly the captured field types, never a default type
        // for an unresolved phantom parameter. Written field order is retained.
        let environment = if captures.is_empty() {
            None
        } else {
            let fields: Vec<_> = captures.iter().map(|(_, value)| value.ty).collect();
            let adt = self.capture_record(&fields)?;
            let root = self.fresh()?;
            for (field, (capture, _)) in captures.iter().enumerate() {
                self.capture_places.insert(
                    *capture,
                    FieldPlace {
                        root,
                        path: vec![FieldStep { record: adt, field }],
                    },
                );
            }
            Some((root, adt))
        };
        // A shadowed source local is absent while the shape is in scope. The
        // checker's stable local identity, not this spelling, resolves all uses.
        let previous = self.locals.remove(name);
        if self.shapes.insert(identity, shape).is_some() {
            return Err(INVALID.into());
        }
        let lowered = self.expr(body);
        self.shapes.remove(&identity);
        for (capture, _) in &captures {
            self.capture_places.remove(capture);
        }
        if let Some(previous) = previous {
            self.locals.insert(name.clone(), previous);
        }
        let body = lowered?;
        if let Some((root, adt)) = environment {
            Ok(ExprNode {
                ty: body.ty,
                kind: ExprKind::Let {
                    local: root,
                    mutable: false,
                    value: Box::new(ExprNode {
                        ty: Type::Adt {
                            id: adt,
                            owned: false,
                        },
                        kind: ExprKind::Record {
                            adt,
                            fields: captures
                                .into_iter()
                                .enumerate()
                                .map(|(index, (_, value))| (index, value))
                                .collect(),
                        },
                    }),
                    body: Box::new(body),
                },
            })
        } else {
            Ok(body)
        }
    }

    fn capture_record(&mut self, fields: &[Type]) -> Result<usize, String> {
        // '$' cannot occur in an accepted source declaration. Identity includes
        // the exact concrete payload sequence, so equal environments can share
        // their product layout without conflating source binding identities.
        if let Some(id) = self
            .registry
            .types
            .iter()
            .position(|adt| adt.origin == AdtOrigin::LocalCapture && adt.arguments == fields)
        {
            return Ok(id);
        }
        if self.registry.types.len() >= 1024 {
            return Err("E-TYPED-ADT: excessive capture layouts".into());
        }
        let id = self.registry.types.len();
        self.registry.types.push(AdtType {
            origin: AdtOrigin::LocalCapture,
            name: CAPTURE_NAME.into(),
            arguments: fields.to_vec(),
            owned: false,
            record_fields: Some(
                (0..fields.len())
                    .map(|index| format!("capture_{index}"))
                    .collect(),
            ),
            variants: vec![AdtVariant {
                name: CAPTURE_NAME.into(),
                fields: fields.to_vec(),
            }],
        });
        Ok(id)
    }

    fn capture_value(&self, local: LocalId, ty: Type) -> Result<ExprNode, String> {
        let place = self.capture_places.get(&local).ok_or(INVALID)?.clone();
        Ok(ExprNode {
            ty,
            kind: ExprKind::FieldRead(place),
        })
    }

    fn capture_shape(
        &mut self,
        expr: &Expr,
        captures: &mut Vec<(LocalId, ExprNode)>,
        depth: usize,
    ) -> Result<Rc<Shape>, String> {
        self.local_work.charge()?;
        if depth > MAX_DEPTH {
            return Err(COMPLEXITY.into());
        }
        if let Some(shape) = self.local_shape(expr) {
            // O(1) sharing is essential: a chain of Pair(previous, previous)
            // aliases must not expand before the materialization budget runs.
            return Ok(shape);
        }
        let shape = match expr {
            Expr::Int(_) | Expr::Bool(_) | Expr::Unit | Expr::Str(_) => {
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) if self.checked.local_use(expr).is_some() => {
                let local = self.locals.get(name).ok_or(INVALID)?;
                if local.mutable
                    || local.mode != ParamMode::Value
                    || !local.initialization.fully_initialized()
                    || field_loans::conflicts(local, &[], false)
                {
                    return Err(INVALID.into());
                }
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) if self.native.constant_id(name).is_some() => {
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) => self.constructor_shape(expr, name, &[], captures, depth)?,
            Expr::Call(name, args) => self.constructor_shape(expr, name, args, captures, depth)?,
            Expr::Record(name, fields) => {
                let info = self.checked.record_info(expr).ok_or(INVALID)?;
                let decl = self
                    .checked
                    .program
                    .types
                    .iter()
                    .find(|d| d.name == *name)
                    .ok_or(INVALID)?;
                let names = decl.record_fields.as_ref().ok_or(INVALID)?;
                if names.len() != fields.len() || info.field_indices.len() != fields.len() {
                    return Err(INVALID.into());
                }
                let indices = info.field_indices.clone();
                let mut seen = HashSet::new();
                let mut result = Vec::new();
                for ((name, value), index) in fields.iter().zip(indices) {
                    if names.get(index) != Some(name) || !seen.insert(index) {
                        return Err(INVALID.into());
                    }
                    result.push((index, self.capture_shape(value, captures, depth + 1)?));
                }
                Shape::Record {
                    nominal: decl.name.clone(),
                    height: product_height(result.iter().map(|(_, field)| field))?,
                    fields: result,
                }
            }
            // No calls, field reads, control flow, mutable reads, allocations or
            // traps may be turned into a deferred computation by this pass.
            _ => return Err(INVALID.into()),
        };
        Ok(Rc::new(shape))
    }

    fn capture_leaf(
        &mut self,
        expr: &Expr,
        captures: &mut Vec<(LocalId, ExprNode)>,
    ) -> Result<Shape, String> {
        self.local_work.capture()?;
        let source = self.source_type(expr)?;
        let ty = self.registry.register(self.checked, &source)?;
        self.copy_dependency(ty)?;
        let value = self.expr(expr)?;
        let local = self.fresh()?;
        captures.push((local, value));
        Ok(Shape::Capture { local, ty })
    }

    fn constructor_shape(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[Expr],
        captures: &mut Vec<(LocalId, ExprNode)>,
        depth: usize,
    ) -> Result<Shape, String> {
        let Some(InferredType::Nominal(nominal, _)) = self.checked.expression_type(expr) else {
            return Err(INVALID.into());
        };
        let decl = self
            .checked
            .program
            .types
            .iter()
            .find(|d| d.name == *nominal)
            .ok_or(INVALID)?;
        if decl.record_fields.is_some() {
            return Err(INVALID.into());
        }
        let variant = decl
            .variants
            .iter()
            .find(|v| v.name == name)
            .ok_or(INVALID)?;
        if variant.fields.len() != args.len() {
            return Err(INVALID.into());
        }
        let mut fields = Vec::new();
        for arg in args {
            fields.push(self.capture_shape(arg, captures, depth + 1)?);
        }
        Ok(Shape::Constructor {
            nominal: nominal.clone(),
            name: name.into(),
            height: product_height(fields.iter())?,
            fields,
        })
    }

    // Owner dependencies include inactive variants (the layout's owner flag)
    // and phantom arguments. Never turn an owner instantiation into a copy just
    // because this particular constructor has no live owner payload.
    fn copy_dependency(&mut self, ty: Type) -> Result<(), String> {
        require_copy(ty, &self.registry.types, &mut self.local_work)
    }

    pub(super) fn polymorphic_use(
        &mut self,
        expr: &Expr,
        mut shape: Rc<Shape>,
    ) -> Result<ExprNode, String> {
        let source = self.source_type(expr)?;
        let target = self.registry.register(self.checked, &source)?;
        self.copy_dependency(target)?;
        if let Expr::Field(_, _) = expr {
            let info = self.checked.field_info(expr).ok_or(INVALID)?;
            if info.is_empty() || info.len() > 16 {
                return Err(INVALID.into());
            }
            for (offset, step) in info.iter().enumerate() {
                self.local_work.charge()?;
                match &*shape {
                    Shape::Record {
                        nominal, fields, ..
                    } => {
                        if nominal != &step.record || fields.len() != step.arity {
                            return Err(INVALID.into());
                        }
                        shape = fields
                            .iter()
                            .find(|(index, _)| *index == step.index)
                            .ok_or(INVALID)?
                            .1
                            .clone();
                    }
                    Shape::Capture { local, ty } => {
                        let mut ty = *ty;
                        let mut path = Vec::new();
                        for step in &info[offset..] {
                            self.local_work.charge()?;
                            let Type::Adt { id, owned: false } = ty else {
                                return Err(INVALID.into());
                            };
                            let adt = &self.registry.types[id];
                            if adt.record_fields.is_none()
                                || adt.name != step.record
                                || adt.variants[0].fields.len() != step.arity
                            {
                                return Err(INVALID.into());
                            }
                            ty = *adt.variants[0].fields.get(step.index).ok_or(INVALID)?;
                            path.push(FieldStep {
                                record: id,
                                field: step.index,
                            });
                        }
                        if ty != target {
                            return Err(INVALID.into());
                        }
                        // Keep the source's sixteen-step limit independent
                        // of the internal environment field. A concrete Copy
                        // temporary, not an extra source projection, holds the
                        // frozen record before following its original path.
                        let value = self.capture_value(
                            *local,
                            match &*shape {
                                Shape::Capture { ty, .. } => *ty,
                                _ => unreachable!(),
                            },
                        )?;
                        let root = self.fresh()?;
                        return Ok(ExprNode {
                            ty,
                            kind: ExprKind::Let {
                                local: root,
                                mutable: false,
                                value: Box::new(value),
                                body: Box::new(ExprNode {
                                    ty,
                                    kind: ExprKind::FieldRead(FieldPlace { root, path }),
                                }),
                            },
                        });
                    }
                    _ => return Err(INVALID.into()),
                }
            }
        } else if !matches!(expr, Expr::Var(_)) {
            return Err(INVALID.into());
        }
        self.materialize(&shape, target, 0)
    }

    fn materialize(
        &mut self,
        shape: &Shape,
        target: Type,
        depth: usize,
    ) -> Result<ExprNode, String> {
        // Charge before following DAG edges or allocating the expanded IR.
        self.local_work.charge()?;
        if depth > MAX_DEPTH {
            return Err(COMPLEXITY.into());
        }
        self.copy_dependency(target)?;
        let kind = match shape {
            Shape::Capture { local, ty } => {
                if *ty != target {
                    return Err(INVALID.into());
                }
                return self.capture_value(*local, target);
            }
            Shape::Constructor {
                nominal,
                name,
                fields,
                ..
            } => {
                let Type::Adt { id, owned: false } = target else {
                    return Err(INVALID.into());
                };
                let adt = &self.registry.types[id];
                if adt.name != *nominal || adt.record_fields.is_some() {
                    return Err(INVALID.into());
                }
                let variant = adt
                    .variants
                    .iter()
                    .position(|v| v.name == *name)
                    .ok_or(INVALID)?;
                let expected = adt.variants[variant].fields.clone();
                if fields.len() != expected.len() {
                    return Err(INVALID.into());
                }
                let mut values = Vec::new();
                for (field, expected) in fields.iter().zip(expected) {
                    values.push(self.materialize(field, expected, depth + 1)?);
                }
                ExprKind::Constructor {
                    adt: id,
                    variant,
                    fields: values,
                }
            }
            Shape::Record {
                nominal, fields, ..
            } => {
                let Type::Adt { id, owned: false } = target else {
                    return Err(INVALID.into());
                };
                let adt = &self.registry.types[id];
                if adt.name != *nominal || adt.record_fields.is_none() {
                    return Err(INVALID.into());
                }
                let expected = adt.variants[0].fields.clone();
                if fields.len() != expected.len() {
                    return Err(INVALID.into());
                }
                let mut values = Vec::new();
                let mut seen = HashSet::new();
                for (index, field) in fields {
                    if !seen.insert(index) {
                        return Err(INVALID.into());
                    }
                    let ty = *expected.get(*index).ok_or(INVALID)?;
                    values.push((*index, self.materialize(field, ty, depth + 1)?));
                }
                ExprKind::Record {
                    adt: id,
                    fields: values,
                }
            }
        };
        Ok(ExprNode { ty: target, kind })
    }
}

/// A capture product is explicitly compiler-generated and has no source
/// declaration identity. Its payloads and identity arguments are exactly equal.
pub(super) fn verify_capture_definition(adt: &AdtType, types: &[AdtType]) -> Result<(), String> {
    if adt.origin == AdtOrigin::Source {
        if adt.name == CAPTURE_NAME {
            return Err(INVALID.into());
        }
        return Ok(());
    }
    let Some(names) = &adt.record_fields else {
        return Err(INVALID.into());
    };
    if adt.name != CAPTURE_NAME
        || adt.owned
        || adt.arguments.is_empty()
        || adt.arguments.len() > MAX_CAPTURES
        || adt.variants.len() != 1
        || adt.variants[0].name != CAPTURE_NAME
        || adt.variants[0].fields != adt.arguments
        || names.len() != adt.arguments.len()
        || names
            .iter()
            .enumerate()
            .any(|(index, name)| *name != format!("capture_{index}"))
    {
        return Err(INVALID.into());
    }
    let mut budget = Budget::default();
    for &ty in &adt.arguments {
        require_copy(ty, types, &mut budget)?;
    }
    Ok(())
}

fn require_copy(ty: Type, types: &[AdtType], budget: &mut Budget) -> Result<(), String> {
    // Pay for every queued edge before growing the worklist, including the
    // initial root. Cached/duplicate visits still consume that reserved unit.
    budget.charge()?;
    let mut pending = vec![ty];
    let mut seen = HashSet::new();
    while let Some(ty) = pending.pop() {
        if budget.copy_types.contains(&ty) || !seen.insert(ty) {
            continue;
        }
        if ty.is_owner() {
            return Err("E-TYPED-LOCAL: owner-dependent instantiation is unsupported".into());
        }
        if let Type::Adt { id, .. } = ty {
            let adt = types.get(id).ok_or(INVALID)?;
            if adt.owned {
                return Err(INVALID.into());
            }
            budget.charge_many(adt.variants.len())?;
            let children = adt
                .variants
                .iter()
                .try_fold(adt.arguments.len(), |count, variant| {
                    count.checked_add(variant.fields.len()).ok_or(COMPLEXITY)
                })?;
            budget.charge_many(children)?;
            pending.extend(adt.arguments.iter().copied());
            // A nominal field can hide owner-valued phantom arguments even
            // when neither the root's arguments nor its physical layout owns.
            pending.extend(
                adt.variants
                    .iter()
                    .flat_map(|variant| &variant.fields)
                    .copied(),
            );
        }
    }
    budget.copy_types.extend(seen);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn work_and_capture_boundaries_are_exact() {
        let mut work = Budget::default();
        for _ in 0..MAX_WORK {
            work.charge().unwrap();
        }
        assert_eq!(work.remaining, 0);
        assert_eq!(work.charge().unwrap_err(), COMPLEXITY);
        assert_eq!(work.charge().unwrap_err(), COMPLEXITY);
        let mut captures = Budget::default();
        for _ in 0..MAX_CAPTURES {
            captures.capture().unwrap();
        }
        assert_eq!(captures.capture().unwrap_err(), COMPLEXITY);
    }

    #[test]
    fn copy_dependency_reserves_children_before_worklist_growth() {
        let types = vec![AdtType {
            origin: AdtOrigin::Source,
            name: "Wide".into(),
            arguments: vec![],
            owned: false,
            record_fields: None,
            variants: vec![AdtVariant {
                name: "Wide".into(),
                fields: vec![Type::Int; 3],
            }],
        }];
        let root = Type::Adt {
            id: 0,
            owned: false,
        };
        let mut exact = Budget {
            remaining: 5,
            ..Budget::default()
        };
        require_copy(root, &types, &mut exact).unwrap();
        assert_eq!(exact.remaining, 0); // root + variant metadata + three edges
        let mut short = Budget {
            remaining: 4,
            ..Budget::default()
        };
        assert_eq!(
            require_copy(root, &types, &mut short).unwrap_err(),
            COMPLEXITY
        );
        assert_eq!(
            short.remaining, 2,
            "failed batch is never partly queued or charged"
        );
        assert!(short.copy_types.is_empty());
    }

    #[test]
    fn shared_shape_height_is_bounded_without_expanding_the_dag() {
        let mut shape = Rc::new(Shape::Capture {
            local: LocalId(0),
            ty: Type::Int,
        });
        for _ in 0..MAX_DEPTH {
            let fields = vec![shape.clone(), shape.clone()];
            let height = product_height(fields.iter()).unwrap();
            shape = Rc::new(Shape::Constructor {
                nominal: "N".into(),
                name: "N".into(),
                fields,
                height,
            });
        }
        assert_eq!(shape.height(), MAX_DEPTH);
        assert_eq!(
            product_height([&shape].into_iter()).unwrap_err(),
            COMPLEXITY
        );
    }

    #[test]
    fn repeated_dag_uses_charge_before_materializing_each_concrete_copy() {
        let mut declarations = "type D0<a> = Leaf\n".to_string();
        let mut bindings = "let d0 = Leaf;".to_string();
        for level in 1..=12 {
            let previous = level - 1;
            declarations.push_str(&format!(
                "type D{level}<a> = Fork{level}(D{previous}<a>,D{previous}<a>)\n"
            ));
            bindings.push_str(&format!(
                "let d{level} = Fork{level}(d{previous},d{previous});"
            ));
        }
        declarations.push_str("fn take(x: D12<i32>) { 0 }\n");
        for uses in [6, 7] {
            let source = format!(
                "{declarations}fn main() {{ {bindings} {} }}",
                vec!["take(d12)"; uses].join(" + ")
            );
            let p = parser::parse(&source).unwrap();
            let checked = check::analyze(&p).unwrap();
            if uses == 6 {
                lower_main(&checked).unwrap();
            } else {
                assert_eq!(lower_main(&checked).unwrap_err(), COMPLEXITY);
            }
        }
    }
}
