//! Independently checked call-scoped intervals for Copy record projections.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectionLoan {
    pub path: Vec<FieldStep>,
    pub exclusive: bool,
}

// Empty paths denote whole-root access. Validated nominal steps prevent field
// indices from another record being mistaken for a disjoint place.
pub(super) fn conflicts(local: &Local, path: &[FieldStep], write: bool) -> bool {
    local.loaned
        || (write && local.shared != 0)
        || local
            .field_loans
            .iter()
            .any(|loan| (write || loan.exclusive) && places::paths_overlap(&loan.path, path))
}

impl Lower<'_, '_> {
    pub(super) fn field_argument(
        &mut self,
        expr: &Expr,
        root: &str,
        ty: Type,
        mode: ParamMode,
    ) -> Result<CallArg, String> {
        let exclusive = mode == ParamMode::Inout;
        if !matches!(mode, ParamMode::Read | ParamMode::Inout) || ty.is_owner() {
            return Err("E-TYPED-LOAN: invalid field argument mode or type".into());
        }
        let place = self.field_place(expr, root, exclusive)?;
        let local = self
            .locals
            .get_mut(root)
            .ok_or("E-TYPED-LOAN: unknown field root")?;
        if records::projected_type(local.ty, &place.path, &self.registry.types)? != ty {
            return Err("E-TYPED-LOAN: incorrect projected argument type".into());
        }
        local.field_loans.push(ProjectionLoan {
            path: place.path.clone(),
            exclusive,
        });
        Ok(if exclusive {
            CallArg::InoutField(place)
        } else {
            CallArg::ReadField(place)
        })
    }
}

pub(super) fn verify_field(
    place: &FieldPlace,
    ty: Type,
    mode: ParamMode,
    scope: &mut HashMap<LocalId, Local>,
    adts: &[AdtType],
) -> Result<(), String> {
    let exclusive = mode == ParamMode::Inout;
    if !matches!(mode, ParamMode::Read | ParamMode::Inout) || ty.is_owner() {
        return Err("E-TYPED-LOAN: invalid field argument mode or type".into());
    }
    if records::verify_place(place, scope, adts, exclusive)? != ty {
        return Err("E-TYPED-LOAN: incorrect projected argument type".into());
    }
    scope
        .get_mut(&place.root)
        .ok_or("E-TYPED-LOAN: field root outside scope")?
        .field_loans
        .push(ProjectionLoan {
            path: place.path.clone(),
            exclusive,
        });
    Ok(())
}
