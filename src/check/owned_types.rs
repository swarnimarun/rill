//! Concrete nominal field metadata shared with affine lowering.
use super::{CheckedProgram, Type};
use crate::ast::TypeExpr;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct CheckedVariant {
    pub name: String,
    pub fields: Vec<Type>,
}

impl CheckedProgram<'_> {
    /// Resolve declaration fields at a concrete nominal instantiation.
    pub fn adt_variants(&self, ty: &Type) -> Result<Vec<CheckedVariant>, String> {
        if let Some(variants) = self
            .adt_variant_cache
            .lock()
            .map_err(|_| "E-OWNED-TYPE: metadata cache poisoned")?
            .get(ty)
            .cloned()
        {
            return Ok(variants);
        }
        let Type::Nominal(name, arguments) = ty else {
            return Err("E-OWNED-TYPE: expected a nominal enum type".into());
        };
        let declaration = self
            .program
            .types
            .iter()
            .find(|decl| decl.name == *name)
            .ok_or_else(|| format!("E-OWNED-TYPE: unknown nominal type '{name}'"))?;
        if declaration.params.len() != arguments.len() {
            return Err("E-OWNED-TYPE: nominal type argument count differs".into());
        }
        for argument in arguments {
            require_concrete(argument)?;
        }
        let variables: HashMap<_, _> = declaration
            .params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let variants: Vec<CheckedVariant> = declaration
            .variants
            .iter()
            .map(|variant| {
                Ok(CheckedVariant {
                    name: variant.name.clone(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|field| self.concrete_field(field, &variables))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, String>>()?;
        self.adt_variant_cache
            .lock()
            .map_err(|_| "E-OWNED-TYPE: metadata cache poisoned")?
            .insert(ty.clone(), variants.clone());
        Ok(variants)
    }

    pub fn is_record_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal(name, _) if self.program.types.iter().any(|decl| decl.name == *name && decl.record_fields.is_some()))
    }

    /// True when a concrete value contains a unique heap owner in any variant.
    pub fn type_is_owner(&self, ty: &Type) -> Result<bool, String> {
        self.owner_at_depth(ty, 0, &mut HashSet::new())
    }

    fn owner_at_depth(
        &self,
        ty: &Type,
        depth: usize,
        active: &mut HashSet<Type>,
    ) -> Result<bool, String> {
        if let Some(owned) = self
            .owned_type_cache
            .lock()
            .map_err(|_| "E-OWNED-TYPE: metadata cache poisoned")?
            .get(ty)
            .copied()
        {
            return Ok(owned);
        }
        if depth > 128 {
            return Err("E-OWNED-TYPE: nominal ownership nesting exceeds 128".into());
        }
        if !active.insert(ty.clone()) {
            return Err("E-OWNED-TYPE: recursive inline ownership layout".into());
        }
        let result = match ty {
            Type::SystemBoxI32 | Type::SystemBytes => Ok(true),
            Type::Nominal(_, _) => {
                let mut owned = false;
                for variant in self.adt_variants(ty)? {
                    for field in variant.fields {
                        owned |= self.owner_at_depth(&field, depth + 1, active)?;
                    }
                }
                Ok(owned)
            }
            Type::Var(_) => Err("E-OWNED-TYPE: unresolved concrete type instance".into()),
            _ => Ok(false),
        };
        active.remove(ty);
        if let Ok(owned) = result {
            self.owned_type_cache
                .lock()
                .map_err(|_| "E-OWNED-TYPE: metadata cache poisoned")?
                .insert(ty.clone(), owned);
        }
        result
    }

    fn concrete_field(
        &self,
        expression: &TypeExpr,
        variables: &HashMap<String, Type>,
    ) -> Result<Type, String> {
        if let Some(ty) = variables.get(&expression.name) {
            if !expression.args.is_empty() {
                return Err("E-OWNED-TYPE: type variable has arguments".into());
            }
            return Ok(ty.clone());
        }
        if matches!(expression.name.as_str(), "ptr" | "const_ptr") {
            if expression.args.len() != 1 || !expression.args[0].args.is_empty() {
                return Err("E-FFI: pointer types require one C pointee name".into());
            }
            return Ok(Type::CPointer {
                pointee: expression.args[0].name.clone(),
                is_const: expression.name == "const_ptr",
            });
        }
        let primitive = match expression.name.as_str() {
            "u8" => Some(Type::CU8),
            "u32" => Some(Type::CU32),
            "u64" => Some(Type::CU64),
            "f32" => Some(Type::CF32),
            "i32" => Some(Type::Int),
            "bool" => Some(Type::Bool),
            "unit" => Some(Type::Unit),
            "str" => Some(Type::StaticStr),
            "u8x16" => Some(Type::U8x16),
            "SystemBoxI32" => Some(Type::SystemBoxI32),
            "SystemBytes" => Some(Type::SystemBytes),
            _ => None,
        };
        if let Some(ty) = primitive {
            if !expression.args.is_empty() {
                return Err("E-OWNED-TYPE: primitive type has arguments".into());
            }
            return Ok(ty);
        }
        let arguments = expression
            .args
            .iter()
            .map(|argument| self.concrete_field(argument, variables))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = Type::Nominal(expression.name.clone(), arguments);
        // Validate nominal identity and arity even for phantom type arguments.
        let declaration = self
            .program
            .types
            .iter()
            .find(|decl| decl.name == expression.name)
            .ok_or_else(|| format!("E-OWNED-TYPE: unknown field type '{}'", expression.name))?;
        if declaration.params.len() != expression.args.len() {
            return Err("E-OWNED-TYPE: field nominal arity differs".into());
        }
        Ok(ty)
    }
}

fn require_concrete(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Var(_) => Err("E-OWNED-TYPE: unresolved concrete type instance".into()),
        Type::Nominal(_, arguments) => {
            for argument in arguments {
                require_concrete(argument)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn concrete_fields_substitute_nested_parameters_and_classify_phantoms() {
        let program = parser::parse("type Phantom<a> = Marker\ntype Inner<a> = Empty | Pair(a, a, i32)\ntype Outer<a> = Missing | Nested(Inner<a>, Phantom<a>)\ntype Ghost<a> = Ghost(Phantom<a>)\nlet main = 42").unwrap();
        let checked = check::analyze(&program).unwrap();
        let nominal = |name: &str, arg| Type::Nominal(name.into(), vec![arg]);
        let phantom = nominal("Phantom", Type::SystemBytes);
        assert!(!checked.type_is_owner(&phantom).unwrap());
        assert!(!checked
            .type_is_owner(&nominal("Ghost", Type::SystemBytes))
            .unwrap());
        let outer = nominal("Outer", Type::SystemBytes);
        assert!(checked.type_is_owner(&outer).unwrap());
        assert!(!checked.type_is_owner(&nominal("Outer", Type::Int)).unwrap());
        let variants = checked.adt_variants(&outer).unwrap();
        assert_eq!(variants[1].name, "Nested");
        assert_eq!(
            variants[1].fields,
            vec![nominal("Inner", Type::SystemBytes), phantom]
        );
        // Repeated queries use the same concrete cache entries.
        assert!(checked.type_is_owner(&outer).unwrap());
        assert!(checked
            .owned_type_cache
            .lock()
            .unwrap()
            .contains_key(&outer));
        assert!(checked
            .adt_variant_cache
            .lock()
            .unwrap()
            .contains_key(&outer));
        assert!(checked
            .type_is_owner(&nominal("Outer", Type::Var(999)))
            .is_err());
        assert!(checked
            .adt_variants(&Type::Nominal("Outer".into(), vec![]))
            .is_err());
    }

    #[test]
    fn inactive_owner_variants_still_trigger_affine_entry_checks() {
        let program =
            parser::parse("type Outcome = Empty | Full(SystemBytes)\nlet main = Empty").unwrap();
        assert!(check::analyze(&program)
            .err()
            .unwrap()
            .contains("main cannot return an owner"));
        let recursive =
            parser::parse("type Recursive = End | Next(Recursive)\nlet main = 42").unwrap();
        assert!(check::analyze(&recursive)
            .err()
            .unwrap()
            .contains("recursive type"));
    }
}
