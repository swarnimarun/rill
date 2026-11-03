//! Finite nominal layouts: closed physical fields and separately proved
//! tag-only enums with frozen logical arguments.
use super::{AdtType, AdtVariant, Type};
use crate::check::{CheckedProgram, Type as SourceType};
use std::collections::{HashMap, HashSet};

pub(super) struct Registry {
    pub types: Vec<AdtType>,
    pub logical: super::logical::Metadata,
    pub pointers: Vec<super::foreign::PointerType>,
    ids: HashMap<SourceType, Type>,
    active: HashSet<SourceType>,
    logical_work: super::logical::Budget,
}
impl Registry {
    pub fn build(
        checked: &CheckedProgram<'_>,
        instances: &super::instances::Instances,
        native: &super::NativeManifest,
    ) -> Result<Self, String> {
        let mut registry = Self {
            types: Vec::new(),
            logical: super::logical::Metadata::from_checked(checked, instances)?,
            pointers: native.pointers.clone(),
            ids: HashMap::new(),
            active: HashSet::new(),
            logical_work: super::logical::Budget::default(),
        };
        for instance in &instances.list {
            for ty in instance
                .params
                .iter()
                .chain(std::iter::once(&instance.result))
            {
                registry.register(checked, ty)?;
            }
        }
        for instance in &instances.list {
            let mut pending = vec![&checked.program.functions[instance.source].body];
            while let Some(expr) = pending.pop() {
                let ty = super::instances::expression_type(checked, expr, &instance.substitution)?;
                registry.register(checked, &ty)?;
                super::instances::runtime_children(checked, expr, &mut pending);
            }
        }
        layout_costs(&registry.types)?;
        Ok(registry)
    }
    pub(super) fn register(
        &mut self,
        checked: &CheckedProgram<'_>,
        ty: &SourceType,
    ) -> Result<Type, String> {
        if let Some(ty) = self.ids.get(ty) {
            return Ok(*ty);
        }
        if let SourceType::CPointer { pointee, is_const } = ty {
            let id = super::foreign::intern_pointer(&mut self.pointers, pointee, *is_const)?;
            let result = Type::RawPointer(id);
            self.ids.insert(ty.clone(), result);
            return Ok(result);
        }
        if !matches!(ty, SourceType::Nominal(..)) {
            return super::primitive_type(ty);
        }
        if super::logical::has_opaque(ty) {
            super::logical::charge_type(ty, &mut self.logical_work)?;
            super::logical::admit(checked, ty)?;
            let SourceType::Nominal(name, args) = ty else {
                unreachable!()
            };
            if self.types.len() >= 1024 {
                return Err("E-TYPED-ADT: excessive nominal layout".into());
            }
            let symbolic = self.logical.instance(name, args)?;
            let declaration = &self.logical.declarations[symbolic.definition];
            // Tag-only still retains every constructor identity. Reserve the
            // complete table before cloning names or growing either registry.
            self.logical_work.charge(declaration.variants.len())?;
            let id = self.types.len();
            self.types.push(AdtType {
                symbolic: Some(symbolic),
                origin: super::AdtOrigin::Source,
                record_fields: None,
                name: name.clone(),
                arguments: Vec::new(),
                variants: declaration
                    .variants
                    .iter()
                    .map(|variant| AdtVariant {
                        name: variant.name.clone(),
                        fields: Vec::new(),
                    })
                    .collect(),
                owned: false,
            });
            let result = Type::Adt { id, owned: false };
            self.ids.insert(ty.clone(), result);
            return Ok(result);
        }
        if checked.is_record_type(ty) && checked.type_is_owner(ty)? {
            return Err(
                "E-TYPED-UNSUPPORTED: owned records require initialization and cleanup tracking"
                    .into(),
            );
        }
        if self.active.len() >= 128 || self.types.len() >= 1024 || !self.active.insert(ty.clone()) {
            return Err("E-TYPED-ADT: recursive or excessive nominal layout".into());
        }
        let SourceType::Nominal(name, args) = ty else {
            unreachable!()
        };
        let arguments = args
            .iter()
            .map(|t| self.register(checked, t))
            .collect::<Result<Vec<_>, _>>()?;
        let mut variants = Vec::new();
        for variant in checked.adt_variants(ty)? {
            let fields = variant
                .fields
                .iter()
                .map(|t| self.register(checked, t))
                .collect::<Result<Vec<_>, _>>()?;
            variants.push(AdtVariant {
                name: variant.name,
                fields,
            });
        }
        let owned = variants
            .iter()
            .flat_map(|v| &v.fields)
            .any(|t| t.is_owner());
        let id = self.types.len();
        let record_fields = checked
            .program
            .types
            .iter()
            .find(|decl| decl.name == *name)
            .and_then(|decl| decl.record_fields.clone());
        self.types.push(AdtType {
            symbolic: None,
            origin: super::AdtOrigin::Source,
            record_fields,
            name: name.clone(),
            arguments,
            variants,
            owned,
        });
        let result = Type::Adt { id, owned };
        self.ids.insert(ty.clone(), result);
        self.active.remove(ty);
        Ok(result)
    }
    pub fn convert(&self, ty: &SourceType) -> Result<Type, String> {
        if let Some(ty) = self.ids.get(ty) {
            Ok(*ty)
        } else {
            super::primitive_type(ty)
        }
    }
}

