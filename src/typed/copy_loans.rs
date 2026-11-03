//! Call-scoped loans of Copy roots and materialized read temporaries.
use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug)]
pub(super) struct CopySlot {
    root: Rc<RefCell<crate::interpreter::Value>>,
    path: Vec<FieldStep>,
}
pub(super) fn slot(value: crate::interpreter::Value) -> CopySlot {
    CopySlot {
        root: Rc::new(RefCell::new(value)),
        path: Vec::new(),
    }
}
impl CopySlot {
    fn project(mut self, path: &[FieldStep]) -> Self {
        // Each operand is separately checked and bounded. Forwarding composes
        // paths without holding a Rust borrow or imposing a new source limit.
        self.path.extend_from_slice(path);
        self
    }
    fn read(&self) -> Result<crate::interpreter::Value, String> {
        let value = self
            .root
            .try_borrow()
            .map_err(|_| "E-TYPED: conflicting runtime Copy loan")?;
        Ok(records::field_value(&value, &self.path)?.clone())
    }
    fn write(&self, value: crate::interpreter::Value) -> Result<(), String> {
        let mut root = self
            .root
            .try_borrow_mut()
            .map_err(|_| "E-TYPED: conflicting runtime Copy loan")?;
        *records::field_value_mut(&mut root, &self.path)? = value;
        Ok(())
    }
}
/// Cells and composed paths are internal addresses. Ordinary reads and new
/// bindings copy values; only explicit call loans retain these handles.
#[derive(Default)]
pub(super) struct CopyLocals {
    slots: HashMap<LocalId, CopySlot>,
}
impl CopyLocals {
    pub fn insert(&mut self, id: LocalId, value: crate::interpreter::Value) {
        self.slots.insert(id, slot(value));
    }
    pub fn bind_loan(&mut self, id: LocalId, value: CopySlot) {
        self.slots.insert(id, value);
    }
    pub fn extend(
        &mut self,
        values: impl IntoIterator<Item = (LocalId, crate::interpreter::Value)>,
    ) {
        for (id, value) in values {
            self.insert(id, value);
        }
    }
    pub fn remove(&mut self, id: &LocalId) {
        self.slots.remove(id);
    }
    pub fn loan(&self, id: &LocalId) -> Result<CopySlot, String> {
        self.slots
            .get(id)
            .cloned()
            .ok_or_else(|| "E-TYPED: missing Copy root".into())
    }
    pub fn loan_field(&self, place: &FieldPlace) -> Result<CopySlot, String> {
        Ok(self.loan(&place.root)?.project(&place.path))
    }
    pub fn read(&self, id: &LocalId) -> Result<crate::interpreter::Value, String> {
        self.loan(id)?.read()
    }
    pub fn write(&mut self, id: &LocalId, value: crate::interpreter::Value) -> Result<(), String> {
        self.loan(id)?.write(value)
    }
    pub fn read_field(&self, place: &FieldPlace) -> Result<crate::interpreter::Value, String> {
        self.loan_field(place)?.read()
    }
    pub fn write_field(
        &mut self,
        place: &FieldPlace,
        value: crate::interpreter::Value,
    ) -> Result<(), String> {
        // Called only after evaluating the RHS, which may replace a prefix.
        self.loan_field(place)?.write(value)
    }
    #[cfg(test)]
    pub fn strong_count(&self, id: LocalId) -> usize {
        Rc::strong_count(&self.slots[&id].root)
    }
    #[cfg(test)]
    pub fn snapshot(&self) -> HashMap<LocalId, crate::interpreter::Value> {
        self.slots
            .iter()
            .map(|(id, slot)| (*id, slot.read().unwrap()))
            .collect()
    }
}

impl Lower<'_, '_> {
    pub(super) fn copy_argument(
        &mut self,
        arg: &Expr,
        ty: Type,
        mode: ParamMode,
    ) -> Result<CallArg, String> {
        // Generalized immutable values have frozen captures, not one concrete
        // addressable root. Read arguments materialize this exact instance;
        // the source checker has already enforced the source root's interval.
        if mode == ParamMode::Read && self.local_shape(arg).is_some() {
            return Ok(CallArg::ReadTemporary(self.expr(arg)?));
        }
        let source = match (mode, arg) {
            (ParamMode::Value, _) => return Ok(CallArg::Value(self.expr(arg)?)),
            (ParamMode::Read, Expr::Field(root, _)) => {
                return self.field_argument(arg, root, ty, mode)
            }
            (ParamMode::Inout, Expr::InoutField(root, _)) => {
                return self.field_argument(arg, root, ty, mode)
            }
            (ParamMode::Read, Expr::Var(name)) if self.locals.contains_key(name) => name,
            (ParamMode::Read, _) => return Ok(CallArg::ReadTemporary(self.expr(arg)?)),
            (ParamMode::Inout, Expr::Inout(name)) => name,
            (ParamMode::Inout, _) => {
                return Err("E-TYPED-UNSUPPORTED: Copy inout requires a root".into())
            }
        };
        let local = self
            .locals
            .get_mut(source)
            .ok_or("E-TYPED-LOAN: unknown Copy root")?;
        if local.ty != ty
            || ty.is_owner()
            || !local.initialization.fully_initialized()
            || field_loans::conflicts(local, &[], false)
        {
            return Err("E-TYPED-LOAN: invalid Copy borrow root".into());
        }
        let result = match mode {
            ParamMode::Read => {
                local.shared = local
                    .shared
                    .checked_add(1)
                    .ok_or("E-TYPED-LOAN: too many shared loans")?;
                CallArg::ReadCopy(local.id)
            }
            ParamMode::Inout => {
                if !local.mutable || local.shared != 0 || !local.field_loans.is_empty() {
                    return Err("E-TYPED-LOAN: overlapping or immutable Copy loan".into());
                }
                local.loaned = true;
                CallArg::InoutCopy(local.id)
            }
            ParamMode::Value => unreachable!(),
        };
        Ok(result)
    }
}

/// Recheck operand identity, mode and interval independently of lowering and
/// runtime cell aliasing. Calls restore the enclosing state after all arguments.
pub(super) fn verify_root(
    id: LocalId,
    ty: Type,
    mode: ParamMode,
    scope: &mut HashMap<LocalId, Local>,
) -> Result<(), String> {
    let local = scope
        .get_mut(&id)
        .ok_or("E-TYPED-LOAN: Copy root outside scope")?;
    if ty.is_owner()
        || local.ty != ty
        || !local.initialization.fully_initialized()
        || field_loans::conflicts(local, &[], false)
    {
        return Err("E-TYPED-LOAN: invalid Copy root loan".into());
    }
    match mode {
        ParamMode::Read => {
            local.shared = local
                .shared
                .checked_add(1)
                .ok_or("E-TYPED-LOAN: too many shared loans")?
        }
        ParamMode::Inout if local.mutable && local.shared == 0 && local.field_loans.is_empty() => {
            local.loaned = true
        }
        _ => return Err("E-TYPED-LOAN: conflicting Copy loan or mode".into()),
    }
    Ok(())
}
