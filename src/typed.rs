//! A provisional typed IR for closed monomorphic scalar, static-string and
//! fixed byte-buffer programs.
//!
//! This is not the verified MIR in PLAN.md. Lowering requires an existing
//! checked program. The C backend uses it for supported forms; owned-box
//! programs also execute through this IR in the interpreter.
//! Programs outside this narrow subset are rejected without approximation.
mod adt_eval;
mod adt_lower;
#[cfg(test)]
mod adt_tests;
mod adt_verify;
mod adts;
mod allocation;
use allocation::AllocationFailure;
pub use allocation::{AllocationContract, AllocationResultLayout, SystemAllocationKind};
mod c;
pub use c::emit_c;

use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, Expr, ParamMode};
use crate::check::{CheckedProgram, Type as InferredType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Bool,
    Unit,
    StaticStr,
    Bytes(usize),
    OwnedI32,
    OwnedBytes,
    Adt { id: usize, owned: bool },
}

impl Type {
    fn is_owner(self) -> bool {
        matches!(
            self,
            Self::OwnedI32 | Self::OwnedBytes | Self::Adt { owned: true, .. }
        )
    }
}

#[derive(Clone, Debug)]
pub struct AdtType {
    pub name: String,
    pub arguments: Vec<Type>,
    pub variants: Vec<AdtVariant>,
    pub owned: bool,
}
#[derive(Clone, Debug)]
pub struct AdtVariant {
    pub name: String,
    pub fields: Vec<Type>,
}
#[derive(Clone, Debug)]
pub struct AdtArm {
    pub variant: Option<usize>,
    pub fields: Vec<Option<LocalId>>,
    pub body: ExprNode,
    pub cleanup: Vec<LocalId>,
}

#[derive(Clone, Debug)]
pub struct ExprNode {
    pub ty: Type,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    TryAlloc {
        kind: SystemAllocationKind,
        input: Box<ExprNode>,
        layout: AllocationResultLayout,
    },
    Int(i32),
    Str(String),
    Bool(bool),
    Unit,
    Read(LocalId),
    Call(usize, Vec<CallArg>),
    Constructor {
        adt: usize,
        variant: usize,
        fields: Vec<ExprNode>,
    },
    Match {
        value: Box<ExprNode>,
        arms: Vec<AdtArm>,
    },
    ByteLen(Box<ExprNode>),
    ByteAt(Box<ExprNode>, Box<ExprNode>),
    BytesNew(usize),
    BytesLen(Box<ExprNode>),
    BytesGet(Box<ExprNode>, Box<ExprNode>),
    BytesSet(Box<ExprNode>, Box<ExprNode>, Box<ExprNode>),
    OwnerLet {
        local: LocalId,
        mutable: bool,
        init: OwnerInit,
        body: Box<ExprNode>,
        cleanup: Option<LocalId>,
    },
    OwnerBytesLen(LocalId),
    OwnerBytesGet(LocalId, Box<ExprNode>),
    OwnerBytesSet(LocalId, Box<ExprNode>, Box<ExprNode>),
    OwnerRead(LocalId),
    OwnerMove(LocalId),
    OwnerWrite(LocalId, Box<ExprNode>),
    Let {
        local: LocalId,
        mutable: bool,
        value: Box<ExprNode>,
        body: Box<ExprNode>,
    },
    Assign(LocalId, Box<ExprNode>),
    Binary(BinaryOp, Box<ExprNode>, Box<ExprNode>),
    If(Box<ExprNode>, Box<ExprNode>, Box<ExprNode>),
    While(Box<ExprNode>, Box<ExprNode>),
}

#[derive(Clone, Debug)]
pub enum CallArg {
    Value(ExprNode),
    Move(LocalId),
    ReadOwner(LocalId),
    InoutOwner(LocalId),
}

#[derive(Clone, Debug)]
pub enum OwnerInit {
    Alloc(Box<ExprNode>),
    BytesAlloc(Box<ExprNode>),
    Move(LocalId),
    Call(Box<ExprNode>),
    Expr(Box<ExprNode>),
}

#[derive(Clone, Debug)]
pub struct TypedMain {
    allocation_contract: Option<AllocationContract>,
    adts: Vec<AdtType>,
    functions: Vec<TypedFunction>,
    entry: usize,
}

#[derive(Clone, Debug)]
struct TypedFunction {
    params: Vec<(LocalId, Type, ParamMode)>,
    result: Type,
    body: ExprNode,
    param_cleanup: Vec<LocalId>,
}

fn primitive_type(ty: &InferredType) -> Result<Type, String> {
    match ty {
        InferredType::Int => Ok(Type::Int),
        InferredType::Bool => Ok(Type::Bool),
        InferredType::Unit => Ok(Type::Unit),
        InferredType::StaticStr => Ok(Type::StaticStr),
        InferredType::Bytes(n) if *n <= 256 => Ok(Type::Bytes(*n)),
        InferredType::SystemBoxI32 => Ok(Type::OwnedI32),
        InferredType::SystemBytes => Ok(Type::OwnedBytes),
        _ => Err("E-TYPED-UNSUPPORTED: nonprimitive or unresolved type".into()),
    }
}

#[derive(Clone, Copy)]
struct Local {
    id: LocalId,
    ty: Type,
    mutable: bool,
    available: bool,
    loaned: bool,
    shared: usize,
    mode: ParamMode,
}

/// Lower a checked program in the supported scalar, string and buffer subset.
/// The C backend uses this result for supported programs; other checked
/// programs use the existing AST backend.
pub fn lower_main(checked: &CheckedProgram<'_>) -> Result<TypedMain, String> {
    let program = checked.program;
    if (!program.types.is_empty() && !checked.has_owned())
        || !program.externs.is_empty()
        || !program.constants.is_empty()
        || checked.has_c_values()
        || !program.native.includes.is_empty()
    {
        return Err("E-TYPED-UNSUPPORTED: types and foreign functions".into());
    }
    let functions: HashMap<_, _> = program
        .functions
        .iter()
        .enumerate()
        .map(|(id, f)| (f.name.clone(), id))
        .collect();
    let entry = *functions.get("main").ok_or("E-TYPED: missing main")?;
    let registry = adts::Registry::build(checked)?;
    let mut lower = Lower {
        registry,
        checked,
        locals: HashMap::new(),
        functions,
        function_modes: program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.params.iter().map(|p| p.mode).collect()))
            .collect(),
        next: 0,
    };
    let mut result = TypedMain {
        allocation_contract: program
            .allocation_types
            .as_ref()
            .map(AllocationContract::from),
        adts: Vec::new(),
        functions: Vec::new(),
        entry,
    };
    for function in &program.functions {
        let signature = checked
            .functions
            .get(&function.name)
            .ok_or("E-TYPED: missing function signature")?;
        if !signature.quantified.is_empty() {
            return Err("E-TYPED-UNSUPPORTED: generic functions".into());
        }
        let result_type = lower.registry.convert(&signature.result)?;
        let param_types = signature
            .params
            .iter()
            .map(|ty| lower.registry.convert(ty))
            .collect::<Result<Vec<_>, _>>()?;
        if param_types.len() != function.params.len() {
            return Err("E-TYPED: invalid function signature".into());
        }
        if function
            .params
            .iter()
            .zip(&param_types)
            .any(|(param, ty)| param.mode != ParamMode::Value && !ty.is_owner())
        {
            return Err("E-TYPED-UNSUPPORTED: borrowed non-owner parameters".into());
        }
        lower.locals.clear();
        let mut params = Vec::new();
        for (param, ty) in function.params.iter().zip(param_types) {
            let id = lower.fresh()?;
            lower.locals.insert(
                param.name.clone(),
                Local {
                    id,
                    ty,
                    mutable: param.mode == ParamMode::Inout,
                    available: true,
                    loaned: false,
                    shared: 0,
                    mode: param.mode,
                },
            );
            params.push((id, ty, param.mode));
        }
        let body = lower.expr(&function.body)?;
        let param_cleanup = function
            .params
            .iter()
            .zip(&params)
            .rev()
            .filter_map(|(param, (id, ty, mode))| {
                (ty.is_owner()
                    && *mode == ParamMode::Value
                    && lower
                        .locals
                        .get(&param.name)
                        .is_some_and(|local| local.id == *id && local.available))
                .then_some(*id)
            })
            .collect();
        result.functions.push(TypedFunction {
            params,
            result: result_type,
            body,
            param_cleanup,
        });
    }
    result.adts = lower.registry.types;
    verify(&result)?;
    Ok(result)
}

