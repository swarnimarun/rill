//! Typed original-slot identities. A path is nominal, never a byte offset.
use super::{adts, AdtType, FieldPlace, FieldStep, LocalId, Type};

/// The empty path denotes the whole local. Forwarded paths may exceed the
/// source operand's 16-step limit; composition does not create a new operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub root: LocalId,
    pub path: Vec<FieldStep>,
}

impl Place {
    pub fn root(root: LocalId) -> Self {
        Self {
            root,
            path: Vec::new(),
        }
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.root == other.root && paths_overlap(&self.path, &other.path)
    }
}

impl From<&FieldPlace> for Place {
    fn from(place: &FieldPlace) -> Self {
        Self {
            root: place.root,
            path: place.path.clone(),
        }
    }
}

/// Valid only after both paths have been checked against their root type.
pub(super) fn paths_overlap(a: &[FieldStep], b: &[FieldStep]) -> bool {
    a.iter().zip(b).all(|(a, b)| a == b)
}

/// Layout/path checking does not imply Copy, initialization or authority.
/// Callers must establish these independently for the requested operation.
pub(super) fn projected_type(
    mut ty: Type,
    path: &[FieldStep],
    types: &[AdtType],
) -> Result<Type, String> {
    adts::validate_type(ty, types)?;
    for step in path {
        let Type::Adt { id, .. } = ty else {
            return Err("E-TYPED-PLACE: projection requires a record".into());
        };
        if id != step.record {
            return Err("E-TYPED-PLACE: projection nominal identity mismatch".into());
        }
        let fields = record_fields(id, types)?;
        ty = *fields
            .get(step.field)
            .ok_or("E-TYPED-PLACE: field index out of range")?;
        adts::validate_type(ty, types)?;
    }
    Ok(ty)
}

pub(super) fn record_fields(id: usize, types: &[AdtType]) -> Result<&[Type], String> {
    let record = types
        .get(id)
        .ok_or("E-TYPED-PLACE: unknown record identity")?;
    let names = record
        .record_fields
        .as_ref()
        .ok_or("E-TYPED-PLACE: enum projection is unsupported")?;
    if record.variants.len() != 1
        || names.len() != record.variants[0].fields.len()
        || names.len() > 4096
    {
        return Err("E-TYPED-PLACE: malformed record layout".into());
    }
    Ok(&record.variants[0].fields)
}

/// All move spellings use this same authority/interval/state transition.
/// Moving out does not require mutation authority, but must originate in a
/// value-owned binding, never in a read/inout parameter's storage.
pub(super) fn take(
    local: &mut super::Local,
    path: &[FieldStep],
    types: &[AdtType],
) -> Result<Type, String> {
    let selected = projected_type(local.ty, path, types)?;
    if !selected.is_owner()
        || local.mode != crate::ast::ParamMode::Value
        || super::field_loans::conflicts(local, path, true)
    {
        return Err("E-TYPED-OWNER: invalid move authority or overlapping loan".into());
    }
    local.initialization.take(path)?;
    Ok(selected)
}
