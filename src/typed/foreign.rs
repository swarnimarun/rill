//! Opaque native declarations and the first checked scalar-call increment.
//! This manifest retains all source declarations, not only reachable operations.
//! It asserts no effects, pointer lifetimes, ownership or target compatibility.
use super::*;
use crate::ast::{ExternConst, ExternFunction, ExternType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ForeignFunctionId(pub(super) u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ForeignConstantId(pub(super) u32);

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
}

impl NativeManifest {
    pub fn from_checked(checked: &CheckedProgram<'_>) -> Result<Self, String> {
        let manifest = Self {
            config: checked.program.native.clone(),
            functions: checked.program.externs.clone(),
            constants: checked.program.constants.clone(),
        };
        manifest.verify()?;
        Ok(manifest)
    }

    pub fn has_foreign(&self) -> bool {
        !self.config.includes.is_empty() || !self.functions.is_empty() || !self.constants.is_empty()
    }

    pub fn verify(&self) -> Result<(), String> {
        self.config.validate()?;
        u32::try_from(self.functions.len()).map_err(|_| "E-TYPED-FFI: too many functions")?;
        u32::try_from(self.constants.len()).map_err(|_| "E-TYPED-FFI: too many constants")?;
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

/// F1 runtime mappings only. Other ABI declarations remain in the manifest,
/// including their header assertions, but their operations wait for F2.
fn runtime_type(ty: &ExternType) -> Option<Type> {
    match ty {
        ExternType::I32 => Some(Type::Int),
        ExternType::Bool => Some(Type::Bool),
        ExternType::Unit => Some(Type::Unit),
        _ => None,
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
        if runtime_type(&function.result).is_none() {
            return Err("E-TYPED-UNSUPPORTED: native runtime type awaits foreign transport".into());
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
                if runtime_type(&param.ty).is_none() {
                    return Err(
                        "E-TYPED-UNSUPPORTED: native runtime type awaits foreign transport".into(),
                    );
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
                let expected =
                    runtime_type(expected).ok_or("E-TYPED-FFI: unsupported native operand type")?;
                // Do not reset scope: enclosing Rill argument loans remain live.
                if verify_expr(value, scope, ids, context)? != expected {
                    return Err("E-TYPED-FFI: wrong native operand type".into());
                }
            }
            _ => return Err("E-TYPED-FFI: wrong native operand kind".into()),
        }
    }
    runtime_type(&function.result)
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
    runtime_type(&constant.ty).ok_or_else(|| "E-TYPED-FFI: unsupported native constant type".into())
}