struct Lower<'a, 'b> {
    registry: adts::Registry,
    checked: &'a CheckedProgram<'b>,
    locals: HashMap<String, Local>,
    functions: HashMap<String, usize>,
    function_modes: HashMap<String, Vec<ParamMode>>,
    next: u32,
}
impl Lower<'_, '_> {
    fn fresh(&mut self) -> Result<LocalId, String> {
        let id = LocalId(self.next);
        self.next = self.next.checked_add(1).ok_or("E-TYPED: too many locals")?;
        Ok(id)
    }
    fn expr(&mut self, expr: &Expr) -> Result<ExprNode, String> {
        let ty = self.registry.convert(
            self.checked
                .expression_type(expr)
                .ok_or("E-TYPED: missing checked expression type")?,
        )?;
        let kind = match expr {
            Expr::Call(name, args)
                if matches!(name.as_str(), "system_box_i32" | "system_bytes_zeroed")
                    && args.len() == 1 =>
            {
                let kind = if name == "system_box_i32" {
                    SystemAllocationKind::BoxI32
                } else {
                    SystemAllocationKind::BytesZeroed
                };
                let contract = self
                    .checked
                    .program
                    .allocation_types
                    .as_ref()
                    .map(AllocationContract::from)
                    .ok_or("E-TYPED-ALLOC: missing canonical allocation contract")?;
                let layout = allocation::resolve(ty, &self.registry.types, &contract)?;
                ExprKind::TryAlloc {
                    kind,
                    input: Box::new(self.expr(&args[0])?),
                    layout,
                }
            }
            Expr::Call(name, args) if matches!(ty,Type::Adt{id,..} if self.registry.types[id].variants.iter().any(|v|v.name==*name)) => {
                self.constructor(ty, name, args)?
            }
            Expr::Var(name)
                if !self.locals.contains_key(name) && matches!(ty, Type::Adt { .. }) =>
            {
                self.constructor(ty, name, &[])?
            }
            Expr::Match(value, arms)
                if self.checked.expression_type(value).is_some_and(|ty| {
                    matches!(self.registry.convert(ty), Ok(Type::Adt { .. }))
                }) =>
            {
                self.match_adt(value, arms)?
            }
            Expr::Int(n) => ExprKind::Int(*n),
            Expr::Str(value) => ExprKind::Str(value.clone()),
            Expr::Bool(b) => ExprKind::Bool(*b),
            Expr::Unit => ExprKind::Unit,
            Expr::Move(name) => {
                let local = self
                    .locals
                    .get_mut(name)
                    .ok_or("E-TYPED: unknown move source")?;
                if !local.ty.is_owner()
                    || local.mode != ParamMode::Value
                    || !local.available
                    || local.loaned
                    || local.shared != 0
                {
                    return Err("E-TYPED-OWNER: invalid returned move".into());
                }
                local.available = false;
                ExprKind::OwnerMove(local.id)
            }
            Expr::Var(name) => {
                let local = self.locals.get(name).ok_or("E-TYPED: missing local")?;
                if local.ty.is_owner() || !local.available || local.loaned || local.shared != 0 {
                    return Err("E-TYPED: cannot copy an owner".into());
                }
                ExprKind::Read(local.id)
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                let owner_type = self
                    .checked
                    .expression_type(value)
                    .map(|ty| self.registry.convert(ty))
                    .transpose()?;
                let is_owner = owner_type.is_some_and(Type::is_owner);
                if is_owner {
                    let init = match &**value {
                        Expr::Call(function, args)
                            if function == "system_box_i32_abort" && args.len() == 1 =>
                        {
                            OwnerInit::Alloc(Box::new(self.expr(&args[0])?))
                        }
                        Expr::Call(function, args)
                            if function == "system_bytes_zeroed_abort" && args.len() == 1 =>
                        {
                            OwnerInit::BytesAlloc(Box::new(self.expr(&args[0])?))
                        }
                        Expr::Move(source) => {
                            let local = self
                                .locals
                                .get_mut(source)
                                .ok_or("E-TYPED: unknown owner")?;
                            if Some(local.ty) != owner_type
                                || local.mode != ParamMode::Value
                                || !local.available
                                || local.loaned
                                || local.shared != 0
                            {
                                return Err("E-TYPED: invalid move".into());
                            }
                            local.available = false;
                            OwnerInit::Move(local.id)
                        }
                        Expr::Call(name, ..) if self.functions.contains_key(name) => {
                            OwnerInit::Call(Box::new(self.expr(value)?))
                        }
                        Expr::Call(..)
                        | Expr::Var(_)
                        | Expr::If(..)
                        | Expr::Match(..)
                        | Expr::Let(..)
                        | Expr::LetMut(..) => OwnerInit::Expr(Box::new(self.expr(value)?)),
                        _ => return Err("E-TYPED-UNSUPPORTED: owner initializer".into()),
                    };
                    let id = self.fresh()?;
                    let mutable = matches!(expr, Expr::LetMut(..));
                    let previous = self.locals.insert(
                        name.clone(),
                        Local {
                            id,
                            ty: owner_type.expect("owner type"),
                            mutable,
                            available: true,
                            loaned: false,
                            shared: 0,
                            mode: ParamMode::Value,
                        },
                    );
                    let body = self.expr(body);
                    let cleanup = self
                        .locals
                        .get(name)
                        .filter(|local| local.id == id && local.available)
                        .map(|_| id);
                    if let Some(previous) = previous {
                        self.locals.insert(name.clone(), previous);
                    } else {
                        self.locals.remove(name);
                    }
                    ExprKind::OwnerLet {
                        local: id,
                        mutable,
                        init,
                        body: Box::new(body?),
                        cleanup,
                    }
                } else {
                    let value = self.expr(value)?;
                    let id = self.fresh()?;
                    let mutable = matches!(expr, Expr::LetMut(..));
                    let previous = self.locals.insert(
                        name.clone(),
                        Local {
                            id,
                            ty: value.ty,
                            mutable,
                            available: true,
                            loaned: false,
                            shared: 0,
                            mode: ParamMode::Value,
                        },
                    );
                    let body = self.expr(body);
                    if let Some(previous) = previous {
                        self.locals.insert(name.clone(), previous);
                    } else {
                        self.locals.remove(name);
                    }
                    ExprKind::Let {
                        local: id,
                        mutable,
                        value: Box::new(value),
                        body: Box::new(body?),
                    }
                }
            }
            Expr::Assign(name, value) => {
                let local = *self
                    .locals
                    .get(name)
                    .ok_or("E-TYPED: missing assignment local")?;
                if !local.mutable
                    || local.ty.is_owner()
                    || !local.available
                    || local.loaned
                    || local.shared != 0
                {
                    return Err("E-TYPED: invalid assignment local".into());
                }
                ExprKind::Assign(local.id, Box::new(self.expr(value)?))
            }
            Expr::Call(name, args) if name == "box_read" && args.len() == 1 => {
                let Expr::Var(root) = &args[0] else {
                    return Err("E-TYPED: owner read requires a root".into());
                };
                let local = self.locals.get(root).ok_or("E-TYPED: unknown owner")?;
                if local.ty != Type::OwnedI32 || !local.available || local.loaned {
                    return Err("E-TYPED: invalid owner read".into());
                }
                ExprKind::OwnerRead(local.id)
            }
            Expr::Call(name, args) if name == "box_write" && args.len() == 2 => {
                let Expr::Inout(root) = &args[0] else {
                    return Err("E-TYPED: owner write requires inout root".into());
                };
                let local = *self.locals.get(root).ok_or("E-TYPED: unknown owner")?;
                if local.ty != Type::OwnedI32
                    || !local.available
                    || !local.mutable
                    || local.loaned
                    || local.shared != 0
                {
                    return Err("E-TYPED: invalid owner write".into());
                }
                self.locals.get_mut(root).unwrap().loaned = true;
                let value = self.expr(&args[1]);
                self.locals.get_mut(root).unwrap().loaned = false;
                ExprKind::OwnerWrite(local.id, Box::new(value?))
            }
            Expr::Call(name, args)
                if matches!(
                    name.as_str(),
                    "owned_bytes_len" | "owned_bytes_get" | "owned_bytes_set"
                ) =>
            {
                let expected = match name.as_str() {
                    "owned_bytes_len" => 1,
                    "owned_bytes_get" => 2,
                    _ => 3,
                };
                if args.len() != expected {
                    return Err("E-TYPED-OWNER: invalid bytes operation arity".into());
                }
                let exclusive = name == "owned_bytes_set";
                let root = match (&args[0], exclusive) {
                    (Expr::Var(root), false) | (Expr::Inout(root), true) => root,
                    _ => return Err("E-TYPED-OWNER: invalid bytes operation root".into()),
                };
                let local = *self.locals.get(root).ok_or("E-TYPED-OWNER: unknown root")?;
                if local.ty != Type::OwnedBytes
                    || !local.available
                    || local.loaned
                    || (exclusive && (!local.mutable || local.shared != 0))
                {
                    return Err("E-TYPED-OWNER: invalid bytes loan".into());
                }
                if exclusive {
                    self.locals.get_mut(root).unwrap().loaned = true;
                } else {
                    self.locals.get_mut(root).unwrap().shared += 1;
                }
                let values = args[1..]
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>, _>>();
                if exclusive {
                    self.locals.get_mut(root).unwrap().loaned = false;
                } else {
                    self.locals.get_mut(root).unwrap().shared -= 1;
                }
                let mut values = values?.into_iter();
                match name.as_str() {
                    "owned_bytes_len" => ExprKind::OwnerBytesLen(local.id),
                    "owned_bytes_get" => {
                        ExprKind::OwnerBytesGet(local.id, Box::new(values.next().unwrap()))
                    }
                    _ => ExprKind::OwnerBytesSet(
                        local.id,
                        Box::new(values.next().unwrap()),
                        Box::new(values.next().unwrap()),
                    ),
                }
            }
            Expr::Call(name, args) if name == "bytes_new" && args.len() == 1 => {
                let Expr::Int(n) = args[0] else {
                    return Err("E-TYPED: invalid bytes_new capacity".into());
                };
                let n = usize::try_from(n).map_err(|_| "E-TYPED: invalid bytes_new capacity")?;
                if n > 256 {
                    return Err("E-TYPED: invalid bytes_new capacity".into());
                }
                ExprKind::BytesNew(n)
            }
            Expr::Call(name, args) if name == "bytes_len" && args.len() == 1 => {
                ExprKind::BytesLen(Box::new(self.expr(&args[0])?))
            }
            Expr::Call(name, args) if name == "bytes_get" && args.len() == 2 => ExprKind::BytesGet(
                Box::new(self.expr(&args[0])?),
                Box::new(self.expr(&args[1])?),
            ),
            Expr::Call(name, args) if name == "bytes_set" && args.len() == 3 => ExprKind::BytesSet(
                Box::new(self.expr(&args[0])?),
                Box::new(self.expr(&args[1])?),
                Box::new(self.expr(&args[2])?),
            ),
            Expr::Call(name, args) if name == "byte_len" && args.len() == 1 => {
                ExprKind::ByteLen(Box::new(self.expr(&args[0])?))
            }
            Expr::Call(name, args) if name == "byte_at" && args.len() == 2 => ExprKind::ByteAt(
                Box::new(self.expr(&args[0])?),
                Box::new(self.expr(&args[1])?),
            ),
            Expr::Call(name, args) => {
                let id = *self
                    .functions
                    .get(name)
                    .ok_or("E-TYPED-UNSUPPORTED: builtin or unknown function")?;
                let signature = self
                    .checked
                    .functions
                    .get(name)
                    .ok_or("E-TYPED: missing function signature")?
                    .clone();
                if args.len() != signature.params.len() {
                    return Err("E-TYPED: wrong call arity".into());
                }
                let modes = self
                    .function_modes
                    .get(name)
                    .ok_or("E-TYPED: missing function modes")?
                    .clone();
                if modes.len() != args.len() {
                    return Err("E-TYPED: invalid function modes".into());
                }
                let mut lowered = Vec::new();
                let mut borrowed = Vec::new();
                for ((arg, ty), mode) in args.iter().zip(&signature.params).zip(modes) {
                    if self.registry.convert(ty)?.is_owner() {
                        let source = match (mode, arg) {
                            (ParamMode::Value, Expr::Move(source))
                            | (ParamMode::Read, Expr::Var(source))
                            | (ParamMode::Inout, Expr::Inout(source)) => source,
                            _ => return Err("E-TYPED-OWNER: wrong owner argument mode".into()),
                        };
                        let local = self
                            .locals
                            .get_mut(source)
                            .ok_or("E-TYPED-OWNER: unknown owner root")?;
                        if local.ty != self.registry.convert(ty)? || !local.available {
                            return Err("E-TYPED-OWNER: invalid owner root".into());
                        }
                        match mode {
                            ParamMode::Value => {
                                if local.mode != ParamMode::Value
                                    || local.loaned
                                    || local.shared != 0
                                {
                                    return Err("E-TYPED-OWNER: invalid move".into());
                                }
                                local.available = false;
                                lowered.push(CallArg::Move(local.id));
                            }
                            ParamMode::Read => {
                                if local.loaned {
                                    return Err("E-TYPED-OWNER: overlapping loan".into());
                                }
                                local.shared = local
                                    .shared
                                    .checked_add(1)
                                    .ok_or("E-TYPED-OWNER: too many shared loans")?;
                                borrowed.push((source.clone(), mode));
                                lowered.push(CallArg::ReadOwner(local.id));
                            }
                            ParamMode::Inout => {
                                if !local.mutable || local.loaned || local.shared != 0 {
                                    return Err("E-TYPED-OWNER: overlapping loan".into());
                                }
                                local.loaned = true;
                                borrowed.push((source.clone(), mode));
                                lowered.push(CallArg::InoutOwner(local.id));
                            }
                        }
                    } else {
                        if mode != ParamMode::Value {
                            return Err("E-TYPED-UNSUPPORTED: borrowed non-owner parameter".into());
                        }
                        lowered.push(CallArg::Value(self.expr(arg)?));
                    }
                }
                for (source, mode) in borrowed.into_iter().rev() {
                    let local = self
                        .locals
                        .get_mut(&source)
                        .ok_or("E-TYPED-OWNER: lost loan root")?;
                    match mode {
                        ParamMode::Read => local.shared -= 1,
                        ParamMode::Inout => local.loaned = false,
                        ParamMode::Value => unreachable!(),
                    }
                }
                ExprKind::Call(id, lowered)
            }
            Expr::Binary(op, a, b) => {
                ExprKind::Binary(*op, Box::new(self.expr(a)?), Box::new(self.expr(b)?))
            }
            Expr::If(c, a, b) => {
                let condition = self.expr(c)?;
                let before = self.locals.clone();
                let yes = self.expr(a)?;
                let after_yes = self.locals.clone();
                self.locals = before;
                let no = self.expr(b)?;
                if !Self::same_states(&after_yes, &self.locals) {
                    return Err(
                        "E-TYPED-OWNER: move in conditional has incompatible branch ownership"
                            .into(),
                    );
                }
                self.locals = after_yes;
                ExprKind::If(Box::new(condition), Box::new(yes), Box::new(no))
            }
            Expr::While(c, b) => ExprKind::While(Box::new(self.expr(c)?), Box::new(self.expr(b)?)),
            _ => return Err("E-TYPED-UNSUPPORTED: expression is outside primitive subset".into()),
        };
        Ok(ExprNode { ty, kind })
    }
}

