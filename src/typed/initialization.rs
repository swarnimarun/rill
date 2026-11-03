//! Type-shaped definite initialization and exact, static scope cleanup.
//!
//! Lowering and verification construct separate states and replay transitions.
//! No state or cleanup annotation supplied by lowering is a verifier fact.
use super::{adts, places, AdtType, FieldStep, LocalId, Type};

/// An enum is atomic here: only records admit independently moved fields.
/// A materialized record with dead children is not itself a dead place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Initialization {
    Dead,
    LiveAtom,
    Record {
        record: usize,
        fields: Vec<Initialization>,
    },
}

/// Children are in declaration order and must execute in reverse order.
/// Record cleanup also retires its bookkeeping shell, even with no live owner
/// children. Such shells are not Rill heap allocations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupTree {
    SkipMoved,
    SkipCopy,
    DropWholeOwner,
    Record {
        record: usize,
        fields: Vec<CleanupTree>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cleanup {
    pub local: LocalId,
    pub tree: CleanupTree,
}

// The existing 1 MiB inline layout budget charges at least 64 bytes per node.
const MAX_NODES: usize = 16_384;

impl Initialization {
    pub(super) fn full(ty: Type, types: &[AdtType]) -> Result<Self, String> {
        let mut remaining = MAX_NODES;
        Self::build(ty, types, &mut remaining, 0)
    }

    fn build(
        ty: Type,
        types: &[AdtType],
        remaining: &mut usize,
        depth: usize,
    ) -> Result<Self, String> {
        if depth > 128 {
            return Err("E-TYPED-INIT: excessive record nesting".into());
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or("E-TYPED-INIT: initialization tree too large")?;
        adts::validate_type(ty, types)?;
        if let Type::Adt { id, .. } = ty {
            if types[id].record_fields.is_some() {
                let mut fields = Vec::new();
                for &field in places::record_fields(id, types)? {
                    if matches!(field, Type::Adt { id: child, .. } if child >= id) {
                        return Err("E-TYPED-INIT: recursive or unordered record layout".into());
                    }
                    fields.push(Self::build(field, types, remaining, depth + 1)?);
                }
                return Ok(Self::Record { record: id, fields });
            }
        }
        Ok(Self::LiveAtom)
    }

    pub(super) fn fully_initialized(&self) -> bool {
        match self {
            Self::Dead => false,
            Self::LiveAtom => true,
            Self::Record { fields, .. } => fields.iter().all(Self::fully_initialized),
        }
    }

    pub(super) fn materialized(&self) -> bool {
        !matches!(self, Self::Dead)
    }

    pub(super) fn selected(&self, path: &[FieldStep]) -> Result<&Self, String> {
        let mut state = self;
        for step in path {
            match state {
                Self::Record { record, fields } if *record == step.record => {
                    state = fields
                        .get(step.field)
                        .ok_or("E-TYPED-INIT: field state out of range")?;
                }
                Self::Dead => return Err("E-TYPED-INIT: ancestor was moved".into()),
                _ => return Err("E-TYPED-INIT: state does not match record path".into()),
            }
        }
        Ok(state)
    }

    /// Type, ownership, borrowed authority and intervals are checked by the
    /// caller. This transition is atomic on failure and never revives storage.
    pub(super) fn take(&mut self, path: &[FieldStep]) -> Result<(), String> {
        if !self.selected(path)?.fully_initialized() {
            return Err("E-TYPED-INIT: move requires a fully initialized place".into());
        }
        let mut state = self;
        for step in path {
            let Self::Record { fields, .. } = state else {
                unreachable!()
            };
            state = &mut fields[step.field];
        }
        *state = Self::Dead;
        Ok(())
    }

    /// Derive cleanup from the caller's independently replayed state. Exact
    /// equality with the IR annotation detects reordered or omitted children.
    pub(super) fn cleanup(&self, ty: Type, types: &[AdtType]) -> Result<CleanupTree, String> {
        let mut remaining = MAX_NODES;
        self.cleanup_bounded(ty, types, &mut remaining, 0)
    }

    fn cleanup_bounded(
        &self,
        ty: Type,
        types: &[AdtType],
        remaining: &mut usize,
        depth: usize,
    ) -> Result<CleanupTree, String> {
        if depth > 128 {
            return Err("E-TYPED-INIT: excessive record nesting".into());
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or("E-TYPED-INIT: cleanup tree too large")?;
        adts::validate_type(ty, types)?;
        if matches!(self, Self::Dead) {
            return if ty.is_owner() {
                Ok(CleanupTree::SkipMoved)
            } else {
                Err("E-TYPED-INIT: Copy storage cannot be moved".into())
            };
        }
        if let Type::Adt { id, .. } = ty {
            if types[id].record_fields.is_some() {
                let Self::Record { record, fields } = self else {
                    return Err("E-TYPED-INIT: record state was collapsed".into());
                };
                let field_types = places::record_fields(id, types)?;
                if *record != id || fields.len() != field_types.len() {
                    return Err("E-TYPED-INIT: incorrect record state shape".into());
                }
                let mut cleanup = Vec::new();
                for (field, &field_ty) in fields.iter().zip(field_types) {
                    if matches!(field_ty, Type::Adt { id: child, .. } if child >= id) {
                        return Err("E-TYPED-INIT: recursive or unordered record layout".into());
                    }
                    cleanup.push(field.cleanup_bounded(field_ty, types, remaining, depth + 1)?);
                }
                return if ty.is_owner() {
                    Ok(CleanupTree::Record {
                        record: id,
                        fields: cleanup,
                    })
                } else {
                    Ok(CleanupTree::SkipCopy)
                };
            }
        }
        if !matches!(self, Self::LiveAtom) {
            return Err("E-TYPED-INIT: atom has record state".into());
        }
        Ok(if ty.is_owner() {
            CleanupTree::DropWholeOwner
        } else {
            CleanupTree::SkipCopy
        })
    }

    pub(super) fn cleanup_entry(
        &self,
        local: LocalId,
        ty: Type,
        types: &[AdtType],
    ) -> Result<Option<Cleanup>, String> {
        let tree = self.cleanup(ty, types)?;
        Ok((ty.is_owner() && self.materialized()).then_some(Cleanup { local, tree }))
    }
}
