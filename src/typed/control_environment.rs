//! Exact native/declaration retention for the new control-result path.
use super::*;
use crate::ast::{ExternType, TypeDecl, TypeExpr};
use crate::typed::{AdtOrigin, Type as RuntimeType, TypedMain};

fn text(value: &str, budget: &mut Budget) -> Result<(), String> {
    budget.charge(1)?;
    budget.charge(value.len())
}
fn schema(ty: &TypeExpr, budget: &mut Budget, depth: usize) -> Result<(), String> {
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    text(&ty.name, budget)?;
    budget.charge(ty.args.len())?;
    for arg in &ty.args {
        schema(arg, budget, depth + 1)?;
    }
    Ok(())
}
fn declarations(types: &[TypeDecl], budget: &mut Budget) -> Result<(), String> {
    budget.charge(types.len())?;
    for declaration in types {
        text(&declaration.name, budget)?;
        budget.charge(declaration.params.len())?;
        for parameter in &declaration.params {
            text(parameter, budget)?;
        }
        if let Some(fields) = &declaration.record_fields {
            budget.charge(fields.len())?;
            for field in fields {
                text(field, budget)?;
            }
        }
        budget.charge(declaration.variants.len())?;
        for variant in &declaration.variants {
            text(&variant.name, budget)?;
            budget.charge(variant.fields.len())?;
            for field in &variant.fields {
                schema(field, budget, 0)?;
            }
        }
    }
    Ok(())
}
fn external(ty: &ExternType, budget: &mut Budget) -> Result<(), String> {
    budget.charge(1)?;
    if let ExternType::Pointer { pointee, .. } = ty {
        text(pointee, budget)?;
    }
    Ok(())
}