/// Check the structural invariants of this provisional IR, including scope,
/// local identity, mutability, and operation types. This is not an ownership
/// or effect proof and must not be used to bypass the source checker.
pub fn verify(program: &TypedMain) -> Result<(), String> {
    adts::verify(&program.adts)?;
    for function in &program.functions {
        let error = std::cell::RefCell::new(None);
        any_expr(&function.body, &|kind| {
            if let ExprKind::TryAlloc { kind, layout, .. } = kind {
                if let Err(e) = allocation::validate(
                    *kind,
                    layout,
                    &program.adts,
                    program.allocation_contract.as_ref(),
                ) {
                    *error.borrow_mut() = Some(e);
                }
            }
            false
        });
        if let Some(error) = error.into_inner() {
            return Err(error);
        }
    }
    let entry = program
        .functions
        .get(program.entry)
        .ok_or("E-TYPED: invalid entry")?;
    if entry.result.is_owner() {
        return Err("E-TYPED-OWNER: main cannot return an owner".into());
    }
    if !entry.params.is_empty() {
        return Err("E-TYPED: main has parameters".into());
    }
    let has_owner = program.functions.iter().any(|f| {
        f.result.is_owner()
            || f.params.iter().any(|(_, ty, _)| ty.is_owner())
            || any_expr(&f.body, &|kind| {
                matches!(
                    kind,
                    ExprKind::OwnerLet { .. } | ExprKind::OwnerRead(_) | ExprKind::OwnerWrite(..)
                )
            })
    });
    if has_owner {
        verify_acyclic_calls(program)?;
    }
    let mut ids = HashSet::new();
    for function in &program.functions {
        adts::validate_type(function.result, &program.adts)?;
        if matches!(function.result, Type::Bytes(n) if n > 256) {
            return Err("E-TYPED: invalid buffer capacity".into());
        }
        let mut scope = HashMap::new();
        for &(id, ty, mode) in &function.params {
            adts::validate_type(ty, &program.adts)?;
            if !ty.is_owner() && mode != ParamMode::Value {
                return Err("E-TYPED-UNSUPPORTED: borrowed non-owner parameter".into());
            }
            if matches!(ty, Type::Bytes(n) if n > 256) {
                return Err("E-TYPED: invalid buffer capacity".into());
            }
            if !ids.insert(id) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                id,
                Local {
                    id,
                    ty,
                    mutable: mode == ParamMode::Inout,
                    available: true,
                    loaned: false,
                    shared: 0,
                    mode,
                },
            );
        }
        if verify_expr(
            &function.body,
            &mut scope,
            &mut ids,
            &program.functions,
            &program.adts,
        )? != function.result
        {
            return Err("E-TYPED: wrong function result type".into());
        }
        let expected: Vec<_> = function
            .params
            .iter()
            .rev()
            .filter_map(|(id, ty, mode)| {
                (ty.is_owner()
                    && *mode == ParamMode::Value
                    && scope.get(id).is_some_and(|local| local.available))
                .then_some(*id)
            })
            .collect();
        if function.param_cleanup != expected {
            return Err("E-TYPED-OWNER: invalid parameter cleanup".into());
        }
    }
    Ok(())
}

