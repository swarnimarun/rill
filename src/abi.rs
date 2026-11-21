//! Explicit, experimental C11 exports for closed integer functions.
//!
//! This emits source artifacts, not a shared-library loader or a live-state
//! migration protocol. `export fn` continues to mean module visibility.

use std::collections::HashSet;
use std::fmt::Write;

use crate::ast::{ParamMode, Program};
use crate::check::Type;

/// A checked source function and its explicit external C symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub source: String,
    pub c_name: String,
}

/// Self-contained C11 source, public declarations and an ABI version 1 manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub c: String,
    pub header: String,
    pub manifest: String,
}

/// Check every declaration, then emit explicitly selected integer functions.
///
/// Each endpoint must have a closed monomorphic signature consisting only of
/// value-mode `i32` parameters and an `i32` result. There may be 1–64 exports,
/// each with at most 32 arguments. This subset rejects an executable `main`,
/// native dependencies/declarations, owners, raw C values, callables and
/// unsupported symbolic/control representations. It never evaluates a function
/// to discover its signature.
///
/// The generated functions use the target compiler's C11 `int32_t` calling
/// convention. Arithmetic and call-depth failures print `rill: runtime trap`
/// and terminate the process with status 1. The call-depth limit is 32 Rill
/// frames per thread; repeated or concurrent host calls retain no Rill state.
pub fn emit_i32_library(program: &Program, exports: &[Export]) -> Result<Artifact, String> {
    // Do this first: selecting a valid endpoint must not hide a bad declaration.
    let checked = crate::check::analyze_library(program)?;
    if program
        .functions
        .iter()
        .any(|function| function.name == "main")
    {
        return Err("E-ABI: a library cannot declare executable main".into());
    }
    if program.native != crate::c_import::Config::default()
        || !program.externs.is_empty()
        || !program.constants.is_empty()
        || checked.has_c_values()
        || checked.has_raw_source_types()?
    {
        return Err(
            "E-ABI: native dependencies, declarations and raw C values are unsupported".into(),
        );
    }
    if checked.has_owned() || program.allocation_types.is_some() {
        return Err("E-ABI: owner values and allocation contracts are unsupported".into());
    }
    if crate::functions::needs_lowering(program) {
        return Err("E-ABI: callable values are unsupported".into());
    }
    if exports.is_empty() || exports.len() > 64 {
        return Err("E-ABI: expected 1 to 64 exports".into());
    }
    let mut sources = HashSet::new();
    let mut names = HashSet::new();
    let mut manifest_exports = Vec::with_capacity(exports.len());
    let mut header = String::from(
        "/* Experimental Rill scalar C ABI. */\n#include <stdint.h>\n\
         #ifndef RILL_I32_ABI_VERSION\n#define RILL_I32_ABI_VERSION 1\n\
         #elif RILL_I32_ABI_VERSION != 1\n#error \"incompatible Rill scalar ABI version\"\n#endif\n\
         #ifdef __cplusplus\nextern \"C\" {\n#endif\n",
    );
    // Function prototypes may safely be repeated. Avoid a common whole-header
    // guard, which would hide declarations when a host includes two libraries.
    for export in exports {
        validate_c_name(&export.c_name)?;
        if !sources.insert(export.source.as_str()) {
            return Err(format!("E-ABI: duplicate source export {}", export.source));
        }
        if !names.insert(export.c_name.as_str()) {
            return Err(format!("E-ABI: duplicate C export {}", export.c_name));
        }
        let function = program
            .functions
            .iter()
            .find(|function| function.name == export.source)
            .ok_or_else(|| format!("E-ABI: unknown source function {}", export.source))?;
        let signature = &checked.functions[&export.source];
        if !signature.quantified.is_empty()
            || signature.result != Type::Int
            || signature.params.iter().any(|ty| *ty != Type::Int)
            || function
                .params
                .iter()
                .any(|param| param.mode != ParamMode::Value)
        {
            return Err(format!(
                "E-ABI: {} requires a closed monomorphic value-i32 signature",
                export.source
            ));
        }
        if function.params.len() > 32 {
            return Err(format!("E-ABI: {} exceeds 32 arguments", export.source));
        }
        write!(header, "int32_t {}(", export.c_name).unwrap();
        if function.params.is_empty() {
            header.push_str("void");
        }
        for ordinal in 0..function.params.len() {
            if ordinal != 0 {
                header.push_str(", ");
            }
            write!(header, "int32_t arg{ordinal}").unwrap();
        }
        header.push_str(");\n");
        manifest_exports.push(serde_json::json!({
            "name": export.c_name,
            "arity": function.params.len(),
            "parameters": vec!["i32"; function.params.len()],
            "result": "i32",
        }));
    }
    header.push_str("#ifdef __cplusplus\n}\n#endif\n");
    let c = crate::typed::emit_i32_library(&checked, exports)?;
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "abi": "rill-c-i32",
        "exports": manifest_exports,
    }))
    .map_err(|error| format!("E-ABI: cannot serialize manifest: {error}"))?
        + "\n";
    Ok(Artifact {
        c,
        header,
        manifest,
    })
}

pub(crate) fn validate_c_name(name: &str) -> Result<(), String> {
    // The compiler's private C names use rl_. rill_ is the explicit public
    // namespace, and cannot overlap C keywords or leading-underscore reserves.
    if !name.starts_with("rill_")
        || name.len() <= "rill_".len()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || name.starts_with("rill_abi_")
    {
        return Err(format!(
            "E-ABI: invalid C export name {name:?}; expected a nonreserved rill_ identifier of at most 128 ASCII bytes"
        ));
    }
    Ok(())
}
