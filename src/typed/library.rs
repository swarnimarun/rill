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
    super::c::emit_i32_library(&Library { program, exports })
}

pub(super) fn verify(library: &Library) -> Result<(), String> {
    let program = &library.program;
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
                    .any(|ty| ty.is_owner() || ty.is_raw())
        })
        || program.functions.iter().any(|function| {
            matches!(function.result, ResultRepr::Value(ty) if ty.is_owner() || ty.is_raw())
                || function
                    .params
                    .iter()
                    .any(|(_, ty, _)| ty.is_owner() || ty.is_raw())
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
                            | ExprKind::ForeignConvert(..)
                            | ExprKind::NullPointer
                            | ExprKind::IsNull(_)
                    )
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
            if function.result != ResultRepr::Value(Type::Int)
                || function.params.len() > 32
                || function
                    .params
                    .iter()
                    .any(|(_, ty, mode)| *ty != Type::Int || *mode != ParamMode::Value)
            {
                return Err("E-ABI: invalid typed integer export signature".into());
            }
        }
        Ok(())
    })
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
}