fn verify_expr(
    expr: &ExprNode,
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    functions: &[TypedFunction],
    adts: &[AdtType],
) -> Result<Type, String> {
    if expr.ty.is_owner()
        && !matches!(
            expr.kind,
            ExprKind::TryAlloc { .. }
                | ExprKind::OwnerMove(_)
                | ExprKind::Call(..)
                | ExprKind::Constructor { .. }
                | ExprKind::Match { .. }
                | ExprKind::If(..)
                | ExprKind::OwnerLet { .. }
                | ExprKind::Let { .. }
        )
    {
        return Err("E-TYPED-OWNER: unsupported owner result expression".into());
    }
    adts::validate_type(expr.ty, adts)?;
    use BinaryOp::*;
    if matches!(expr.ty, Type::Bytes(n) if n > 256) {
        return Err("E-TYPED: invalid buffer capacity".into());
    }
    let ty = match &expr.kind {
        ExprKind::TryAlloc { input, layout, .. } => {
            if verify_expr(input, scope, ids, functions, adts)? != Type::Int {
                return Err("E-TYPED-ALLOC: input must be i32".into());
            }
            Type::Adt {
                id: layout.result_adt,
                owned: true,
            }
        }
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => adt_verify::constructor(*adt, *variant, fields, scope, ids, functions, adts)?,
        ExprKind::Match { value, arms } => {
            adt_verify::match_value(value, arms, scope, ids, functions, adts)?
        }
        ExprKind::Int(_) => Type::Int,
        ExprKind::Str(_) => Type::StaticStr,
        ExprKind::Bool(_) => Type::Bool,
        ExprKind::Unit => Type::Unit,
        ExprKind::Read(id) => {
            let local = scope.get(id).ok_or("E-TYPED: read outside local scope")?;
            if local.ty.is_owner() || !local.available || local.loaned || local.shared != 0 {
                return Err("E-TYPED-OWNER: invalid ordinary read".into());
            }
            local.ty
        }
        ExprKind::OwnerMove(id) => {
            let source = scope
                .get_mut(id)
                .ok_or("E-TYPED-OWNER: unknown return move source")?;
            if !source.ty.is_owner()
                || source.mode != ParamMode::Value
                || !source.available
                || source.loaned
                || source.shared != 0
            {
                return Err("E-TYPED-OWNER: invalid return move".into());
            }
            source.available = false;
            source.ty
        }
        ExprKind::OwnerRead(id) => {
            let local = scope.get(id).ok_or("E-TYPED-OWNER: missing owner")?;
            if local.ty != Type::OwnedI32 || !local.available || local.loaned {
                return Err("E-TYPED-OWNER: invalid owner read".into());
            }
            Type::Int
        }
        ExprKind::OwnerWrite(id, value) => {
            let local = *scope.get(id).ok_or("E-TYPED-OWNER: missing owner")?;
            if local.ty != Type::OwnedI32
                || !local.available
                || !local.mutable
                || local.loaned
                || local.shared != 0
            {
                return Err("E-TYPED-OWNER: invalid owner write".into());
            }
            scope.get_mut(id).unwrap().loaned = true;
            let value_type = verify_expr(value, scope, ids, functions, adts);
            scope.get_mut(id).unwrap().loaned = false;
            if value_type? != Type::Int {
                return Err("E-TYPED-OWNER: invalid written value".into());
            }
            Type::Unit
        }
        ExprKind::OwnerBytesLen(id)
        | ExprKind::OwnerBytesGet(id, _)
        | ExprKind::OwnerBytesSet(id, _, _) => {
            let local = *scope.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            let exclusive = matches!(expr.kind, ExprKind::OwnerBytesSet(..));
            if local.ty != Type::OwnedBytes
                || !local.available
                || local.loaned
                || (exclusive && (!local.mutable || local.shared != 0))
            {
                return Err("E-TYPED-OWNER: invalid bytes loan".into());
            }
            if exclusive {
                scope.get_mut(id).unwrap().loaned = true;
            } else {
                let owner = scope.get_mut(id).unwrap();
                owner.shared = owner
                    .shared
                    .checked_add(1)
                    .ok_or("E-TYPED-OWNER: too many loans")?;
            }
            let check = match &expr.kind {
                ExprKind::OwnerBytesLen(_) => Ok(()),
                ExprKind::OwnerBytesGet(_, index) => {
                    if verify_expr(index, scope, ids, functions, adts)? == Type::Int {
                        Ok(())
                    } else {
                        Err("E-TYPED-OWNER: invalid bytes index")
                    }
                }
                ExprKind::OwnerBytesSet(_, index, value) => {
                    if verify_expr(index, scope, ids, functions, adts)? == Type::Int
                        && verify_expr(value, scope, ids, functions, adts)? == Type::Int
                    {
                        Ok(())
                    } else {
                        Err("E-TYPED-OWNER: invalid bytes operands")
                    }
                }
                _ => unreachable!(),
            };
            if exclusive {
                scope.get_mut(id).unwrap().loaned = false;
            } else {
                scope.get_mut(id).unwrap().shared -= 1;
            }
            check?;
            if exclusive {
                Type::Unit
            } else {
                Type::Int
            }
        }
        ExprKind::OwnerLet {
            local,
            mutable,
            init,
            body,
            cleanup,
        } => {
            let owner_ty = match init {
                OwnerInit::Expr(value) => {
                    let ty = verify_expr(value, scope, ids, functions, adts)?;
                    if !ty.is_owner() {
                        return Err("E-TYPED-OWNER: expected affine initializer".into());
                    }
                    ty
                }
                OwnerInit::Call(value) => {
                    if !matches!(value.kind, ExprKind::Call(..)) {
                        return Err("E-TYPED-OWNER: initializer must be a direct call".into());
                    }
                    let ty = verify_expr(value, scope, ids, functions, adts)?;
                    if !ty.is_owner() {
                        return Err("E-TYPED-OWNER: call must return an owner".into());
                    }
                    ty
                }
                OwnerInit::Alloc(value) | OwnerInit::BytesAlloc(value) => {
                    if verify_expr(value, scope, ids, functions, adts)? != Type::Int {
                        return Err("E-TYPED-OWNER: allocation requires i32".into());
                    }
                    if matches!(init, OwnerInit::Alloc(_)) {
                        Type::OwnedI32
                    } else {
                        Type::OwnedBytes
                    }
                }
                OwnerInit::Move(source) => {
                    let source = scope
                        .get_mut(source)
                        .ok_or("E-TYPED-OWNER: unknown move source")?;
                    if !source.ty.is_owner()
                        || source.mode != ParamMode::Value
                        || !source.available
                        || source.loaned
                        || source.shared != 0
                    {
                        return Err("E-TYPED-OWNER: invalid move".into());
                    }
                    source.available = false;
                    source.ty
                }
            };
            if !ids.insert(*local) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                *local,
                Local {
                    id: *local,
                    ty: owner_ty,
                    mutable: *mutable,
                    available: true,
                    loaned: false,
                    shared: 0,
                    mode: ParamMode::Value,
                },
            );
            let result = verify_expr(body, scope, ids, functions, adts)?;
            let live = scope
                .get(local)
                .ok_or("E-TYPED-OWNER: missing owner")?
                .available;
            if *cleanup != live.then_some(*local) {
                return Err("E-TYPED-OWNER: invalid cleanup".into());
            }
            scope.remove(local);
            result
        }
        ExprKind::Call(id, args) => {
            let function = functions
                .get(*id)
                .ok_or("E-TYPED: invalid function identity")?;
            if args.len() != function.params.len() {
                return Err("E-TYPED: wrong call arity".into());
            }
            let loans: Vec<_> = scope
                .iter()
                .map(|(id, local)| (*id, local.shared, local.loaned))
                .collect();
            for (arg, (_, ty, mode)) in args.iter().zip(&function.params) {
                match (arg, ty, mode) {
                    (CallArg::Move(id), ty, ParamMode::Value) if ty.is_owner() => {
                        let owner = scope
                            .get_mut(id)
                            .ok_or("E-TYPED-OWNER: unknown move source")?;
                        if owner.ty != *ty
                            || owner.mode != ParamMode::Value
                            || !owner.available
                            || owner.loaned
                            || owner.shared != 0
                        {
                            return Err("E-TYPED-OWNER: invalid move argument".into());
                        }
                        owner.available = false;
                    }
                    (CallArg::ReadOwner(id), ty, ParamMode::Read) if ty.is_owner() => {
                        let owner = scope
                            .get_mut(id)
                            .ok_or("E-TYPED-OWNER: unknown read root")?;
                        if owner.ty != *ty || !owner.available || owner.loaned {
                            return Err("E-TYPED-OWNER: invalid shared loan".into());
                        }
                        owner.shared = owner
                            .shared
                            .checked_add(1)
                            .ok_or("E-TYPED-OWNER: too many shared loans")?;
                    }
                    (CallArg::InoutOwner(id), ty, ParamMode::Inout) if ty.is_owner() => {
                        let owner = scope
                            .get_mut(id)
                            .ok_or("E-TYPED-OWNER: unknown inout root")?;
                        if owner.ty != *ty
                            || !owner.available
                            || !owner.mutable
                            || owner.loaned
                            || owner.shared != 0
                        {
                            return Err("E-TYPED-OWNER: invalid exclusive loan".into());
                        }
                        owner.loaned = true;
                    }
                    (CallArg::Value(value), ty, ParamMode::Value) if !ty.is_owner() => {
                        if verify_expr(value, scope, ids, functions, adts)? != *ty {
                            return Err("E-TYPED: wrong call argument type".into());
                        }
                    }
                    _ => return Err("E-TYPED-OWNER: invalid call argument mode".into()),
                }
            }
            for (id, shared, loaned) in loans {
                let local = scope.get_mut(&id).ok_or("E-TYPED: lost call scope")?;
                local.shared = shared;
                local.loaned = loaned;
            }
            function.result
        }
        ExprKind::BytesNew(n) if *n <= 256 => Type::Bytes(*n),
        ExprKind::BytesNew(_) => return Err("E-TYPED: invalid buffer capacity".into()),
        ExprKind::BytesLen(value) => {
            if !matches!(
                verify_expr(value, scope, ids, functions, adts)?,
                Type::Bytes(_)
            ) {
                return Err("E-TYPED: invalid bytes_len operand".into());
            }
            Type::Int
        }
        ExprKind::BytesGet(value, index) => {
            if !matches!(
                verify_expr(value, scope, ids, functions, adts)?,
                Type::Bytes(_)
            ) || verify_expr(index, scope, ids, functions, adts)? != Type::Int
            {
                return Err("E-TYPED: invalid bytes_get operands".into());
            }
            Type::Int
        }
        ExprKind::BytesSet(value, index, byte) => {
            let ty = verify_expr(value, scope, ids, functions, adts)?;
            if !matches!(ty, Type::Bytes(_))
                || verify_expr(index, scope, ids, functions, adts)? != Type::Int
                || verify_expr(byte, scope, ids, functions, adts)? != Type::Int
            {
                return Err("E-TYPED: invalid bytes_set operands".into());
            }
            ty
        }
        ExprKind::ByteLen(value) => {
            if verify_expr(value, scope, ids, functions, adts)? != Type::StaticStr {
                return Err("E-TYPED: invalid byte_len operand".into());
            }
            Type::Int
        }
        ExprKind::ByteAt(value, index) => {
            if verify_expr(value, scope, ids, functions, adts)? != Type::StaticStr
                || verify_expr(index, scope, ids, functions, adts)? != Type::Int
            {
                return Err("E-TYPED: invalid byte_at operands".into());
            }
            Type::Int
        }
        ExprKind::Let {
            local,
            mutable,
            value,
            body,
        } => {
            let value_ty = verify_expr(value, scope, ids, functions, adts)?;
            if value_ty.is_owner() {
                return Err("E-TYPED-OWNER: owner needs explicit init".into());
            }
            if !ids.insert(*local) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                *local,
                Local {
                    id: *local,
                    ty: value_ty,
                    mutable: *mutable,
                    available: true,
                    loaned: false,
                    shared: 0,
                    mode: ParamMode::Value,
                },
            );
            let result = verify_expr(body, scope, ids, functions, adts);
            scope.remove(local);
            result?
        }
        ExprKind::Assign(id, value) => {
            let local = *scope
                .get(id)
                .ok_or("E-TYPED: assignment outside local scope")?;
            if !local.mutable
                || local.ty.is_owner()
                || !local.available
                || local.loaned
                || verify_expr(value, scope, ids, functions, adts)? != local.ty
            {
                return Err("E-TYPED: invalid assignment".into());
            }
            Type::Unit
        }
        ExprKind::Binary(op, a, b) => {
            let left = verify_expr(a, scope, ids, functions, adts)?;
            let snapshot = scope.clone();
            let right = verify_expr(b, scope, ids, functions, adts)?;
            if matches!(op, And | Or) && !same_owner_states(&snapshot, scope) {
                return Err("E-TYPED-OWNER: move in conditional expression".into());
            }
            match op {
                Add | Sub | Mul | Div | Rem if left == Type::Int && right == Type::Int => Type::Int,
                Lt | Le | Gt | Ge if left == Type::Int && right == Type::Int => Type::Bool,
                And | Or if left == Type::Bool && right == Type::Bool => Type::Bool,
                Eq | Ne if left == right && matches!(left, Type::Int | Type::Bool | Type::Unit) => {
                    Type::Bool
                }
                _ => return Err("E-TYPED: invalid binary operand types".into()),
            }
        }
        ExprKind::If(c, a, b) => {
            if verify_expr(c, scope, ids, functions, adts)? != Type::Bool {
                return Err("E-TYPED: invalid condition".into());
            }
            let mut yes_scope = scope.clone();
            let a = verify_expr(a, &mut yes_scope, ids, functions, adts)?;
            let mut no_scope = scope.clone();
            if verify_expr(b, &mut no_scope, ids, functions, adts)? != a {
                return Err("E-TYPED: inconsistent branches".into());
            }
            if !same_owner_states(&yes_scope, &no_scope) {
                return Err(
                    "E-TYPED-OWNER: move in conditional has incompatible branch ownership".into(),
                );
            }
            *scope = yes_scope;
            a
        }
        ExprKind::While(c, b) => {
            let before = scope.clone();
            if verify_expr(c, scope, ids, functions, adts)? != Type::Bool {
                return Err("E-TYPED: invalid loop condition".into());
            }
            if !same_owner_states(&before, scope) {
                return Err("E-TYPED-OWNER: move in loop".into());
            }
            if verify_expr(b, scope, ids, functions, adts)? != Type::Unit {
                return Err("E-TYPED: invalid loop body".into());
            }
            if !same_owner_states(&before, scope) {
                return Err("E-TYPED-OWNER: move in loop".into());
            }
            Type::Unit
        }
    };
    if expr.ty != ty {
        return Err("E-TYPED: incorrect expression type".into());
    }
    Ok(ty)
}

