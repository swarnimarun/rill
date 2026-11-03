//! Non-consuming patterns over closed Copy values. All projections are checked
//! against concrete nominal layouts, and coverage is rechecked at the IR boundary.
use super::*;
use crate::ast::{MatchArm, Pattern};

impl Lower<'_, '_> {
    pub(super) fn match_copy(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
    ) -> Result<ExprKind, String> {
        let value = self.expr(value)?;
        if value.ty.is_owner() {
            return Err("E-TYPED-MATCH: cannot copy an owner".into());
        }
        let outer = self.locals.clone();
        let mut lowered = Vec::new();
        let mut joined = None;
        for arm in arms {
            self.locals = outer.clone();
            let pattern = self.copy_pattern(&arm.pattern, value.ty)?;
            let body = self.expr(&arm.body)?;
            let after = outer
                .iter()
                .map(|(name, old)| {
                    let local = self
                        .locals
                        .get(name)
                        .filter(|l| l.id == old.id)
                        .cloned()
                        .unwrap_or_else(|| old.clone());
                    (name.clone(), local)
                })
                .collect();
            if let Some(first) = &joined {
                if !Self::same_states(first, &after) {
                    return Err("E-TYPED-MATCH: incompatible arm ownership".into());
                }
            } else {
                joined = Some(after);
            }
            lowered.push(CopyArm { pattern, body });
        }
        self.locals = joined.unwrap_or(outer);
        Ok(ExprKind::CopyMatch {
            value: Box::new(value),
            arms: lowered,
        })
    }

    fn copy_pattern(&mut self, pattern: &Pattern, ty: Type) -> Result<CopyPattern, String> {
        if ty.is_owner() {
            return Err("E-TYPED-MATCH: owner in Copy pattern".into());
        }
        Ok(match pattern {
            Pattern::Wildcard => CopyPattern::Wildcard,
            Pattern::Binding(name) => {
                let id = self.fresh()?;
                self.locals.insert(
                    name.clone(),
                    Local {
                        id,
                        ty,
                        mutable: false,
                        initialization: Initialization::full(ty, &self.registry.types)?,
                        loaned: false,
                        shared: 0,
                        field_loans: Vec::new(),
                        mode: ParamMode::Value,
                    },
                );
                CopyPattern::Binding(id)
            }
            Pattern::Int(n) => CopyPattern::Int(*n),
            Pattern::Bool(b) => CopyPattern::Bool(*b),
            Pattern::Unit => CopyPattern::Unit,
            Pattern::Constructor(name, fields) => {
                let Type::Adt { id, owned: false } = ty else {
                    return Err("E-TYPED-MATCH: constructor pattern requires Copy enum".into());
                };
                if self.registry.types[id].record_fields.is_some() {
                    return Err("E-TYPED-RECORD: positional record patterns are unsupported".into());
                }
                let variant = self.registry.types[id]
                    .variants
                    .iter()
                    .position(|v| v.name == *name)
                    .ok_or("E-TYPED-MATCH: wrong pattern constructor")?;
                let types = self.registry.types[id].variants[variant].fields.clone();
                if fields.len() != types.len() {
                    return Err("E-TYPED-MATCH: wrong pattern arity".into());
                }
                let fields = fields
                    .iter()
                    .zip(types)
                    .map(|(p, t)| self.copy_pattern(p, t))
                    .collect::<Result<_, _>>()?;
                CopyPattern::Constructor {
                    adt: id,
                    variant,
                    fields,
                }
            }
        })
    }
}

