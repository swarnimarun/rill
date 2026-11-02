use super::*;
#[allow(clippy::too_many_arguments)]
pub(super) fn construct(
    adt: usize,
    variant: usize,
    fields: &[ExprNode],
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<EvalArg, String> {
    let mut values = Vec::new();
    for field in fields {
        values.push(if field.ty.is_owner() {
            EvalArg::Owner(evaluate_owner_expr(
                field, locals, program, depth, state, owners,
            )?)
        } else {
            EvalArg::Value(evaluate_expr(field, locals, program, depth, state, owners)?)
        });
    }
    construct_values(adt, variant, values, program, state)
}
pub(super) fn construct_values(
    adt: usize,
    variant: usize,
    values: Vec<EvalArg>,
    program: &TypedMain,
    state: &mut EvalState,
) -> Result<EvalArg, String> {
    if program.adts[adt].owned {
        state
            .storage
            .try_reserve(1)
            .map_err(|_| "E-TYPED-ADT: interpreter storage exhausted")?;
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or("runtime trap")?;
        state.storage.insert(
            id,
            OwnerStorage::Adt {
                adt,
                variant,
                fields: values,
            },
        );
        Ok(EvalArg::Owner(OwnerHandle {
            id,
            mode: ParamMode::Value,
        }))
    } else {
        let fields = values
            .into_iter()
            .map(|v| match v {
                EvalArg::Value(v) => Ok(v),
                _ => Err("E-TYPED-ADT: owner in Copy constructor"),
            })
            .collect::<Result<_, _>>()?;
        Ok(EvalArg::Value(crate::interpreter::Value::Adt(
            program.adts[adt].variants[variant].name.clone(),
            fields,
        )))
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn match_value<T>(
    value: &ExprNode,
    arms: &[AdtArm],
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
    evaluate_body: EvaluateBody<T>,
) -> Result<T, String> {
    let Type::Adt { id: expected, .. } = value.ty else {
        return Err("E-TYPED-ADT: invalid match value".into());
    };
    let carrier = if value.ty.is_owner() {
        EvalArg::Owner(evaluate_owner_expr(
            value, locals, program, depth, state, owners,
        )?)
    } else {
        EvalArg::Value(evaluate_expr(value, locals, program, depth, state, owners)?)
    };
    let variant = match &carrier {
        EvalArg::Owner(handle) => match state.storage.get(&handle.id) {
            Some(OwnerStorage::Adt { adt, variant, .. }) if *adt == expected => *variant,
            _ => return Err("E-TYPED-ADT: invalid runtime enum".into()),
        },
        EvalArg::Value(crate::interpreter::Value::Adt(name, _)) => program.adts[expected]
            .variants
            .iter()
            .position(|v| v.name == *name)
            .ok_or("E-TYPED-ADT: wrong constructor")?,
        _ => return Err("E-TYPED-ADT: invalid carrier".into()),
    };
    let arm = arms
        .iter()
        .find(|a| a.variant == Some(variant) || a.variant.is_none())
        .ok_or("E-TYPED-ADT: missing runtime arm")?;
    let values = if arm.variant.is_none() {
        vec![carrier]
    } else {
        match carrier {
            EvalArg::Owner(handle) => match state.storage.remove(&handle.id) {
                Some(OwnerStorage::Adt { fields, .. }) => fields,
                _ => return Err("E-TYPED-ADT: missing carrier".into()),
            },
            EvalArg::Value(crate::interpreter::Value::Adt(_, fields)) => {
                fields.into_iter().map(EvalArg::Value).collect()
            }
            _ => return Err("E-TYPED-ADT: invalid carrier".into()),
        }
    };
    for (id, value) in arm.fields.iter().zip(values) {
        if let Some(id) = id {
            match value {
                EvalArg::Owner(handle) => {
                    owners.insert(*id, handle);
                }
                EvalArg::Value(value) => {
                    locals.insert(*id, value);
                }
            }
        }
    }
    let result = evaluate_body(&arm.body, locals, program, depth, state, owners);
    if result.is_ok() {
        for id in &arm.cleanup {
            let owner = owners
                .remove(id)
                .ok_or("E-TYPED-ADT: missing payload cleanup")?;
            state.release(owner.id)?;
        }
    }
    for id in arm.fields.iter().flatten() {
        locals.remove(id);
        owners.remove(id);
    }
    result
}
