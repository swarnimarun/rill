//! Whole-source raw-type admission for explicit pure-library boundaries.
//!
//! This deliberately leaves `has_c_values` and ordinary backend routing alone.
//! Inspect borrowed type trees and each declaration schema once, without
//! substituting fields or expanding a potentially large nominal layout.
use super::{CheckedProgram, Type};
use crate::ast::TypeExpr;
use std::collections::HashSet;

const WORK: usize = 4_000_000;
const LIMIT: &str = "E-LIBRARY-TYPE-COMPLEXITY: raw type scan exceeds 4000000 nodes";

impl CheckedProgram<'_> {
    /// Include dormant parameters, phantom arguments, callable environments and
    /// unused declaration fields, rather than only executed raw expressions.
    pub(crate) fn has_raw_source_types(&self) -> Result<bool, String> {
        let mut scan = Scan {
            remaining: WORK,
            raw: false,
        };
        for function in &self.program.functions {
            scan.reserve(1)?;
            let signature = &self.functions[&function.name];
            for ty in signature
                .params
                .iter()
                .chain(std::iter::once(&signature.result))
            {
                scan.checked_type(ty)?;
            }
        }
        for declaration in &self.program.types {
            scan.reserve(1)?;
            scan.reserve(declaration.params.len())?;
            scan.reserve(declaration.variants.len())?;
            let bindings: HashSet<_> = declaration.params.iter().map(String::as_str).collect();
            for variant in &declaration.variants {
                for field in &variant.fields {
                    scan.source_type(field, &bindings)?;
                }
            }
        }
        // These maps have no source order. Accumulate and visit every tree,
        // including after finding a raw atom, so hash iteration cannot change
        // whether the complete shared budget succeeds or reports its limit.
        for ty in self
            .expression_types
            .values()
            .chain(self.application_types.values())
        {
            scan.checked_type(ty)?;
        }
        Ok(scan.raw)
    }
}

struct Scan {
    remaining: usize,
    raw: bool,
}

impl Scan {
    fn reserve(&mut self, nodes: usize) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(nodes).ok_or(LIMIT)?;
        Ok(())
    }

    fn checked_type(&mut self, root: &Type) -> Result<(), String> {
        self.reserve(1)?;
        let mut pending = vec![root];
        while let Some(ty) = pending.pop() {
            match ty {
                Type::CStr
                | Type::CU8
                | Type::CU32
                | Type::CU64
                | Type::CF32
                | Type::CPointer { .. } => self.raw = true,
                Type::Nominal(_, arguments) => {
                    self.reserve(arguments.len())?;
                    pending.extend(arguments);
                }
                Type::Callable {
                    environment,
                    params,
                    result,
                    ..
                } => {
                    self.reserve(params.len().checked_add(2).ok_or(LIMIT)?)?;
                    pending.push(environment);
                    pending.extend(params);
                    pending.push(result);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn source_type(&mut self, root: &TypeExpr, bindings: &HashSet<&str>) -> Result<(), String> {
        self.reserve(1)?;
        let mut pending = vec![root];
        while let Some(ty) = pending.pop() {
            // Match declared_type: internal cstr precedes binder lookup, while
            // ordinary binders can shadow primitive and pointer type names.
            if ty.name == "$cstr" && ty.args.is_empty() {
                self.raw = true;
            } else if bindings.contains(ty.name.as_str()) {
                continue;
            } else if matches!(
                ty.name.as_str(),
                "u8" | "u32" | "u64" | "f32" | "ptr" | "const_ptr"
            ) {
                self.raw = true;
            }
            self.reserve(ty.args.len())?;
            pending.extend(&ty.args);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduling_budget_is_shared_and_raw_detection_does_not_skip_work() {
        let tree = Type::Nominal("Pair".into(), vec![Type::CU32, Type::Int]);
        let mut too_small = Scan {
            remaining: 2,
            raw: false,
        };
        assert_eq!(too_small.checked_type(&tree).unwrap_err(), LIMIT);
        assert!(!too_small.raw);
        let mut exact = Scan {
            remaining: 3,
            raw: false,
        };
        exact.checked_type(&tree).unwrap();
        assert!(exact.raw);
        assert_eq!(exact.remaining, 0);
        assert_eq!(exact.checked_type(&Type::Int).unwrap_err(), LIMIT);
    }

    #[test]
    fn schema_bindings_shadow_primitives_but_not_internal_cstr() {
        let bindings = HashSet::from(["u32", "ptr", "$cstr"]);
        let mut scan = Scan {
            remaining: 3,
            raw: false,
        };
        for name in ["u32", "ptr"] {
            scan.source_type(
                &TypeExpr {
                    name: name.into(),
                    args: vec![],
                },
                &bindings,
            )
            .unwrap();
        }
        assert!(!scan.raw);
        scan.source_type(
            &TypeExpr {
                name: "$cstr".into(),
                args: vec![],
            },
            &bindings,
        )
        .unwrap();
        assert!(scan.raw);
    }
}
