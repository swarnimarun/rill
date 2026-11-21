//! Opaque native declarations and exact Copy scalar/raw-pointer transport.
//! This manifest retains all source declarations, not only reachable operations.
//! It asserts no effects, pointer lifetimes, ownership or target compatibility.
use super::*;
use crate::ast::{ExternConst, ExternFunction, ExternType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ForeignFunctionId(pub(super) u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ForeignConstantId(pub(super) u32);

/// Nominal C pointee spelling and constness, never an address or Rill loan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PointerTypeId(pub(super) u32);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PointerType {
    pub pointee: String,
    pub is_const: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ForeignConvert {
    U8,
    U32,
    U64,
    F32,
}
impl ForeignConvert {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "u8" => Some(Self::U8),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "f32" => Some(Self::F32),
            _ => None,
        }
    }
    pub(super) fn result(self) -> Type {
        match self {
            Self::U8 => Type::CU8,
            Self::U32 => Type::CU32,
            Self::U64 => Type::CU64,
            Self::F32 => Type::CF32,
        }
    }
}

// Pointer spelling is preserved, not canonicalized against host C typedefs.
// Reserve our generated namespace, not header-owned C typedef identifiers.
// System names such as _IO_FILE and valid type keywords retain their spelling.
pub(super) fn validate_pointer(pointee: &str) -> Result<(), String> {
    crate::check::validate_extern_type(&ExternType::Pointer {
        pointee: pointee.into(),
        is_const: false,
    })?;
    if pointee.starts_with("rl_")
        || matches!(
            pointee,
            "auto"
                | "break"
                | "case"
                | "const"
                | "continue"
                | "default"
                | "do"
                | "else"
                | "enum"
                | "extern"
                | "for"
                | "goto"
                | "if"
                | "inline"
                | "register"
                | "restrict"
                | "return"
                | "sizeof"
                | "static"
                | "struct"
                | "switch"
                | "typedef"
                | "union"
                | "volatile"
                | "while"
        )
    {
        return Err("E-TYPED-FFI: invalid or reserved C pointee name".into());
    }
    Ok(())
}

pub(super) fn intern_pointer(
    pointers: &mut Vec<PointerType>,
    pointee: &str,
    is_const: bool,
) -> Result<PointerTypeId, String> {
    validate_pointer(pointee)?;
    if let Some(id) = pointers
        .iter()
        .position(|p| p.pointee == pointee && p.is_const == is_const)
    {
        return u32::try_from(id)
            .map(PointerTypeId)
            .map_err(|_| "E-TYPED-FFI: excessive pointer registry".into());
    }
    // In addition to the checked source/type traversal budgets, keep native
    // identities explicitly bounded. There is no host layout or target cache.
    if pointers.len() >= 4096 {
        return Err("E-TYPED-FFI: excessive pointer registry".into());
    }
    let id =
        u32::try_from(pointers.len()).map_err(|_| "E-TYPED-FFI: excessive pointer registry")?;
    pointers.push(PointerType {
        pointee: pointee.into(),
        is_const,
    });
    Ok(PointerTypeId(id))
}

/// A native parameter is by value or a direct static UTF-8 C literal, never a
/// Rill loan. C strings deliberately have no storable runtime `Type`.
#[derive(Clone, Debug)]
pub enum ForeignArg {
    Value(ExprNode),
    StaticCStr(String),
}

#[derive(Clone, Debug, Default)]
pub(super) struct NativeManifest {
    pub config: crate::c_import::Config,
    pub functions: Vec<ExternFunction>,
    pub constants: Vec<ExternConst>,
    pub pointers: Vec<PointerType>,
}