fn any_expr(expr: &ExprNode, predicate: &dyn Fn(&ExprKind) -> bool) -> bool {
    if predicate(&expr.kind) {
        return true;
    }
    match &expr.kind {
        ExprKind::Constructor { fields, .. } => fields.iter().any(|e| any_expr(e, predicate)),
        ExprKind::Match { value, arms } => {
            any_expr(value, predicate) || arms.iter().any(|a| any_expr(&a.body, predicate))
        }
        ExprKind::OwnerLet { init, body, .. } => {
            matches!(init, OwnerInit::Alloc(value) | OwnerInit::BytesAlloc(value) | OwnerInit::Call(value) | OwnerInit::Expr(value) if any_expr(value, predicate))
                || any_expr(body, predicate)
        }
        ExprKind::OwnerBytesSet(_, a, b) => any_expr(a, predicate) || any_expr(b, predicate),
        ExprKind::TryAlloc { input: value, .. }
        | ExprKind::OwnerBytesGet(_, value)
        | ExprKind::OwnerWrite(_, value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => any_expr(value, predicate),
        ExprKind::Call(_, args) => args
            .iter()
            .any(|arg| matches!(arg, CallArg::Value(value) if any_expr(value, predicate))),
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => {
            any_expr(value, predicate) || any_expr(body, predicate)
        }
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            any_expr(a, predicate) || any_expr(b, predicate) || any_expr(c, predicate)
        }
        _ => false,
    }
}

