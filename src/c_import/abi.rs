//! C spellings and target-compiler assertions shared by checked emitters.
//! These helpers format already validated declarations. They neither validate
//! source programs nor establish foreign effects, ownership or lifetimes.
use super::Config;
use crate::ast::{ExternConst, ExternFunction, ExternType};
use std::fmt::Write;

pub(crate) fn c_type(ty: &ExternType) -> String {
    match ty {
        ExternType::I32 => "int32_t".into(),
        ExternType::Bool => "_Bool".into(),
        ExternType::Unit => "void".into(),
        ExternType::CStr => "const char *".into(),
        ExternType::U8 => "uint8_t".into(),
        ExternType::U32 => "uint32_t".into(),
        ExternType::U64 => "uint64_t".into(),
        ExternType::F32 => "float".into(),
        ExternType::Pointer { pointee, is_const } => format!(
            "{}{} *",
            if *is_const { "const " } else { "" },
            match pointee.as_str() {
                "i32" => "int32_t",
                "u8" => "uint8_t",
                "u32" => "uint32_t",
                "u64" => "uint64_t",
                "f32" => "float",
                "bool" => "_Bool",
                _ => pointee,
            }
        ),
    }
}

/// Emit every function, then every constant, preserving each declaration list's
/// order and retaining unused entries.
/// Callers emit the config's includes and the standard integer headers first.
/// Header compatibility remains a question for the selected target compiler;
/// absent headers, an extern prototype is only the programmer's ABI assertion.
pub(crate) fn emit_declarations(
    config: &Config,
    functions: &[ExternFunction],
    constants: &[ExternConst],
    out: &mut String,
) {
    for function in functions {
        let result = c_type(&function.result);
        let params = if function.params.is_empty() {
            "void".into()
        } else {
            function
                .params
                .iter()
                .map(|p| c_type(&p.ty))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if config.includes.is_empty() {
            writeln!(out, "extern {result} {}({params});", function.name).unwrap();
        } else {
            writeln!(out, "_Static_assert(__builtin_types_compatible_p(__typeof__(&{}), {result} (*)({params})), \"E-FFI: incompatible header prototype for {}\");", function.name, function.name).unwrap();
        }
    }
    for constant in constants {
        let n = &constant.name;
        let c = c_type(&constant.ty);
        let condition = match constant.ty {
            ExternType::U8 => format!("__builtin_classify_type({n}) == 1 && ({n}) >= 0 && ({n}) <= UINT8_MAX"),
            ExternType::U32 => format!("__builtin_classify_type({n}) == 1 && ({n}) >= 0 && ({n}) <= UINT32_MAX"),
            ExternType::U64 => format!("__builtin_classify_type({n}) == 1 && ({n}) >= 0 && ({n}) <= UINT64_MAX"),
            ExternType::I32 => format!("__builtin_classify_type({n}) == 1 && (({n}) >= 0 ? ({n}) <= INT32_MAX : ({n}) >= INT32_MIN)"),
            _ => format!("__builtin_types_compatible_p(__typeof__({n}), {c})"),
        };
        writeln!(
            out,
            "_Static_assert({condition}, \"E-FFI: incompatible header constant {n}\");"
        )
        .unwrap();
    }
}