impl NativeManifest {
    pub fn from_checked(checked: &CheckedProgram<'_>) -> Result<Self, String> {
        let mut manifest = Self {
            config: checked.program.native.clone(),
            functions: checked.program.externs.clone(),
            constants: checked.program.constants.clone(),
            pointers: Vec::new(),
        };
        for ty in manifest
            .functions
            .iter()
            .flat_map(|f| {
                f.params
                    .iter()
                    .map(|p| &p.ty)
                    .chain(std::iter::once(&f.result))
            })
            .chain(manifest.constants.iter().map(|c| &c.ty))
        {
            if let ExternType::Pointer { pointee, is_const } = ty {
                intern_pointer(&mut manifest.pointers, pointee, *is_const)?;
            }
        }
        manifest.verify()?;
        Ok(manifest)
    }

    pub fn has_foreign(&self) -> bool {
        !self.config.includes.is_empty()
            || !self.functions.is_empty()
            || !self.constants.is_empty()
            || !self.pointers.is_empty()
    }

    pub fn verify(&self) -> Result<(), String> {
        self.config.validate()?;
        u32::try_from(self.functions.len()).map_err(|_| "E-TYPED-FFI: too many functions")?;
        u32::try_from(self.constants.len()).map_err(|_| "E-TYPED-FFI: too many constants")?;
        if self.pointers.len() > 4096 {
            return Err("E-TYPED-FFI: excessive pointer registry".into());
        }
        let mut pointer_keys = HashSet::new();
        for pointer in &self.pointers {
            validate_pointer(&pointer.pointee)?;
            if !pointer_keys.insert(pointer) {
                return Err("E-TYPED-FFI: duplicate pointer identity".into());
            }
        }
        for ty in self
            .functions
            .iter()
            .flat_map(|f| {
                f.params
                    .iter()
                    .map(|p| &p.ty)
                    .chain(std::iter::once(&f.result))
            })
            .chain(self.constants.iter().map(|c| &c.ty))
        {
            if matches!(ty, ExternType::Pointer { .. }) && self.runtime_type(ty).is_none() {
                return Err("E-TYPED-FFI: missing native pointer identity".into());
            }
        }
        let mut symbols = HashSet::new();
        for function in &self.functions {
            crate::check::validate_extern_function(function)?;
            for param in &function.params {
                // Names are source metadata, not C parameter declarators. Match
                // source identifier syntax rather than imposing C keywords.
                let tokens = crate::lexer::lex(&param.name)?;
                if !matches!(tokens.as_slice(), [crate::lexer::Token { kind: crate::lexer::Kind::Ident(name), .. }, crate::lexer::Token { kind: crate::lexer::Kind::Eof, .. }] if name == &param.name)
                {
                    return Err("E-TYPED-FFI: invalid parameter name".into());
                }
            }
            if !symbols.insert(&function.name) {
                return Err("E-TYPED-FFI: duplicate native symbol".into());
            }
        }
        if !self.constants.is_empty() && self.config.includes.is_empty() {
            return Err("E-TYPED-FFI: foreign constants require @include".into());
        }
        for constant in &self.constants {
            crate::check::validate_extern_constant(constant)?;
            if !symbols.insert(&constant.name) {
                return Err("E-TYPED-FFI: duplicate native symbol".into());
            }
        }
        Ok(())
    }

    pub fn function_id(&self, name: &str) -> Option<ForeignFunctionId> {
        self.functions
            .iter()
            .position(|f| f.name == name)
            .and_then(|id| u32::try_from(id).ok())
            .map(ForeignFunctionId)
    }
    pub fn constant_id(&self, name: &str) -> Option<ForeignConstantId> {
        self.constants
            .iter()
            .position(|c| c.name == name)
            .and_then(|id| u32::try_from(id).ok())
            .map(ForeignConstantId)
    }
}