impl Frozen {
    fn schema_type(
        &self,
        ty: &TypeExpr,
        parameters: &[String],
        arguments: &[Type],
        budget: &mut Budget,
        depth: usize,
    ) -> Result<Type, String> {
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        text(&ty.name, budget)?;
        budget.charge(parameters.len())?;
        for (parameter, actual) in parameters.iter().zip(arguments) {
            text(parameter, budget)?;
            if parameter == &ty.name {
                if !ty.args.is_empty() {
                    return Err(INVALID.into());
                }
                return copy_type(actual, budget, depth);
            }
        }
        let primitive = match ty.name.as_str() {
            "i32" => Some(Type::Int),
            "bool" => Some(Type::Bool),
            "unit" => Some(Type::Unit),
            "str" => Some(Type::StaticStr),
            "u8x16" => Some(Type::U8x16),
            _ => None,
        };
        if let Some(primitive) = primitive {
            if !ty.args.is_empty() {
                return Err(INVALID.into());
            }
            return Ok(primitive);
        }
        budget.charge(self.witness.environment.types.len())?;
        let mut definition = None;
        for declaration in &self.witness.environment.types {
            text(&declaration.name, budget)?;
            if declaration.name == ty.name {
                definition = Some(declaration);
                break;
            }
        }
        let declaration = definition
            .ok_or("E-TYPED-UNSUPPORTED: no-normal control schemas require pure Copy values")?;
        if declaration.params.len() != ty.args.len() {
            return Err(INVALID.into());
        }
        budget.charge(ty.args.len())?;
        let mut args = Vec::with_capacity(ty.args.len());
        for arg in &ty.args {
            args.push(self.schema_type(arg, parameters, arguments, budget, depth + 1)?);
        }
        Ok(Type::Nominal(budget.text(&ty.name)?, args))
    }
    pub(super) fn check_environment(
        &self,
        program: &TypedMain,
        budget: &mut Budget,
    ) -> Result<(), String> {
        let expected = &self.witness.environment;
        let native = &program.native;
        for list in [
            &native.config.includes,
            &native.config.links,
            &native.config.packages,
        ] {
            budget.charge(list.len())?;
            for value in list {
                text(value, budget)?;
            }
        }
        budget.charge(native.functions.len())?;
        for function in &native.functions {
            text(&function.name, budget)?;
            budget.charge(function.params.len())?;
            for param in &function.params {
                text(&param.name, budget)?;
                external(&param.ty, budget)?;
            }
            external(&function.result, budget)?;
        }
        budget.charge(native.constants.len())?;
        for constant in &native.constants {
            text(&constant.name, budget)?;
            external(&constant.ty, budget)?;
        }
        budget.charge(native.pointers.len())?;
        for pointer in &native.pointers {
            text(&pointer.pointee, budget)?;
        }
        if native.config != expected.native
            || native.functions != expected.externs
            || native.constants != expected.constants
        {
            return Err(INVALID.into());
        }
        let mut pointers: Vec<crate::typed::foreign::PointerType> = Vec::new();
        for ty in expected
            .externs
            .iter()
            .flat_map(|f| {
                f.params
                    .iter()
                    .map(|p| &p.ty)
                    .chain(std::iter::once(&f.result))
            })
            .chain(expected.constants.iter().map(|c| &c.ty))
        {
            budget.charge(1)?;
            if let ExternType::Pointer { pointee, is_const } = ty {
                budget.charge(pointers.len())?;
                let mut found = false;
                for pointer in &pointers {
                    budget.charge(
                        pointer
                            .pointee
                            .len()
                            .checked_add(pointee.len())
                            .ok_or(LIMIT)?,
                    )?;
                    found |= pointer.pointee == *pointee && pointer.is_const == *is_const;
                }
                if !found {
                    budget.charge(1)?;
                    pointers.push(crate::typed::foreign::PointerType {
                        pointee: budget.text(pointee)?,
                        is_const: *is_const,
                    });
                }
            }
        }
        if pointers != native.pointers {
            return Err(INVALID.into());
        }
        match (&expected.allocation, &program.allocation_contract) {
            (None, None) => {}
            (Some(expected), Some(actual)) => {
                for (expected, actual) in [
                    (&expected.result_type, &actual.result_name),
                    (&expected.ok_constructor, &actual.ok_name),
                    (&expected.err_constructor, &actual.err_name),
                    (&expected.error_type, &actual.error_name),
                    (
                        &expected.invalid_length_constructor,
                        &actual.invalid_length_name,
                    ),
                    (
                        &expected.size_overflow_constructor,
                        &actual.size_overflow_name,
                    ),
                    (
                        &expected.out_of_memory_constructor,
                        &actual.out_of_memory_name,
                    ),
                ] {
                    text(expected, budget)?;
                    text(actual, budget)?;
                    if expected != actual {
                        return Err(INVALID.into());
                    }
                }
            }
            _ => return Err(INVALID.into()),
        }
        let logical = &program.logical;
        declarations(&logical.declarations, budget)?;
        budget.charge(logical.source_functions.len())?;
        for name in &logical.source_functions {
            text(name, budget)?;
        }
        budget.charge(logical.binders.len())?;
        budget.charge(logical.function_scopes.len())?;
        budget.charge(logical.scope_sources.len())?;
        if self.binders.is_empty() {
            if logical.domain.is_some()
                || !logical.binders.is_empty()
                || !logical.declarations.is_empty()
                || !logical.source_functions.is_empty()
                || !logical.function_scopes.is_empty()
                || !logical.scope_sources.is_empty()
            {
                return Err(INVALID.into());
            }
        } else {
            if logical.domain.as_ref() != Some(&self.domain)
                || logical.binders != self.binders
                || logical.source_functions != expected.function_names
                || logical.declarations != expected.types
                || logical.function_scopes.len() != self.instances.len()
                || logical.scope_sources.len() != self.instances.len()
            {
                return Err(INVALID.into());
            }
            budget.charge(self.instances.len())?;
            for (slot, instance) in self.instances.iter().enumerate() {
                if logical.function_scopes[slot] != instance.scope
                    || logical.scope_sources.get(instance.scope) != Some(&instance.source)
                {
                    return Err(INVALID.into());
                }
            }
        }
        // Pin every complete runtime nominal schema, not just the constructors
        // currently executed or fields demanded by a phantom product.
        budget.charge(program.adts.len())?;
        for (id, adt) in program.adts.iter().enumerate() {
            if adt.origin != AdtOrigin::Source || adt.owned {
                return Err(INVALID.into());
            }
            let Type::Nominal(name, args) =
                self.runtime_type(RuntimeType::Adt { id, owned: false }, program, budget, 0)?
            else {
                return Err(INVALID.into());
            };
            budget.charge(expected.types.len())?;
            let mut source = None;
            for declaration in &expected.types {
                text(&declaration.name, budget)?;
                if declaration.name == name {
                    source = Some(declaration);
                    break;
                }
            }
            let source = source.ok_or(INVALID)?;
            if source.params.len() != args.len() || source.variants.len() != adt.variants.len() {
                return Err(INVALID.into());
            }
            if let Some(fields) = &adt.record_fields {
                budget.charge(fields.len())?;
                for field in fields {
                    text(field, budget)?;
                }
            }
            if source.record_fields != adt.record_fields {
                return Err(INVALID.into());
            }
            budget.charge(adt.variants.len())?;
            let parameters = &source.params;
            for (source, actual) in source.variants.iter().zip(&adt.variants) {
                text(&actual.name, budget)?;
                if source.name != actual.name || source.fields.len() != actual.fields.len() {
                    return Err(INVALID.into());
                }
                budget.charge(source.fields.len())?;
                for (field, actual) in source.fields.iter().zip(&actual.fields) {
                    let expected = self.schema_type(field, parameters, &args, budget, 0)?;
                    if expected != self.runtime_type(*actual, program, budget, 0)? {
                        return Err(INVALID.into());
                    }
                }
            }
        }
        Ok(())
    }
}