pub(super) fn verify(types: &[AdtType]) -> Result<(), String> {
    if types.len() > 1024 {
        return Err("E-TYPED-ADT: excessive type registry".into());
    }
    let mut identities = HashSet::new();
    for (id, adt) in types.iter().enumerate() {
        if adt.name.is_empty()
            || adt.variants.is_empty()
            || !identities.insert((
                adt.origin,
                adt.name.clone(),
                adt.arguments.clone(),
                adt.symbolic.clone(),
            ))
        {
            return Err("E-TYPED-ADT: invalid nominal identity".into());
        }
        if adt.symbolic.is_some()
            && (adt.origin != super::AdtOrigin::Source
                || adt.owned
                || adt.record_fields.is_some()
                || !adt.arguments.is_empty()
                || adt.variants.iter().any(|v| !v.fields.is_empty()))
        {
            return Err(
                "E-TYPED-LOGICAL: symbolic arguments require a tag-only source enum".into(),
            );
        }
        super::records::verify_definition(adt)?;
        super::local_values::verify_capture_definition(adt, types)?;
        let mut names = HashSet::new();
        let mut owned = false;
        for variant in &adt.variants {
            if variant.name.is_empty()
                || !names.insert(&variant.name)
                || variant.fields.len() > 4096
            {
                return Err("E-TYPED-ADT: invalid variants".into());
            }
            for &field in &variant.fields {
                validate_type(field, types)?;
                if let Type::Adt { id: child, .. } = field {
                    if child >= id {
                        return Err("E-TYPED-ADT: recursive or unordered nominal layout".into());
                    }
                }
                owned |= field.is_owner();
            }
        }
        for &argument in &adt.arguments {
            validate_type(argument, types)?;
            if let Type::Adt { id: child, .. } = argument {
                if child >= id {
                    return Err("E-TYPED-ADT: recursive nominal argument".into());
                }
            }
        }
        if owned != adt.owned {
            return Err("E-TYPED-ADT: forged ownership classification".into());
        }
    }
    layout_costs(types)?;
    Ok(())
}
pub(super) fn validate_type(ty: Type, types: &[AdtType]) -> Result<(), String> {
    match ty {
        Type::Adt { id, owned } if types.get(id).is_none_or(|a| a.owned != owned) => {
            Err("E-TYPED-ADT: invalid nominal type".into())
        }
        Type::Bytes(n) if n > 256 => Err("E-TYPED: invalid buffer capacity".into()),
        _ => Ok(()),
    }
}

/// Conservative inline byte costs, computed once over the verified DAG.
pub(super) fn layout_costs(types: &[AdtType]) -> Result<Vec<usize>, String> {
    let mut costs = Vec::with_capacity(types.len());
    for adt in types {
        let mut largest = 64usize;
        for variant in &adt.variants {
            let mut cost = 64usize;
            for ty in &variant.fields {
                let field = match ty {
                    Type::Adt { id, .. } => {
                        *costs.get(*id).ok_or("E-TYPED-ADT: recursive layout")?
                    }
                    Type::Bytes(n) => (*n).max(64),
                    _ => 64,
                };
                cost = cost
                    .checked_add(field)
                    .ok_or("E-C-STACK: excessive nominal layout")?;
                if cost > 1024 * 1024 {
                    return Err("E-C-STACK: nominal inline layout exceeds 1 MiB".into());
                }
            }
            largest = largest.max(cost);
        }
        costs.push(largest);
    }
    Ok(costs)
}

#[cfg(test)]
mod symbolic_budget_tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn symbolic_variants_are_reserved_before_registry_growth() {
        let parsed = parser::parse("type Flag<a> = Off | On\nfn main() { Off }").unwrap();
        let checked = check::analyze(&parsed).unwrap();
        let instances = super::super::instances::Instances::discover(&checked).unwrap();
        let native = super::super::NativeManifest::from_checked(&checked).unwrap();
        let mut registry = Registry::build(&checked, &instances, &native).unwrap();
        let nested = SourceType::Nominal(
            "Flag".into(),
            vec![SourceType::Nominal("Flag".into(), vec![SourceType::Var(0)])],
        );
        let before = (
            registry.types.len(),
            registry.ids.len(),
            registry.pointers.len(),
        );
        registry.logical_work = super::super::logical::Budget::default();
        // Three logical nodes plus both variants need five retained work units.
        registry
            .logical_work
            .charge(super::super::logical::MAX_WORK - 4)
            .unwrap();
        assert!(registry
            .register(&checked, &nested)
            .unwrap_err()
            .contains("LOGICAL-COMPLEXITY"));
        assert_eq!(
            (
                registry.types.len(),
                registry.ids.len(),
                registry.pointers.len()
            ),
            before
        );
        assert!(registry.active.is_empty());
        registry.logical_work = super::super::logical::Budget::default();
        registry
            .logical_work
            .charge(super::super::logical::MAX_WORK - 5)
            .unwrap();
        let ty = registry.register(&checked, &nested).unwrap();
        assert_eq!(registry.types.len(), before.0 + 1);
        assert_eq!(registry.ids.len(), before.1 + 1);
        // Reusing a verified complete layout does not clone another tag table.
        assert_eq!(registry.register(&checked, &nested).unwrap(), ty);
    }
}