impl NativeManifest {
    pub(super) fn validate_type(&self, ty: Type) -> Result<(), String> {
        if let Type::RawPointer(id) = ty {
            self.pointers
                .get(id.0 as usize)
                .ok_or("E-TYPED-FFI: invalid pointer identity")?;
        }
        Ok(())
    }
    pub(super) fn runtime_type(&self, ty: &ExternType) -> Option<Type> {
        match ty {
            ExternType::I32 => Some(Type::Int),
            ExternType::Bool => Some(Type::Bool),
            ExternType::Unit => Some(Type::Unit),
            ExternType::U8 => Some(Type::CU8),
            ExternType::U32 => Some(Type::CU32),
            ExternType::U64 => Some(Type::CU64),
            ExternType::F32 => Some(Type::CF32),
            ExternType::Pointer { pointee, is_const } => self
                .pointers
                .iter()
                .position(|p| p.pointee == *pointee && p.is_const == *is_const)
                .and_then(|id| u32::try_from(id).ok())
                .map(|id| Type::RawPointer(PointerTypeId(id))),
            ExternType::CStr => None,
        }
    }
}

impl Lower<'_, '_> {
    pub(super) fn foreign_call(
        &mut self,
        id: ForeignFunctionId,
        args: &[Expr],
    ) -> Result<ExprKind, String> {
        let function = self.native.functions[id.0 as usize].clone();
        if args.len() != function.params.len() {
            return Err("E-TYPED-FFI: wrong native call arity".into());
        }
        if self.native.runtime_type(&function.result).is_none() {
            return Err("E-TYPED-FFI: invalid native runtime type".into());
        }
        let mut lowered = Vec::new();
        for (arg, param) in args.iter().zip(&function.params) {
            if param.ty == ExternType::CStr {
                let Expr::CStr(value) = arg else {
                    return Err("E-TYPED-FFI: expected a direct static C string".into());
                };
                check_cstr(value)?;
                lowered.push(ForeignArg::StaticCStr(value.clone()));
            } else {
                if self.native.runtime_type(&param.ty).is_none() {
                    return Err("E-TYPED-FFI: invalid native runtime type".into());
                }
                lowered.push(ForeignArg::Value(self.expr(arg)?));
            }
        }
        Ok(ExprKind::ForeignCall(id, lowered))
    }
}

fn check_cstr(value: &str) -> Result<(), String> {
    if value.as_bytes().contains(&0) {
        return Err("E-TYPED-FFI: static C string contains NUL".into());
    }
    Ok(())
}

pub(super) fn verify_call(
    id: ForeignFunctionId,
    args: &[ForeignArg],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    context: &VerifyContext<'_>,
) -> Result<Type, String> {
    let function = context
        .native
        .functions
        .get(id.0 as usize)
        .ok_or("E-TYPED-FFI: invalid native function identity")?;
    if args.len() != function.params.len() {
        return Err("E-TYPED-FFI: wrong native call arity".into());
    }
    for (arg, param) in args.iter().zip(&function.params) {
        match (arg, &param.ty) {
            (ForeignArg::StaticCStr(value), ExternType::CStr) => check_cstr(value)?,
            (ForeignArg::Value(value), expected) if *expected != ExternType::CStr => {
                let expected = context
                    .native
                    .runtime_type(expected)
                    .ok_or("E-TYPED-FFI: unsupported native operand type")?;
                // Do not reset scope: enclosing Rill argument loans remain live.
                if verify_expr(value, scope, ids, context)? != expected {
                    return Err("E-TYPED-FFI: wrong native operand type".into());
                }
            }
            _ => return Err("E-TYPED-FFI: wrong native operand kind".into()),
        }
    }
    context
        .native
        .runtime_type(&function.result)
        .ok_or_else(|| "E-TYPED-FFI: unsupported native result type".into())
}

pub(super) fn verify_constant(
    id: ForeignConstantId,
    native: &NativeManifest,
) -> Result<Type, String> {
    let constant = native
        .constants
        .get(id.0 as usize)
        .ok_or("E-TYPED-FFI: invalid native constant identity")?;
    native
        .runtime_type(&constant.ty)
        .ok_or_else(|| "E-TYPED-FFI: unsupported native constant type".into())
}
