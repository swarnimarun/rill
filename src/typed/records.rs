//! Finite Copy records: nominal products and independently checked field paths.
use super::*;

pub(super) fn verify_definition(adt: &AdtType) -> Result<(), String> {
    if let Some(names) = &adt.record_fields {
        let mut seen = HashSet::new();
        if adt.owned
            || adt.variants.len() != 1
            || names.len() != adt.variants[0].fields.len()
            || names
                .iter()
                .any(|name| name.is_empty() || !seen.insert(name))
        {
            return Err("E-TYPED-RECORD: invalid Copy record metadata".into());
        }
    }
    Ok(())
}
fn definition(id: usize, adts: &[AdtType]) -> Result<&AdtType, String> {
    let adt = adts
        .get(id)
        .ok_or("E-TYPED-RECORD: unknown record identity")?;
    if adt.record_fields.is_none() || adt.owned || adt.variants.len() != 1 {
        return Err("E-TYPED-RECORD: expected a Copy record".into());
    }
    Ok(adt)
}
impl Lower<'_, '_> {
    pub(super) fn record(
        &mut self,
        expr: &Expr,
        ty: Type,
        fields: &[(String, Expr)],
    ) -> Result<ExprKind, String> {
        let Type::Adt { id, owned: false } = ty else {
            return Err("E-TYPED-RECORD: expected a Copy record".into());
        };
        definition(id, &self.registry.types)?;
        let info = self
            .checked
            .record_info(expr)
            .ok_or("E-TYPED-RECORD: missing construction metadata")?;
        if info.constructor != self.registry.types[id].variants[0].name
            || info.field_indices.len() != fields.len()
        {
            return Err("E-TYPED-RECORD: inconsistent construction metadata".into());
        }
        let indices = info.field_indices.clone();
        let mut lowered = Vec::new();
        for ((name, value), index) in fields.iter().zip(indices) {
            if self.registry.types[id]
                .record_fields
                .as_ref()
                .and_then(|names| names.get(index))
                != Some(name)
            {
                return Err("E-TYPED-RECORD: inconsistent field identity".into());
            }
            lowered.push((index, self.expr(value)?));
        }
        Ok(ExprKind::Record {
            adt: id,
            fields: lowered,
        })
    }
    pub(super) fn field_place(
        &self,
        expr: &Expr,
        root: &str,
        write: bool,
    ) -> Result<FieldPlace, String> {
        let local = self
            .locals
            .get(root)
            .ok_or("E-TYPED-RECORD: unknown record root")?;
        let info = self
            .checked
            .field_info(expr)
            .ok_or("E-TYPED-RECORD: missing field metadata")?;
        let mut ty = local.ty;
        let mut path = Vec::new();
        for field in info {
            let Type::Adt { id, owned: false } = ty else {
                return Err("E-TYPED-RECORD: field on non-record".into());
            };
            let adt = definition(id, &self.registry.types)?;
            if adt.name != field.record || adt.variants[0].fields.len() != field.arity {
                return Err("E-TYPED-RECORD: inconsistent projection metadata".into());
            }
            ty = *adt.variants[0]
                .fields
                .get(field.index)
                .ok_or("E-TYPED-RECORD: unknown field")?;
            path.push(FieldStep {
                record: id,
                field: field.index,
            });
        }
        let place = FieldPlace {
            root: local.id,
            path,
        };
        projected_type(local.ty, &place.path, &self.registry.types)?;
        check_access(local, &place.path, write)?;
        Ok(place)
    }
}
fn check_access(local: &Local, path: &[FieldStep], write: bool) -> Result<(), String> {
    if local.ty.is_owner()
        || !local.initialization.fully_initialized()
        || (write && !local.mutable)
        || field_loans::conflicts(local, path, write)
    {
        return Err("E-TYPED-RECORD: conflicting or immutable field access".into());
    }
    Ok(())
}
pub(super) fn projected_type(
    ty: Type,
    path: &[FieldStep],
    adts: &[AdtType],
) -> Result<Type, String> {
    if path.is_empty() || path.len() > 16 {
        return Err("E-TYPED-RECORD: field path must contain 1 to 16 steps".into());
    }
    if ty.is_owner() {
        return Err("E-TYPED-RECORD: owner field is unsupported".into());
    }
    let result = places::projected_type(ty, path, adts)?;
    if result.is_owner() {
        return Err("E-TYPED-RECORD: owner field is unsupported".into());
    }
    Ok(result)
}
pub(super) fn verify_place(
    place: &FieldPlace,
    scope: &HashMap<LocalId, Local>,
    adts: &[AdtType],
    write: bool,
) -> Result<Type, String> {
    let local = scope
        .get(&place.root)
        .ok_or("E-TYPED-RECORD: root outside scope")?;
    check_access(local, &place.path, write)?;
    projected_type(local.ty, &place.path, adts)
}
pub(super) fn verify_constructor(
    id: usize,
    fields: &[(usize, ExprNode)],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    functions: &[TypedFunction],
    adts: &[AdtType],
) -> Result<Type, String> {
    let adt = definition(id, adts)?;
    if fields.len() != adt.variants[0].fields.len() {
        return Err("E-TYPED-RECORD: incomplete record construction".into());
    }
    let mut seen = HashSet::new();
    for (index, value) in fields {
        let expected = adt.variants[0]
            .fields
            .get(*index)
            .ok_or("E-TYPED-RECORD: field index out of range")?;
        if !seen.insert(index) {
            return Err("E-TYPED-RECORD: duplicate field initialization".into());
        }
        if verify_expr(value, scope, ids, functions, adts)? != *expected {
            return Err("E-TYPED-RECORD: incorrect field type".into());
        }
    }
    Ok(Type::Adt { id, owned: false })
}
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_constructor(
    id: usize,
    fields: &[(usize, ExprNode)],
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<crate::interpreter::Value, String> {
    let adt = &program.adts[id];
    let names = adt
        .record_fields
        .as_ref()
        .ok_or("E-TYPED-RECORD: missing names")?;
    let mut values = vec![None; names.len()];
    for (index, value) in fields {
        values[*index] = Some(evaluate_expr(value, locals, program, depth, state, owners)?);
    }
    let fields = names
        .iter()
        .cloned()
        .zip(values)
        .map(|(name, value)| {
            Ok((
                name,
                value.ok_or("E-TYPED-RECORD: missing initialized field")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(crate::interpreter::Value::Record(adt.name.clone(), fields))
}
pub(super) fn field_value<'a>(
    mut value: &'a crate::interpreter::Value,
    path: &[FieldStep],
) -> Result<&'a crate::interpreter::Value, String> {
    for step in path {
        let crate::interpreter::Value::Record(_, fields) = value else {
            return Err("E-TYPED-RECORD: invalid runtime record".into());
        };
        value = &fields
            .get(step.field)
            .ok_or("E-TYPED-RECORD: missing runtime field")?
            .1;
    }
    Ok(value)
}
pub(super) fn field_value_mut<'a>(
    mut value: &'a mut crate::interpreter::Value,
    path: &[FieldStep],
) -> Result<&'a mut crate::interpreter::Value, String> {
    for step in path {
        let crate::interpreter::Value::Record(_, fields) = value else {
            return Err("E-TYPED-RECORD: invalid runtime record".into());
        };
        value = &mut fields
            .get_mut(step.field)
            .ok_or("E-TYPED-RECORD: missing runtime field")?
            .1;
    }
    Ok(value)
}
