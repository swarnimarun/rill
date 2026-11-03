use super::*;
pub(super) fn constructor(
    adt: usize,
    variant: usize,
    fields: &[ExprNode],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    functions: &[TypedFunction],
    adts: &[AdtType],
) -> Result<Type, String> {
    let definition = adts
        .get(adt)
        .ok_or("E-TYPED-ADT: invalid constructor type")?;
    if definition.record_fields.is_some() {
        return Err("E-TYPED-RECORD: positional record construction is unsupported".into());
    }
    let variant = definition
        .variants
        .get(variant)
        .ok_or("E-TYPED-ADT: invalid constructor tag")?;
    if fields.len() != variant.fields.len() {
        return Err("E-TYPED-ADT: wrong constructor arity".into());
    }
    for (value, ty) in fields.iter().zip(&variant.fields) {
        if verify_expr(value, scope, ids, functions, adts)? != *ty {
            return Err("E-TYPED-ADT: wrong constructor field type".into());
        }
    }
    Ok(Type::Adt {
        id: adt,
        owned: definition.owned,
    })
}
pub(super) fn match_value(
    value: &ExprNode,
    arms: &[AdtArm],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    functions: &[TypedFunction],
    adts: &[AdtType],
) -> Result<Type, String> {
    let Type::Adt { id, .. } = verify_expr(value, scope, ids, functions, adts)? else {
        return Err("E-TYPED-ADT: match requires nominal value".into());
    };
    let definition = &adts[id];
    if definition.record_fields.is_some() {
        return Err("E-TYPED-RECORD: positional record matching is unsupported".into());
    }

    let mut seen = HashSet::new();
    let mut result = None;
    let mut joined = None;
    let mut default_seen = false;
    for arm in arms {
        if default_seen || !seen.insert(arm.variant) {
            return Err("E-TYPED-ADT: duplicate or unreachable match tag".into());
        }
        let field_types = if let Some(tag) = arm.variant {
            definition
                .variants
                .get(tag)
                .ok_or("E-TYPED-ADT: invalid match tag")?
                .fields
                .clone()
        } else {
            if seen.len() > definition.variants.len() {
                return Err("E-TYPED-ADT: unreachable default".into());
            }
            default_seen = true;
            vec![value.ty]
        };
        if arm.fields.len() != field_types.len() {
            return Err("E-TYPED-ADT: wrong pattern arity".into());
        }
        let mut branch = scope.clone();
        for (&id, &ty) in arm.fields.iter().zip(&field_types) {
            match id {
                Some(id) => {
                    if !ids.insert(id) {
                        return Err("E-TYPED: duplicate local identity".into());
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
                None if ty.is_owner() => {
                    return Err("E-TYPED-ADT: missing owner payload cleanup".into())
                }
                None => {}
            }
        }
        let ty = verify_expr(&arm.body, &mut branch, ids, functions, adts)?;
        if result.is_some_and(|expected| expected != ty) {
            return Err("E-TYPED-ADT: inconsistent arm result".into());
        }
        result = Some(ty);
        let mut expected = Vec::new();
        for (id, ty) in arm.fields.iter().zip(&field_types).rev() {
            if let Some(id) = id {
                if ty.is_owner() {
                    let local = branch.get(id).ok_or("E-TYPED-ADT: missing payload state")?;
                    if let Some(entry) = local.initialization.cleanup_entry(*id, *ty, adts)? {
                        expected.push(entry);
                    }
                }
            }
        }
        if arm.cleanup != expected {
            return Err("E-TYPED-ADT: invalid payload cleanup".into());
        }
        for id in arm.fields.iter().flatten() {
            branch.remove(id);
        }
        if let Some(first) = &joined {
            if !same_owner_states(first, &branch) {
                return Err("E-TYPED-ADT: incompatible arm ownership".into());
            }
        } else {
            joined = Some(branch);
        }
    }
    if !default_seen && seen.len() != definition.variants.len() {
        return Err("E-TYPED-ADT: nonexhaustive match".into());
    }
    if let Some(joined) = joined {
        *scope = joined;
    }
    result.ok_or_else(|| "E-TYPED-ADT: empty match".into())
}