pub(super) fn verify_match(
    value: &ExprNode,
    arms: &[CopyArm],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    context: &VerifyContext<'_>,
) -> Result<Type, String> {
    let adts = context.adts;
    let ty = verify_expr(value, scope, ids, context)?;
    if ty.is_owner() {
        return Err("E-TYPED-MATCH: cannot copy an owner".into());
    }
    let mut result = None;
    let mut joined = None;
    let mut budget = 100_000usize;
    for arm in arms {
        let mut branch = scope.clone();
        let mut bindings = Vec::new();
        verify_pattern(&arm.pattern, ty, adts, &mut bindings, 0, &mut budget)?;
        for &(id, ty) in &bindings {
            if !ids.insert(id) {
                return Err("E-TYPED-MATCH: duplicate pattern binding identity".into());
            }
            branch.insert(
                id,
                Local {
                    id,
                    ty,
                    mutable: false,
                    initialization: Initialization::full(ty, adts)?,
                    loaned: false,
                    shared: 0,
                    field_loans: Vec::new(),
                    mode: ParamMode::Value,
                },
            );
        }
        let body_ty = verify_expr(&arm.body, &mut branch, ids, context)?;
        if result.is_some_and(|expected| expected != body_ty) {
            return Err("E-TYPED-MATCH: inconsistent arm result".into());
        }
        result = Some(body_ty);
        for (id, _) in bindings {
            branch.remove(&id);
        }
        if let Some(first) = &joined {
            if !same_owner_states(first, &branch) {
                return Err("E-TYPED-MATCH: incompatible arm ownership".into());
            }
        } else {
            joined = Some(branch);
        }
    }
    let rows = arms.iter().map(|a| vec![&a.pattern]).collect::<Vec<_>>();
    if !exhaustive(&[ty], &rows, adts, &mut budget)? {
        return Err("E-TYPED-MATCH: nonexhaustive match".into());
    }
    if let Some(joined) = joined {
        *scope = joined;
    }
    result.ok_or_else(|| "E-TYPED-MATCH: empty match".into())
}

fn spend(budget: &mut usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(1)
        .ok_or("E-TYPED-MATCH: pattern analysis budget exceeded")?;
    Ok(())
}
fn verify_pattern(
    pattern: &CopyPattern,
    ty: Type,
    adts: &[AdtType],
    bindings: &mut Vec<(LocalId, Type)>,
    depth: usize,
    budget: &mut usize,
) -> Result<(), String> {
    spend(budget)?;
    adts::validate_type(ty, adts)?;
    if ty.is_owner() || depth > 128 {
        return Err("E-TYPED-MATCH: owner or excessive pattern nesting".into());
    }
    match pattern {
        CopyPattern::Wildcard => {}
        CopyPattern::Binding(id) => bindings.push((*id, ty)),
        CopyPattern::Int(_) if ty == Type::Int => {}
        CopyPattern::Bool(_) if ty == Type::Bool => {}
        CopyPattern::Unit if ty == Type::Unit => {}
        CopyPattern::Constructor {
            adt,
            variant,
            fields,
        } if ty
            == (Type::Adt {
                id: *adt,
                owned: false,
            }) =>
        {
            if adts[*adt].record_fields.is_some() {
                return Err("E-TYPED-RECORD: positional record patterns are unsupported".into());
            }
            let variant = adts[*adt]
                .variants
                .get(*variant)
                .ok_or("E-TYPED-MATCH: invalid pattern tag")?;
            if variant.fields.len() != fields.len() {
                return Err("E-TYPED-MATCH: wrong pattern arity".into());
            }
            for (pattern, &ty) in fields.iter().zip(&variant.fields) {
                verify_pattern(pattern, ty, adts, bindings, depth + 1, budget)?;
            }
        }
        _ => return Err("E-TYPED-MATCH: pattern type mismatch".into()),
    }
    Ok(())
}
fn irrefutable(pattern: &CopyPattern) -> bool {
    matches!(pattern, CopyPattern::Wildcard | CopyPattern::Binding(_))
}

