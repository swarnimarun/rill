//! Private alternate entry contract for explicit scalar C exports.
use std::collections::HashSet;

use super::{ExprKind, ResultRepr, Type, TypedMain};
use crate::ast::ParamMode;

pub(super) struct FunctionExport {
    pub function: usize,
    pub c_name: String,
}

pub(super) struct Library {
    pub program: TypedMain,
    pub exports: Vec<FunctionExport>,
}

pub(crate) fn emit_i32_library(
    checked: &crate::check::CheckedProgram<'_>,
    exports: &[crate::abi::Export],
) -> Result<String, String> {
    super::c::emit_i32_library(&lower_library(checked, exports)?)
}

pub(crate) fn emit_copy_library(
    checked: &crate::check::CheckedProgram<'_>,
    exports: &[crate::abi::Export],
) -> Result<(String, Vec<crate::abi::copy::Function>), String> {
    let library = lower_library(checked, exports)?;
    let functions = copy_functions(&library)?;
    let c = super::c::emit_copy_library(&library)?;
    Ok((c, functions))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn emit_copy_reload(
    checked: &crate::check::CheckedProgram<'_>,
    exports: &[crate::abi::Export],
    build_context: &[u8],
) -> Result<(String, Vec<crate::abi::copy::Function>, Vec<u8>), String> {
    let library = lower_library(checked, exports)?;
    let functions = copy_functions(&library)?;
    let manifest = crate::abi::copy::manifest(&functions)?;
    let (c, identity) = super::c::emit_copy_reload(&library, &manifest, build_context)?;
    Ok((c, functions, identity))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn emit_i32_reload(
    checked: &crate::check::CheckedProgram<'_>,
    exports: &[crate::abi::Export],
    manifest: &str,
    build_context: &[u8],
) -> Result<(String, Vec<u8>), String> {
    super::c::emit_i32_reload(&lower_library(checked, exports)?, manifest, build_context)
}

fn lower_library(
    checked: &crate::check::CheckedProgram<'_>,
    exports: &[crate::abi::Export],
) -> Result<Library, String> {
    let (program, instances) = super::lower_rooted(checked, None, None, Some(exports))?;
    let exports = exports
        .iter()
        .map(|export| {
            let signature = &checked.functions[&export.source];
            Ok(FunctionExport {
                // Source order and recursive discoveries determine final IDs.
                // Never infer these from the public export ordering or names.
                function: instances.call(
                    &export.source,
                    signature.params.clone(),
                    signature.result.clone(),
                )?,
                c_name: export.c_name.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Library { program, exports })
}

pub(super) fn verify(library: &Library) -> Result<(), String> {
    verify_boundary(library, false)
}

pub(super) fn verify_copy(library: &Library) -> Result<(), String> {
    verify_boundary(library, true)?;
    for function in copy_functions_unverified(library)? {
        function.validate()?;
    }
    Ok(())
}

fn verify_boundary(library: &Library, copy: bool) -> Result<(), String> {
    let program = &library.program;
    let forbidden_type = |ty: &Type| ty.is_owner() || (ty.is_raw() && !(copy && *ty == Type::CF32));
    // The logical/control proof currently binds one source main and scope 0.
    // A library cannot reinterpret that proof as evidence for several roots.
    let logical = &program.logical;
    if logical.control.is_some()
        || logical.domain.is_some()
        || !logical.declarations.is_empty()
        || !logical.source_functions.is_empty()
        || !logical.scope_sources.is_empty()
        || !logical.function_scopes.is_empty()
        || !logical.binders.is_empty()
        || program.adts.iter().any(|adt| adt.symbolic.is_some())
        || super::has_control_extension(program)
    {
        return Err("E-ABI: symbolic logical and control representations are unsupported".into());
    }
    if program.native.config != crate::c_import::Config::default()
        || program.native.has_foreign()
        || program.allocation_contract.is_some()
        || program.adts.iter().any(|adt| {
            adt.owned
                || adt
                    .arguments
                    .iter()
                    .chain(adt.variants.iter().flat_map(|variant| &variant.fields))
                    .any(forbidden_type)
        })
        || program.functions.iter().any(|function| {
            matches!(function.result, ResultRepr::Value(ty) if forbidden_type(&ty))
                || function
                    .params
                    .iter()
                    .any(|(_, ty, _)| forbidden_type(ty))
                || super::any_computation(&function.body, &|kind| {
                    matches!(
                        kind,
                        ExprKind::TryAlloc { .. }
                            | ExprKind::OwnerLet { .. }
                            | ExprKind::OwnerRead(_)
                            | ExprKind::OwnerWrite(..)
                            | ExprKind::OwnerMove(_)
                            | ExprKind::OwnerBytesLen(_)
                            | ExprKind::OwnerBytesGet(..)
                            | ExprKind::OwnerBytesSet(..)
                            | ExprKind::ForeignCall(..)
                            | ExprKind::ForeignConst(_)
                            | ExprKind::NullPointer
                            | ExprKind::IsNull(_)
                    )
                        || matches!(kind, ExprKind::ForeignConvert(conversion, _) if !(copy && *conversion == super::ForeignConvert::F32))
                })
        })
    {
        return Err("E-ABI: owners and native values are unsupported".into());
    }
    // This changes only the external entry contract. Every signature, body,
    // operand, local identity, loan and cleanup still takes the ordinary IR
    // verifier, including helpers that are not themselves public endpoints.
    super::verify_with_entry(program, |program| {
        if library.exports.is_empty() || library.exports.len() > 64 {
            return Err("E-ABI: expected 1 to 64 exports".into());
        }
        if program.entry != library.exports[0].function {
            return Err("E-ABI: invalid first library root".into());
        }
        let mut names = HashSet::new();
        let mut functions = HashSet::new();
        for export in &library.exports {
            crate::abi::validate_c_name(&export.c_name)?;
            if !names.insert(export.c_name.as_str()) || !functions.insert(export.function) {
                return Err("E-ABI: duplicate library export".into());
            }
            let function = program
                .functions
                .get(export.function)
                .ok_or("E-ABI: invalid library function identity")?;
            if function.params.len() > 32
                || function
                    .params
                    .iter()
                    .any(|(_, ty, mode)| *mode != ParamMode::Value || (!copy && *ty != Type::Int))
                || if copy {
                    !matches!(function.result, ResultRepr::Value(_))
                } else {
                    function.result != ResultRepr::Value(Type::Int)
                }
            {
                return Err(if copy {
                    "E-COPY-ABI: invalid typed Copy export signature"
                } else {
                    "E-ABI: invalid typed integer export signature"
                }
                .into());
            }
        }
        Ok(())
    })
}

pub(super) fn copy_functions(library: &Library) -> Result<Vec<crate::abi::copy::Function>, String> {
    verify_copy(library)?;
    copy_functions_unverified(library)
}

fn stable_name(name: &str) -> String {
    // The module loader's generated prefix contains a compilation-local ID.
    // The wire protocol uses structural shapes and source leaf names. It does
    // not assert that equal shapes from different Rill modules are nominally
    // interchangeable inside Rill itself.
    if let Some(rest) = name.strip_prefix("M$") {
        if let Some((id, leaf)) = rest.split_once('$') {
            if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && !leaf.is_empty() {
                return leaf.into();
            }
        }
    }
    name.into()
}

fn copy_functions_unverified(library: &Library) -> Result<Vec<crate::abi::copy::Function>, String> {
    use crate::abi::copy::{Field, Function, Schema, Variant, MAX_DEPTH, MAX_NODES};
    fn schema(
        program: &TypedMain,
        ty: Type,
        depth: usize,
        nodes: &mut usize,
        active: &mut HashSet<usize>,
    ) -> Result<Schema, String> {
        if depth > MAX_DEPTH || *nodes >= MAX_NODES {
            return Err("E-COPY-ABI: schema exceeds depth or node limit".into());
        }
        *nodes += 1;
        Ok(match ty {
            Type::Int => Schema::I32,
            Type::Bool => Schema::Bool,
            Type::CF32 => Schema::F32,
            Type::Adt { id, owned: false } => {
                if !active.insert(id) {
                    return Err("E-COPY-ABI: recursive state storage is unsupported".into());
                }
                let adt = program
                    .adts
                    .get(id)
                    .ok_or("E-COPY-ABI: unknown aggregate storage")?;
                if adt.symbolic.is_some() || adt.origin != super::AdtOrigin::Source {
                    return Err(
                        "E-COPY-ABI: symbolic or generated state storage is unsupported".into(),
                    );
                }
                let result = if let Some(names) = &adt.record_fields {
                    Schema::Record {
                        name: stable_name(&adt.name),
                        fields: names
                            .iter()
                            .zip(&adt.variants[0].fields)
                            .map(|(name, ty)| {
                                Ok(Field {
                                    name: name.clone(),
                                    schema: schema(program, *ty, depth + 1, nodes, active)?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                    }
                } else {
                    *nodes = nodes
                        .checked_add(adt.variants.len())
                        .ok_or("E-COPY-ABI: schema node count overflow")?;
                    if *nodes > MAX_NODES {
                        return Err("E-COPY-ABI: schema exceeds node limit".into());
                    }
                    Schema::Enum {
                        name: stable_name(&adt.name),
                        variants: adt
                            .variants
                            .iter()
                            .map(|variant| {
                                Ok(Variant {
                                    name: stable_name(&variant.name),
                                    fields: variant
                                        .fields
                                        .iter()
                                        .map(|ty| schema(program, *ty, depth + 1, nodes, active))
                                        .collect::<Result<_, String>>()?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                    }
                };
                active.remove(&id);
                result
            }
            _ => {
                return Err(
                    "E-COPY-ABI: state requires i32, bool, f32 or finite closed records/enums"
                        .into(),
                )
            }
        })
    }
    library
        .exports
        .iter()
        .map(|export| {
            let function = &library.program.functions[export.function];
            let ResultRepr::Value(result) = function.result else {
                return Err("E-COPY-ABI: export has no returning value storage".into());
            };
            let make_schema = |ty| schema(&library.program, ty, 0, &mut 0, &mut HashSet::new());
            Ok(Function {
                name: export.c_name.clone(),
                parameters: function
                    .params
                    .iter()
                    .map(|(_, ty, _)| make_schema(*ty))
                    .collect::<Result<_, _>>()?,
                result: make_schema(result)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Library {
        let program = crate::parser::parse("fn identity(x: i32) -> i32 { x }").unwrap();
        let checked = crate::check::analyze_library(&program).unwrap();
        let exports = [crate::abi::Export {
            source: "identity".into(),
            c_name: "rill_identity".into(),
        }];
        let (program, _) =
            super::super::lower_rooted(&checked, None, None, Some(&exports)).unwrap();
        Library {
            program,
            exports: vec![FunctionExport {
                function: 0,
                c_name: "rill_identity".into(),
            }],
        }
    }

    #[test]
    fn library_entry_contract_does_not_relax_executable_main() {
        let library = fixture();
        verify(&library).unwrap();
        assert_eq!(
            super::super::verify(&library.program).unwrap_err(),
            "E-TYPED: main has parameters"
        );
        assert!(super::super::emit_c(&library.program).is_err());
    }

    #[test]
    fn independent_library_verifier_checks_ids_signatures_and_bodies() {
        let mut library = fixture();
        library.exports[0].function = usize::MAX;
        assert!(verify(&library).is_err());
        let mut library = fixture();
        library.program.functions[0].params[0].2 = ParamMode::Read;
        assert!(verify(&library)
            .unwrap_err()
            .contains("integer export signature"));
        let mut library = fixture();
        library.program.functions[0].body =
            super::super::Computation::Value(super::super::ExprNode {
                ty: Type::Int,
                kind: ExprKind::Bool(true),
            });
        assert!(verify(&library).is_err());
        let mut library = fixture();
        library.program.logical.source_functions.push("main".into());
        assert!(verify(&library).unwrap_err().contains("symbolic logical"));
    }

    #[test]
    fn copy_entry_admission_preserves_scalar_and_executable_contracts() {
        let source = "type State={x:i32;ready:bool;speed:f32} fn update(state:State)->State{state}";
        let program = crate::parser::parse(source).unwrap();
        let checked = crate::check::analyze_library(&program).unwrap();
        let exports = [crate::abi::Export {
            source: "update".into(),
            c_name: "rill_update".into(),
        }];
        let library = lower_library(&checked, &exports).unwrap();
        verify_copy(&library).unwrap();
        assert!(verify(&library).is_err());
        assert!(super::super::verify(&library.program).is_err());
        assert!(super::super::emit_c(&library.program).is_err());
        let mut library = lower_library(&checked, &exports).unwrap();
        library.program.adts[0].variants[0].fields[2] = Type::CU32;
        assert!(verify_copy(&library).is_err());
        let mut library = lower_library(&checked, &exports).unwrap();
        library.program.functions[0].params[0].2 = ParamMode::Read;
        assert!(verify_copy(&library).is_err());
        let mut library = lower_library(&checked, &exports).unwrap();
        library.program.adts[0].record_fields.as_mut().unwrap()[1] = "x".into();
        assert!(verify_copy(&library).is_err());
    }
}