fn collect_calls(expr: &ExprNode, out: &mut Vec<usize>) {
    match &expr.kind {
        ExprKind::Constructor { fields, .. } => {
            for field in fields {
                collect_calls(field, out);
            }
        }
        ExprKind::Match { value, arms } => {
            collect_calls(value, out);
            for arm in arms {
                collect_calls(&arm.body, out);
            }
        }
        ExprKind::Call(id, args) => {
            out.push(*id);
            for arg in args {
                if let CallArg::Value(value) = arg {
                    collect_calls(value, out);
                }
            }
        }
        ExprKind::OwnerLet { init, body, .. } => {
            if let OwnerInit::Alloc(value)
            | OwnerInit::BytesAlloc(value)
            | OwnerInit::Call(value)
            | OwnerInit::Expr(value) = init
            {
                collect_calls(value, out);
            }
            collect_calls(body, out);
        }
        ExprKind::OwnerBytesSet(_, a, b) => {
            collect_calls(a, out);
            collect_calls(b, out);
        }
        ExprKind::TryAlloc { input: value, .. }
        | ExprKind::OwnerBytesGet(_, value)
        | ExprKind::OwnerWrite(_, value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => collect_calls(value, out),
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => {
            collect_calls(value, out);
            collect_calls(body, out);
        }
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            collect_calls(a, out);
            collect_calls(b, out);
            collect_calls(c, out);
        }
        _ => {}
    }
}
fn verify_acyclic_calls(program: &TypedMain) -> Result<(), String> {
    let mut edges = Vec::new();
    let mut indegree = vec![0usize; program.functions.len()];
    for function in &program.functions {
        let mut targets = Vec::new();
        collect_calls(&function.body, &mut targets);
        for &target in &targets {
            let degree = indegree
                .get_mut(target)
                .ok_or("E-TYPED: invalid function identity")?;
            *degree = degree.checked_add(1).ok_or("E-TYPED: too many calls")?;
        }
        edges.push(targets);
    }
    let mut queue: std::collections::VecDeque<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while let Some(source) = queue.pop_front() {
        visited += 1;
        for &target in &edges[source] {
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if visited != program.functions.len() {
        return Err("E-TYPED-OWNER: recursive owner programs are unsupported".into());
    }
    Ok(())
}

fn same_owner_states(a: &HashMap<LocalId, Local>, b: &HashMap<LocalId, Local>) -> bool {
    a.iter()
        .filter(|(_, local)| local.ty.is_owner())
        .all(|(id, local)| {
            b.get(id).is_some_and(|other| {
                other.ty == local.ty
                    && other.available == local.available
                    && other.loaned == local.loaned
                    && other.shared == local.shared
            })
        })
}

/// Evaluate the provisional IR. Owned-box programs use this path in production;
/// other programs currently use it for differential validation. Verification is
/// repeated because callers can construct or modify IR nodes after lowering.
pub fn evaluate(program: &TypedMain) -> Result<crate::interpreter::Value, String> {
    verify(program)?;
    let mut state = EvalState::default();
    match evaluate_function(program, program.entry, Vec::new(), 1, &mut state)? {
        EvalArg::Value(value) => Ok(value),
        EvalArg::Owner(_) => Err("E-TYPED-OWNER: main returned an owner".into()),
    }
}

#[derive(Clone, Copy, Debug)]
struct OwnerHandle {
    id: usize,
    mode: ParamMode,
}

#[derive(Debug)]
enum EvalArg {
    Value(crate::interpreter::Value),
    Owner(OwnerHandle),
}

fn evaluate_function(
    program: &TypedMain,
    id: usize,
    args: Vec<EvalArg>,
    depth: usize,
    state: &mut EvalState,
) -> Result<EvalArg, String> {
    let function = &program.functions[id];
    let mut locals = HashMap::new();
    let mut owners = HashMap::new();
    for ((id, ty, mode), value) in function.params.iter().zip(args) {
        match (ty, mode, value) {
            (ty, mode, EvalArg::Owner(handle)) if ty.is_owner() && *mode == handle.mode => {
                owners.insert(*id, handle);
            }
            (ty, ParamMode::Value, EvalArg::Value(value)) if !ty.is_owner() => {
                locals.insert(*id, value);
            }
            _ => return Err("E-TYPED-OWNER: invalid runtime argument".into()),
        }
    }
    let result = if function.result.is_owner() {
        evaluate_owner_expr(
            &function.body,
            &mut locals,
            program,
            depth,
            state,
            &mut owners,
        )
        .map(EvalArg::Owner)
    } else {
        evaluate_expr(
            &function.body,
            &mut locals,
            program,
            depth,
            state,
            &mut owners,
        )
        .map(EvalArg::Value)
    };
    if result.is_ok() {
        for id in &function.param_cleanup {
            let handle = owners
                .remove(id)
                .ok_or("E-TYPED-OWNER: missing parameter cleanup")?;
            if handle.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: attempted borrowed cleanup".into());
            }
            state.release(handle.id)?;
        }
        if owners
            .values()
            .any(|handle| handle.mode == ParamMode::Value)
        {
            return Err("E-TYPED-OWNER: live owner escaped frame cleanup".into());
        }
    }
    result
}

fn evaluate_expr(
    expr: &ExprNode,
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<crate::interpreter::Value, String> {
    use crate::interpreter::Value;
    match &expr.kind {
        ExprKind::TryAlloc { .. } => Err("E-TYPED-ALLOC: affine allocation used as Copy".into()),
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => match adt_eval::construct(
            *adt, *variant, fields, locals, program, depth, state, owners,
        )? {
            EvalArg::Value(value) => Ok(value),
            _ => Err("E-TYPED-ADT: affine constructor used as Copy".into()),
        },
        ExprKind::Match { value, arms } => adt_eval::match_value(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_expr,
        ),
        ExprKind::OwnerLet { .. } => {
            evaluate_owner_let(expr, locals, program, depth, state, owners, evaluate_expr)
        }
        ExprKind::OwnerRead(id) => {
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing live owner")?;
            state
                .storage
                .get(&handle.id)
                .and_then(|value| match value {
                    OwnerStorage::BoxI32(value) => value.first(),
                    _ => None,
                })
                .copied()
                .map(Value::Int)
                .ok_or_else(|| "E-TYPED-OWNER: missing allocation".into())
        }
        ExprKind::OwnerWrite(id, value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid value".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing live owner")?;
            *state
                .storage
                .get_mut(&handle.id)
                .and_then(|owner| match owner {
                    OwnerStorage::BoxI32(value) => value.first_mut(),
                    _ => None,
                })
                .ok_or("E-TYPED-OWNER: missing allocation")? = value;
            Ok(Value::Unit)
        }
        ExprKind::OwnerBytesLen(id) => {
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            match state.storage.get(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => Ok(Value::Int(
                    i32::try_from(bytes.len()).map_err(|_| "runtime trap")?,
                )),
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::OwnerBytesGet(id, index) => {
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid bytes index".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            let index = usize::try_from(index).map_err(|_| "runtime trap")?;
            match state.storage.get(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => bytes
                    .get(index)
                    .copied()
                    .map(|b| Value::Int(i32::from(b)))
                    .ok_or_else(|| "runtime trap".into()),
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::OwnerBytesSet(id, index, value) => {
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid bytes index".into());
            };
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid byte".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            let index = usize::try_from(index).map_err(|_| "runtime trap")?;
            let value = u8::try_from(value).map_err(|_| "runtime trap")?;
            match state.storage.get_mut(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => {
                    *bytes.get_mut(index).ok_or("runtime trap")? = value;
                    Ok(Value::Unit)
                }
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::Int(n) => Ok(Value::Int(*n)),
        ExprKind::Str(value) => Ok(Value::Str(value.clone())),
        ExprKind::Bool(value) => Ok(Value::Bool(*value)),
        ExprKind::Unit => Ok(Value::Unit),
        ExprKind::Read(id) => locals
            .get(id)
            .cloned()
            .ok_or_else(|| "E-TYPED: missing local".into()),
        ExprKind::BytesNew(n) => Ok(Value::Bytes(vec![0; *n])),
        ExprKind::BytesLen(value) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_len", &[value])
        }
        ExprKind::BytesGet(value, index) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            let index = evaluate_expr(index, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_get", &[value, index])
        }
        ExprKind::BytesSet(value, index, byte) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            let index = evaluate_expr(index, locals, program, depth, state, owners)?;
            let byte = evaluate_expr(byte, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_set", &[value, index, byte])
        }
        ExprKind::ByteLen(value) => {
            let Value::Str(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid string".into());
            };
            i32::try_from(value.len())
                .map(Value::Int)
                .map_err(|_| "runtime trap".into())
        }
        ExprKind::ByteAt(value, index) => {
            let Value::Str(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid string".into());
            };
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid index".into());
            };
            usize::try_from(index)
                .ok()
                .and_then(|i| value.as_bytes().get(i))
                .map(|b| Value::Int(i32::from(*b)))
                .ok_or_else(|| "runtime trap".into())
        }
        ExprKind::OwnerMove(_) => Err("E-TYPED-OWNER: move cannot produce a scalar".into()),
        ExprKind::Call(id, args) => {
            match evaluate_call(*id, args, locals, program, depth, state, owners)? {
                EvalArg::Value(value) => Ok(value),
                EvalArg::Owner(_) => Err("E-TYPED-OWNER: owner used as a scalar".into()),
            }
        }
        ExprKind::Let {
            local, value, body, ..
        } => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            locals.insert(*local, value);
            let result = evaluate_expr(body, locals, program, depth, state, owners);
            locals.remove(local);
            result
        }
        ExprKind::Assign(id, value) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            *locals
                .get_mut(id)
                .ok_or("E-TYPED: missing assignment local")? = value;
            Ok(Value::Unit)
        }
        ExprKind::Binary(op, a, b) => {
            let a = evaluate_expr(a, locals, program, depth, state, owners)?;
            if *op == BinaryOp::And && a == Value::Bool(false) {
                return Ok(a);
            }
            if *op == BinaryOp::Or && a == Value::Bool(true) {
                return Ok(a);
            }
            let b = evaluate_expr(b, locals, program, depth, state, owners)?;
            crate::interpreter::binary(op, a, b)
        }
        ExprKind::If(c, a, b) => match evaluate_expr(c, locals, program, depth, state, owners)? {
            Value::Bool(true) => evaluate_expr(a, locals, program, depth, state, owners),
            Value::Bool(false) => evaluate_expr(b, locals, program, depth, state, owners),
            _ => Err("E-TYPED: invalid condition".into()),
        },
        ExprKind::While(c, body) => {
            loop {
                match evaluate_expr(c, locals, program, depth, state, owners)? {
                    Value::Bool(false) => break,
                    Value::Bool(true) => {
                        evaluate_expr(body, locals, program, depth, state, owners)?;
                    }
                    _ => return Err("E-TYPED: invalid condition".into()),
                }
            }
            Ok(Value::Unit)
        }
    }
}

enum OwnerStorage {
    Adt {
        adt: usize,
        variant: usize,
        fields: Vec<EvalArg>,
    },
    BoxI32(Vec<i32>),
    Bytes(Vec<u8>),
}

fn evaluate_call(
    id: usize,
    args: &[CallArg],
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<EvalArg, String> {
    let mut values = Vec::new();
    for arg in args {
        match arg {
            CallArg::Value(value) => values.push(EvalArg::Value(evaluate_expr(
                value, locals, program, depth, state, owners,
            )?)),
            CallArg::Move(id) => values.push(EvalArg::Owner(
                owners
                    .remove(id)
                    .ok_or("E-TYPED-OWNER: missing move argument")?,
            )),
            CallArg::ReadOwner(id) | CallArg::InoutOwner(id) => {
                let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing borrow root")?;
                let mode = if matches!(arg, CallArg::ReadOwner(_)) {
                    ParamMode::Read
                } else {
                    ParamMode::Inout
                };
                values.push(EvalArg::Owner(OwnerHandle {
                    id: handle.id,
                    mode,
                }));
            }
        }
    }
    if depth >= 32 {
        return Err("runtime trap".into());
    }
    evaluate_function(program, id, values, depth + 1, state)
}

fn evaluate_owner_expr(
    expr: &ExprNode,
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<OwnerHandle, String> {
    match &expr.kind {
        ExprKind::TryAlloc {
            kind,
            input,
            layout,
        } => {
            let crate::interpreter::Value::Int(input) =
                evaluate_expr(input, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-ALLOC: invalid input".into());
            };
            allocation::evaluate(*kind, input, layout, program, state)
        }
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => match adt_eval::construct(
            *adt, *variant, fields, locals, program, depth, state, owners,
        )? {
            EvalArg::Owner(owner) => Ok(owner),
            _ => Err("E-TYPED-ADT: Copy constructor used as affine".into()),
        },
        ExprKind::Match { value, arms } => adt_eval::match_value(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        ExprKind::If(condition, yes, no) => {
            match evaluate_expr(condition, locals, program, depth, state, owners)? {
                crate::interpreter::Value::Bool(true) => {
                    evaluate_owner_expr(yes, locals, program, depth, state, owners)
                }
                crate::interpreter::Value::Bool(false) => {
                    evaluate_owner_expr(no, locals, program, depth, state, owners)
                }
                _ => Err("E-TYPED-ADT: invalid condition".into()),
            }
        }
        ExprKind::OwnerMove(id) => {
            let handle = owners
                .remove(id)
                .ok_or("E-TYPED-OWNER: missing returned owner")?;
            if handle.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: returned borrowed owner".into());
            }
            Ok(handle)
        }
        ExprKind::Call(id, args) => {
            match evaluate_call(*id, args, locals, program, depth, state, owners)? {
                EvalArg::Owner(owner) => Ok(owner),
                EvalArg::Value(_) => Err("E-TYPED-OWNER: scalar used as an owner".into()),
            }
        }
        ExprKind::OwnerLet { .. } => evaluate_owner_let(
            expr,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        ExprKind::Let {
            local, value, body, ..
        } => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            locals.insert(*local, value);
            let result = evaluate_owner_expr(body, locals, program, depth, state, owners);
            locals.remove(local);
            result
        }
        _ => Err("E-TYPED-OWNER: unsupported owner result".into()),
    }
}

#[derive(Default)]
struct EvalState {
    storage: HashMap<usize, OwnerStorage>,
    next_id: usize,
    allocations: usize,
    frees: usize,
    #[cfg(test)]
    released: Vec<i32>,
    fail_after: Option<usize>,
    fail_at_attempt: Option<usize>,
    allocation_attempts: usize,
    #[cfg(test)]
    allocation_trace: Vec<(usize, SystemAllocationKind)>,
    #[cfg(test)]
    release_trace: Vec<(usize, SystemAllocationKind)>,
}
impl EvalState {
    fn begin_attempt(&mut self) -> Result<bool, String> {
        self.allocation_attempts = self
            .allocation_attempts
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: allocation attempt overflow")?;
        Ok(self.fail_after == Some(self.allocations)
            || self.fail_at_attempt == Some(self.allocation_attempts))
    }
    fn allocate(&mut self, value: i32) -> Result<OwnerHandle, String> {
        self.try_allocate(value)?.map_err(|_| "runtime trap".into())
    }
    fn try_allocate(
        &mut self,
        value: i32,
    ) -> Result<Result<OwnerHandle, AllocationFailure>, String> {
        if self.begin_attempt()? {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        self.storage
            .try_reserve(1)
            .map_err(|_| "E-TYPED-OWNER: interpreter storage exhausted")?;
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: identity overflow")?;
        let mut storage = Vec::new();
        if storage.try_reserve_exact(1).is_err() {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        storage.push(value);
        self.storage.insert(id, OwnerStorage::BoxI32(storage));
        #[cfg(test)]
        self.allocation_trace
            .push((id, SystemAllocationKind::BoxI32));
        self.next_id = next;
        self.allocations += 1;
        Ok(Ok(OwnerHandle {
            id,
            mode: ParamMode::Value,
        }))
    }
    fn release(&mut self, id: usize) -> Result<(), String> {
        let storage = self
            .storage
            .remove(&id)
            .ok_or("E-TYPED-OWNER: missing allocation")?;
        if let OwnerStorage::Adt { fields, .. } = storage {
            for field in fields.into_iter().rev() {
                if let EvalArg::Owner(owner) = field {
                    self.release(owner.id)?;
                }
            }
            return Ok(());
        }
        self.frees += 1;
        #[cfg(test)]
        self.release_trace.push((
            id,
            match &storage {
                OwnerStorage::BoxI32(_) => SystemAllocationKind::BoxI32,
                OwnerStorage::Bytes(_) => SystemAllocationKind::BytesZeroed,
                OwnerStorage::Adt { .. } => unreachable!(),
            },
        ));
        #[cfg(test)]
        if let OwnerStorage::BoxI32(ref value) = storage {
            self.released.push(value[0]);
        }
        drop(storage);
        Ok(())
    }
    fn allocate_bytes(&mut self, len: i32) -> Result<OwnerHandle, String> {
        self.try_allocate_bytes(len)?
            .map_err(|_| "runtime trap".into())
    }
    fn try_allocate_bytes(
        &mut self,
        len: i32,
    ) -> Result<Result<OwnerHandle, AllocationFailure>, String> {
        if let Err(error) = allocation::checked_byte_size(
            len,
            std::mem::size_of::<usize>() as u128,
            usize::MAX as u128,
        ) {
            return Ok(Err(error));
        }
        let len = usize::try_from(len).map_err(|_| "E-TYPED-OWNER: invalid checked length")?;
        if self.begin_attempt()? {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        self.storage
            .try_reserve(1)
            .map_err(|_| "E-TYPED-OWNER: interpreter storage exhausted")?;
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: identity overflow")?;
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len.max(1)).is_err() {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        bytes.resize(len, 0);
        self.storage.insert(id, OwnerStorage::Bytes(bytes));
        #[cfg(test)]
        self.allocation_trace
            .push((id, SystemAllocationKind::BytesZeroed));
        self.next_id = next;
        self.allocations += 1;
        Ok(Ok(OwnerHandle {
            id,
            mode: ParamMode::Value,
        }))
    }
}

type EvaluateBody<T> = fn(
    &ExprNode,
    &mut HashMap<LocalId, crate::interpreter::Value>,
    &TypedMain,
    usize,
    &mut EvalState,
    &mut HashMap<LocalId, OwnerHandle>,
) -> Result<T, String>;

#[inline(never)]
fn evaluate_owner_let<T>(
    expr: &ExprNode,
    locals: &mut HashMap<LocalId, crate::interpreter::Value>,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
    evaluate_body: EvaluateBody<T>,
) -> Result<T, String> {
    use crate::interpreter::Value;
    let ExprKind::OwnerLet {
        local,
        init,
        body,
        cleanup,
        ..
    } = &expr.kind
    else {
        unreachable!()
    };
    let storage = match init {
        OwnerInit::Alloc(value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid allocation value".into());
            };
            state.allocate(value)?
        }
        OwnerInit::BytesAlloc(value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid allocation length".into());
            };
            state.allocate_bytes(value)?
        }
        OwnerInit::Call(value) | OwnerInit::Expr(value) => {
            evaluate_owner_expr(value, locals, program, depth, state, owners)?
        }
        OwnerInit::Move(source) => owners
            .remove(source)
            .ok_or("E-TYPED-OWNER: missing move source")?,
    };
    owners.insert(*local, storage);
    let result = evaluate_body(body, locals, program, depth, state, owners);
    if result.is_ok() {
        if let Some(id) = cleanup {
            let storage = owners
                .remove(id)
                .ok_or("E-TYPED-OWNER: missing cleanup owner")?;
            if storage.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: attempted borrowed cleanup".into());
            }
            state.release(storage.id)?;
        }
    } else {
        owners.remove(local);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn checked_mutation_and_shadowing_have_distinct_locals() {
        let p = parser::parse("let main = let mut x = 1 in let ignored = (let x = true in x) in let done = x <- x + 1 in x").unwrap();
        let checked = check::analyze(&p).unwrap();
        let ir = lower_main(&checked).unwrap();
        verify(&ir).unwrap();
    }

    #[test]
    fn rejects_unimplemented_features_without_lowering() {
        let p = parser::parse("let id x = x\nlet main = id(1)").unwrap();
        let checked = check::analyze(&p).unwrap();
        assert!(lower_main(&checked)
            .unwrap_err()
            .contains("E-TYPED-UNSUPPORTED"));
    }

    #[test]
    fn verifier_rejects_out_of_scope_and_bad_types() {
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Int,
                body: ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Read(LocalId(0)),
                },
            }],
        };
        assert!(verify(&p).unwrap_err().contains("outside local scope"));
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Bool,
                body: ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Int(1),
                },
            }],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("incorrect expression type"));
    }
    #[test]
    fn verifier_rejects_invalid_string_operations() {
        let unit = || ExprNode {
            ty: Type::Unit,
            kind: ExprKind::Unit,
        };
        let body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::ByteLen(Box::new(unit())),
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Int,
                body,
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid byte_len"));
        let string = || ExprNode {
            ty: Type::StaticStr,
            kind: ExprKind::Str("a".into()),
        };
        let body = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Binary(BinaryOp::Eq, Box::new(string()), Box::new(string())),
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Bool,
                body,
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid binary"));
    }

    #[test]
    fn verifier_rejects_invalid_buffer_operations() {
        let bytes = ExprNode {
            ty: Type::Bytes(1),
            kind: ExprKind::BytesNew(1),
        };
        let bad_index = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Bool(true),
        };
        let body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::BytesGet(Box::new(bytes), Box::new(bad_index)),
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Int,
                body,
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid bytes_get"));
        let body = ExprNode {
            ty: Type::Bytes(257),
            kind: ExprKind::BytesNew(257),
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: Type::Bytes(257),
                body,
            }],
        };
        assert!(verify(&program)
            .unwrap_err()
            .contains("invalid buffer capacity"));
    }

    fn owner_program(body: ExprNode) -> TypedMain {
        TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: body.ty,
                body,
            }],
        }
    }
    fn int(value: i32) -> ExprNode {
        ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(value),
        }
    }
    fn owned(local: u32, init: OwnerInit, body: ExprNode, cleanup: Option<u32>) -> ExprNode {
        ExprNode {
            ty: body.ty,
            kind: ExprKind::OwnerLet {
                local: LocalId(local),
                mutable: true,
                init,
                body: Box::new(body),
                cleanup: cleanup.map(LocalId),
            },
        }
    }
    fn alloc(value: i32) -> OwnerInit {
        OwnerInit::Alloc(Box::new(int(value)))
    }

    #[test]
    fn bytes_owner_verifier_rejects_wrong_kind_and_overlapping_operands() {
        let bytes_len = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerBytesLen(LocalId(0)),
        };
        assert!(verify(&owner_program(owned(0, alloc(1), bytes_len, Some(0)))).is_err());
        let box_read = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerRead(LocalId(0)),
        };
        let bytes_alloc = || OwnerInit::BytesAlloc(Box::new(int(2)));
        assert!(verify(&owner_program(owned(0, bytes_alloc(), box_read, Some(0)))).is_err());
        let nested_read = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerBytesLen(LocalId(0)),
        };
        let set = ExprNode {
            ty: Type::Unit,
            kind: ExprKind::OwnerBytesSet(LocalId(0), Box::new(nested_read), Box::new(int(0))),
        };
        assert!(verify(&owner_program(owned(0, bytes_alloc(), set, Some(0)))).is_err());

        let call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(1, vec![CallArg::Move(LocalId(0))]),
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![
                TypedFunction {
                    params: vec![],
                    result: Type::Int,
                    body: owned(0, bytes_alloc(), call, None),
                    param_cleanup: vec![],
                },
                TypedFunction {
                    params: vec![(LocalId(1), Type::OwnedI32, ParamMode::Value)],
                    result: Type::Int,
                    body: int(0),
                    param_cleanup: vec![LocalId(1)],
                },
            ],
        };
        assert!(verify(&program).is_err());

        for (mode, arg) in [
            (ParamMode::Read, CallArg::ReadOwner(LocalId(0))),
            (ParamMode::Inout, CallArg::InoutOwner(LocalId(0))),
        ] {
            let call = ExprNode {
                ty: Type::Int,
                kind: ExprKind::Call(1, vec![arg]),
            };
            let program = TypedMain {
                allocation_contract: None,
                adts: Vec::new(),
                entry: 0,
                functions: vec![
                    TypedFunction {
                        params: vec![],
                        result: Type::Int,
                        body: owned(0, bytes_alloc(), call, Some(0)),
                        param_cleanup: vec![],
                    },
                    TypedFunction {
                        params: vec![(LocalId(1), Type::OwnedI32, mode)],
                        result: Type::Int,
                        body: int(0),
                        param_cleanup: vec![],
                    },
                ],
            };
            assert!(verify(&program).is_err());
        }
    }

    #[test]
    fn ownership_verifier_rejects_forged_moves_and_cleanup() {
        let moved_then_dropped_twice = owned(
            0,
            alloc(1),
            owned(1, OwnerInit::Move(LocalId(0)), int(0), Some(1)),
            Some(0),
        );
        assert!(verify(&owner_program(moved_then_dropped_twice))
            .unwrap_err()
            .contains("invalid cleanup"));
        let second_move = owned(
            0,
            alloc(1),
            owned(
                1,
                OwnerInit::Move(LocalId(0)),
                owned(2, OwnerInit::Move(LocalId(0)), int(0), Some(2)),
                Some(1),
            ),
            None,
        );
        assert!(verify(&owner_program(second_move))
            .unwrap_err()
            .contains("invalid move"));
        let read_moved = owned(
            0,
            alloc(1),
            owned(
                1,
                OwnerInit::Move(LocalId(0)),
                ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::OwnerRead(LocalId(0)),
                },
                Some(1),
            ),
            None,
        );
        assert!(verify(&owner_program(read_moved))
            .unwrap_err()
            .contains("invalid owner read"));
        let copy_owner = owned(
            0,
            alloc(1),
            ExprNode {
                ty: Type::OwnedI32,
                kind: ExprKind::Read(LocalId(0)),
            },
            Some(0),
        );
        assert!(verify(&owner_program(copy_owner)).is_err());
        let right = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerRead(LocalId(0)),
        };
        let write_during_loan = owned(
            0,
            alloc(1),
            ExprNode {
                ty: Type::Unit,
                kind: ExprKind::OwnerWrite(LocalId(0), Box::new(right)),
            },
            Some(0),
        );
        assert!(verify(&owner_program(write_during_loan))
            .unwrap_err()
            .contains("invalid owner read"));
    }

    #[test]
    fn ownership_verifier_rejects_conditional_and_loop_moves() {
        let moved_branch = owned(1, OwnerInit::Move(LocalId(0)), int(0), Some(1));
        let conditional = ExprNode {
            ty: Type::Int,
            kind: ExprKind::If(
                Box::new(ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Bool(true),
                }),
                Box::new(moved_branch),
                Box::new(int(0)),
            ),
        };
        let p = owner_program(owned(0, alloc(1), conditional, None));
        assert!(verify(&p).unwrap_err().contains("move in conditional"));
        let body = owned(
            1,
            OwnerInit::Move(LocalId(0)),
            ExprNode {
                ty: Type::Unit,
                kind: ExprKind::Unit,
            },
            Some(1),
        );
        let loop_node = ExprNode {
            ty: Type::Unit,
            kind: ExprKind::While(
                Box::new(ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Bool(false),
                }),
                Box::new(body),
            ),
        };
        let p = owner_program(owned(0, alloc(1), loop_node, None));
        assert!(verify(&p).unwrap_err().contains("move in loop"));
    }

    #[test]
    fn ownership_verifier_rejects_invalid_call_operands_and_parameter_cleanup() {
        let owner_params = |count: u32, cleanup: Vec<LocalId>| TypedFunction {
            params: (0..count)
                .map(|i| (LocalId(i), Type::OwnedI32, ParamMode::Value))
                .collect(),
            result: Type::Int,
            body: int(0),
            param_cleanup: cleanup,
        };
        let call = |args| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(0, args),
        };
        let main = |body| TypedFunction {
            params: vec![],
            result: Type::Int,
            body,
            param_cleanup: vec![],
        };
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                owner_params(2, vec![LocalId(1), LocalId(0)]),
                main(owned(
                    2,
                    alloc(1),
                    call(vec![CallArg::Move(LocalId(2)), CallArg::Move(LocalId(2))]),
                    None,
                )),
            ],
        };
        assert!(verify(&p).unwrap_err().contains("invalid move argument"));
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                owner_params(1, vec![LocalId(0)]),
                main(call(vec![CallArg::Value(int(1))])),
            ],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("invalid call argument mode"));
        let scalar = TypedFunction {
            params: vec![(LocalId(0), Type::Int, ParamMode::Value)],
            result: Type::Int,
            body: int(0),
            param_cleanup: vec![],
        };
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                scalar,
                main(owned(
                    1,
                    alloc(1),
                    call(vec![CallArg::Move(LocalId(1))]),
                    None,
                )),
            ],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("invalid call argument mode"));
        for cleanup in [vec![], vec![LocalId(0), LocalId(1)]] {
            let p = TypedMain {
                allocation_contract: None,
                adts: Vec::new(),
                entry: 1,
                functions: vec![owner_params(2, cleanup), main(int(0))],
            };
            assert!(verify(&p)
                .unwrap_err()
                .contains("invalid parameter cleanup"));
        }
    }

    #[test]
    fn ownership_verifier_rejects_forged_borrow_aliases_and_cleanup() {
        let function = |modes: Vec<ParamMode>| TypedFunction {
            params: modes
                .into_iter()
                .enumerate()
                .map(|(i, mode)| (LocalId(i as u32 + 1), Type::OwnedI32, mode))
                .collect(),
            result: Type::Int,
            body: int(0),
            param_cleanup: vec![],
        };
        let main = |body| TypedFunction {
            params: vec![],
            result: Type::Int,
            body,
            param_cleanup: vec![],
        };
        let call = |id, args| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(id, args),
        };
        for (modes, args) in [
            (
                vec![ParamMode::Inout, ParamMode::Inout],
                vec![
                    CallArg::InoutOwner(LocalId(0)),
                    CallArg::InoutOwner(LocalId(0)),
                ],
            ),
            (
                vec![ParamMode::Read, ParamMode::Inout],
                vec![
                    CallArg::ReadOwner(LocalId(0)),
                    CallArg::InoutOwner(LocalId(0)),
                ],
            ),
            (
                vec![ParamMode::Inout, ParamMode::Read],
                vec![
                    CallArg::InoutOwner(LocalId(0)),
                    CallArg::ReadOwner(LocalId(0)),
                ],
            ),
        ] {
            let program = TypedMain {
                allocation_contract: None,
                adts: Vec::new(),
                entry: 1,
                functions: vec![
                    function(modes),
                    main(owned(0, alloc(1), call(0, args), Some(0))),
                ],
            };
            assert!(verify(&program).unwrap_err().contains("loan"));
        }
        let good = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                function(vec![ParamMode::Read, ParamMode::Read]),
                main(owned(
                    0,
                    alloc(1),
                    call(
                        0,
                        vec![
                            CallArg::ReadOwner(LocalId(0)),
                            CallArg::ReadOwner(LocalId(0)),
                        ],
                    ),
                    Some(0),
                )),
            ],
        };
        verify(&good).unwrap();
        let bad_move = TypedFunction {
            params: vec![(LocalId(1), Type::OwnedI32, ParamMode::Read)],
            result: Type::Int,
            body: owned(2, OwnerInit::Move(LocalId(1)), int(0), Some(2)),
            param_cleanup: vec![],
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![bad_move, main(int(0))],
        };
        assert!(verify(&program).unwrap_err().contains("invalid move"));
        let bad_cleanup = TypedFunction {
            param_cleanup: vec![LocalId(1)],
            ..function(vec![ParamMode::Read])
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![bad_cleanup, main(int(0))],
        };
        assert!(verify(&program)
            .unwrap_err()
            .contains("invalid parameter cleanup"));
    }

    #[test]
    fn ownership_verifier_rejects_nested_argument_reborrow() {
        let outer = TypedFunction {
            params: vec![
                (LocalId(1), Type::OwnedI32, ParamMode::Read),
                (LocalId(2), Type::Int, ParamMode::Value),
            ],
            result: Type::Int,
            body: int(0),
            param_cleanup: vec![],
        };
        let inner = TypedFunction {
            params: vec![(LocalId(3), Type::OwnedI32, ParamMode::Inout)],
            result: Type::Int,
            body: int(0),
            param_cleanup: vec![],
        };
        let inner_call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(1, vec![CallArg::InoutOwner(LocalId(0))]),
        };
        let outer_call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(
                0,
                vec![CallArg::ReadOwner(LocalId(0)), CallArg::Value(inner_call)],
            ),
        };
        let main = TypedFunction {
            params: vec![],
            result: Type::Int,
            body: owned(0, alloc(1), outer_call, Some(0)),
            param_cleanup: vec![],
        };
        let program = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 2,
            functions: vec![outer, inner, main],
        };
        assert!(verify(&program).unwrap_err().contains("exclusive loan"));
    }

    #[test]
    fn ownership_verifier_rejects_recursive_call_graphs() {
        let call = |id| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(id, vec![]),
        };
        let function = |body| TypedFunction {
            params: vec![],
            result: Type::Int,
            body,
            param_cleanup: vec![],
        };
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![function(owned(0, alloc(1), call(0), Some(0)))],
        };
        assert!(verify(&p).unwrap_err().contains("recursive owner programs"));
        let p = TypedMain {
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![
                function(owned(0, alloc(1), call(1), Some(0))),
                function(call(0)),
            ],
        };
        assert!(verify(&p).unwrap_err().contains("recursive owner programs"));
    }

    #[test]
    fn returned_owner_runtime_transfers_cleanup_and_preserves_traps() {
        let source = parser::parse("let make n = let returned = system_box_i32_abort(n) in let scratch = system_box_i32_abort(7) in move returned\nlet main = let owner = make(42) in box_read(owner)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        let mut state = EvalState::default();
        assert!(matches!(
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap(),
            EvalArg::Value(crate::interpreter::Value::Int(42))
        ));
        assert_eq!(state.allocations, 2);
        assert_eq!(state.released, vec![7, 42]);
        assert!(state.storage.is_empty());
        for failed in [0, 1] {
            let mut state = EvalState {
                fail_after: Some(failed),
                ..EvalState::default()
            };
            assert_eq!(
                evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap_err(),
                "runtime trap"
            );
            assert_eq!(state.allocations, failed);
            // Abort policy promises no Rill cleanup after an allocation trap.
            assert_eq!(state.frees, 0);
        }
    }

    #[test]
    fn verifier_rejects_forged_owner_returns_and_initializers() {
        let source = parser::parse("let pass owner = let n = box_read(owner) in move owner\nlet main = let original = system_box_i32_abort(42) in let returned = pass(move original) in box_read(returned)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        verify(&typed).unwrap();
        let returned_param = typed.functions[0].params[0].0;

        let mut cleanup_return = typed.clone();
        cleanup_return.functions[0]
            .param_cleanup
            .push(returned_param);
        assert!(verify(&cleanup_return).is_err());

        let mut borrow_return = typed.clone();
        borrow_return.functions[0].params[0].2 = ParamMode::Read;
        assert!(verify(&borrow_return).is_err());

        let mut copied_return = typed.clone();
        copied_return.functions[0].body = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Read(returned_param),
        };
        assert!(verify(&copied_return).is_err());

        let mut recursive = typed.clone();
        let owner_call = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Call(0, vec![CallArg::Move(returned_param)]),
        };
        recursive.functions[0].body = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::OwnerLet {
                local: LocalId(900),
                mutable: false,
                init: OwnerInit::Call(Box::new(owner_call)),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
                cleanup: None,
            },
        };
        assert!(verify(&recursive).unwrap_err().contains("recursive"));

        let mut invalid_initializer = typed.clone();
        invalid_initializer.functions[0].body = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::OwnerLet {
                local: LocalId(900),
                mutable: false,
                init: OwnerInit::Call(Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(returned_param),
                })),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
                cleanup: None,
            },
        };
        assert!(verify(&invalid_initializer)
            .unwrap_err()
            .contains("direct call"));

        let mut ordinary_let = typed;
        ordinary_let.functions[0].body = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Let {
                local: LocalId(900),
                mutable: false,
                value: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(returned_param),
                }),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
            },
        };
        assert!(verify(&ordinary_let).unwrap_err().contains("explicit init"));
    }

    #[test]
    fn owner_allocator_tracks_cleanup_and_oom() {
        for (source, allocations, releases) in [
            ("let main = let a = system_box_i32_abort(1) in let b = system_box_i32_abort(2) in box_read(a) + box_read(b)", 2, vec![2, 1]),
            ("let main = let a = system_box_i32_abort(1) in let b = move a in box_read(b)", 1, vec![1]),
            ("let main = let mut i = 0 in let done = while i < 3 do let b = system_box_i32_abort(i) in i <- i + 1 in i", 3, vec![0, 1, 2]),
        ] {
            let source = parser::parse(source).unwrap();
            let checked = check::analyze(&source).unwrap();
            let typed = lower_main(&checked).unwrap();
            let mut state = EvalState::default();
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap();
            assert_eq!(state.allocations, allocations);
            assert_eq!(state.frees, releases.len());
            assert_eq!(state.released, releases);
        }
        let source =
            parser::parse("let main = let b = system_box_i32_abort(1) in box_read(b)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        let mut state = EvalState {
            fail_after: Some(0),
            ..EvalState::default()
        };
        assert_eq!(
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap_err(),
            "runtime trap"
        );
        assert_eq!(state.allocations, 0);
        assert_eq!(state.frees, 0);
    }

    #[test]
    fn typed_evaluator_matches_checked_ast_interpreter() {
        for source in [
            "let main = 2 + 3 * 4",
            "let main = 2147483647 + 1",
            "let main = false && (1 / 0 == 0)",
            "let main = if true then 42 else 1 / 0",
            "let main = let mut x = 0 in let ignored = while x < 100 do x <- x + 1 in x",
            "let main = let mut x = 1 in let ignored = (let x = true in x) in let done = x <- x + 1 in x",
            "let main = let mut x = 0 in let ignored = x <- (let done = x <- 1 in x + 1) in x",
        ] {
            let program = parser::parse(source).unwrap();
            let checked = check::analyze(&program).unwrap();
            let ir = lower_main(&checked).unwrap();
            assert_eq!(evaluate(&ir), crate::interpreter::run(&program), "{source}");
        }
    }
}