// Pattern-matrix specialization over independently verified concrete types.
// Wildcards are shared rather than expanded into recursively cloned patterns.
// A total row ends the check early, avoiding exponential products of wildcards.
fn exhaustive(
    types: &[Type],
    rows: &[Vec<&CopyPattern>],
    adts: &[AdtType],
    budget: &mut usize,
) -> Result<bool, String> {
    let mut pending = vec![(types.to_vec(), rows.to_vec())];
    while let Some((types, rows)) = pending.pop() {
        spend(budget)?;
        if rows.iter().any(|row| row.iter().all(|p| irrefutable(p))) {
            continue;
        }
        if rows.is_empty() {
            return Ok(false);
        }
        let Some((&first, rest)) = types.split_first() else {
            continue;
        };
        match first {
            Type::Adt { id, .. } => {
                for (tag, variant) in adts[id].variants.iter().enumerate() {
                    let mut next_types = variant.fields.clone();
                    next_types.extend_from_slice(rest);
                    let mut next_rows = Vec::new();
                    for row in &rows {
                        *budget = budget
                            .checked_sub(next_types.len().max(1))
                            .ok_or("E-TYPED-MATCH: pattern analysis budget exceeded")?;
                        let mut next = match row[0] {
                            p if irrefutable(p) => {
                                vec![&CopyPattern::Wildcard; variant.fields.len()]
                            }
                            CopyPattern::Constructor {
                                variant, fields, ..
                            } if *variant == tag => fields.iter().collect(),
                            _ => continue,
                        };
                        next.extend_from_slice(&row[1..]);
                        next_rows.push(next);
                    }
                    pending.push((next_types, next_rows));
                }
            }
            Type::Bool => {
                for b in [false, true] {
                    let mut next = Vec::new();
                    for row in &rows {
                        *budget = budget
                            .checked_sub(row.len())
                            .ok_or("E-TYPED-MATCH: pattern analysis budget exceeded")?;
                        if irrefutable(row[0]) || matches!(row[0], CopyPattern::Bool(v) if *v == b)
                        {
                            next.push(row[1..].to_vec());
                        }
                    }
                    pending.push((rest.to_vec(), next));
                }
            }
            _ => {
                let mut next = Vec::new();
                for row in &rows {
                    *budget = budget
                        .checked_sub(row.len())
                        .ok_or("E-TYPED-MATCH: pattern analysis budget exceeded")?;
                    if irrefutable(row[0])
                        || (first == Type::Unit && matches!(row[0], CopyPattern::Unit))
                    {
                        next.push(row[1..].to_vec());
                    }
                }
                pending.push((rest.to_vec(), next));
            }
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_match<T>(
    value: &ExprNode,
    arms: &[CopyArm],
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
    evaluate_body: EvaluateBody<T>,
) -> Result<T, String> {
    let value = evaluate_expr(value, locals, program, depth, state, owners)?;
    for arm in arms {
        let mut bindings = Vec::new();
        if matches_value(&arm.pattern, &value, program, &mut bindings) {
            let ids = bindings.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            locals.extend(bindings);
            let result = evaluate_body(&arm.body, locals, program, depth, state, owners);
            for id in ids {
                locals.remove(&id);
            }
            return result;
        }
    }
    Err("E-TYPED-MATCH: missing runtime arm".into())
}
fn matches_value(
    pattern: &CopyPattern,
    value: &crate::interpreter::Value,
    program: &TypedMain,
    bindings: &mut Vec<(LocalId, crate::interpreter::Value)>,
) -> bool {
    use crate::interpreter::Value;
    match (pattern, value) {
        (CopyPattern::Wildcard, _) => true,
        (CopyPattern::Binding(id), value) => {
            bindings.push((*id, value.clone()));
            true
        }
        (CopyPattern::Int(a), Value::Int(b)) => a == b,
        (CopyPattern::Bool(a), Value::Bool(b)) => a == b,
        (CopyPattern::Unit, Value::Unit) => true,
        (
            CopyPattern::Constructor {
                adt,
                variant,
                fields,
            },
            Value::Adt(name, values),
        ) => {
            *name == program.adts[*adt].variants[*variant].name
                && fields.len() == values.len()
                && fields
                    .iter()
                    .zip(values)
                    .all(|(p, v)| matches_value(p, v, program, bindings))
        }
        _ => false,
    }
}
