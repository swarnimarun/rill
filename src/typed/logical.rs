//! Frozen logical identities for tag-only enum layouts. These are not runtime
//! types or inference variables. Only a complete all-nullary declaration can
//! hide an opaque argument; every other runtime layout stays closed.
use super::{AdtOrigin, Type, TypedMain};
use crate::ast::{TypeDecl, TypeExpr};
use crate::check::{CheckedProgram, Type as SourceType};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const INVALID: &str = "E-TYPED-LOGICAL: invalid symbolic representation";
const LIMIT: &str = "E-TYPED-LOGICAL-COMPLEXITY: logical type work limit exceeded";
pub(super) const MAX_WORK: usize = 100_000;
pub(super) const MAX_DEPTH: usize = 128;

/// A compilation brand has identity, not a serialized address or solver ID.
#[derive(Clone, Debug)]
pub(super) struct Domain(Arc<()>);
impl Default for Domain {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}
impl PartialEq for Domain {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Domain {}
impl Hash for Domain {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OpaqueId {
    pub(super) domain: Domain,
    pub(super) index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LogicalType {
    /// A checked primitive leaf. Nominal and variable source types are invalid here.
    Atom(SourceType),
    Nominal {
        definition: usize,
        arguments: Vec<LogicalType>,
    },
    Opaque(OpaqueId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dependencies {
    pub(super) native: bool,
    pub(super) owner: bool,
}
impl Dependencies {
    fn include(&mut self, other: Self) {
        self.native |= other.native;
        self.owner |= other.owner;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SymbolicInstance {
    pub(super) definition: usize,
    pub(super) arguments: Vec<LogicalType>,
    // A cache only: the independent verifier reconstructs both bits from the
    // complete logical arguments and canonical declaration graph.
    pub(super) dependencies: Dependencies,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpaqueBinding {
    pub scope: usize,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Metadata {
    pub domain: Option<Domain>,
    pub declarations: Vec<TypeDecl>,
    pub source_functions: Vec<String>,
    pub scope_sources: Vec<usize>,
    pub function_scopes: Vec<usize>,
    pub binders: Vec<OpaqueBinding>,
}

#[derive(Default)]
pub(super) struct Budget(usize);
impl Budget {
    pub fn charge(&mut self, count: usize) -> Result<(), String> {
        self.0 = self
            .0
            .checked_add(count)
            .filter(|n| *n <= MAX_WORK)
            .ok_or(LIMIT)?;
        Ok(())
    }
}

pub(super) fn has_opaque(ty: &SourceType) -> bool {
    match ty {
        SourceType::Var(_) => true,
        SourceType::Nominal(_, args) => args.iter().any(has_opaque),
        _ => false,
    }
}

/// Validate before cloning, hashing or appending child work. This also charges
/// substituted trees, rather than just their unexpanded variable occurrence.
pub(super) fn charge_type(ty: &SourceType, budget: &mut Budget) -> Result<(), String> {
    fn walk(ty: &SourceType, depth: usize, budget: &mut Budget) -> Result<(), String> {
        budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err("E-TYPED-INSTANCES: concrete type nesting exceeds 128".into());
        }
        if let SourceType::Nominal(_, args) = ty {
            for arg in args {
                walk(arg, depth + 1, budget)?;
            }
        }
        Ok(())
    }
    walk(ty, 0, budget)
}

pub(super) fn all_nullary(decl: &TypeDecl) -> bool {
    decl.record_fields.is_none()
        && !decl.variants.is_empty()
        && decl
            .variants
            .iter()
            .all(|variant| variant.fields.is_empty())
}

pub(super) fn admit(checked: &CheckedProgram<'_>, ty: &SourceType) -> Result<(), String> {
    fn validate(checked: &CheckedProgram<'_>, ty: &SourceType, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        if let SourceType::Nominal(name, args) = ty {
            let decl = checked
                .program
                .types
                .iter()
                .find(|d| d.name == *name)
                .ok_or(INVALID)?;
            if decl.params.len() != args.len() {
                return Err(INVALID.into());
            }
            for arg in args {
                validate(checked, arg, depth + 1)?;
            }
        }
        Ok(())
    }
    validate(checked, ty, 0)?;
    if has_opaque(ty) {
        let allowed = if let SourceType::Nominal(name, _) = ty {
            checked
                .program
                .types
                .iter()
                .find(|d| d.name == *name)
                .is_some_and(all_nullary)
        } else {
            false
        };
        if !allowed {
            return Err("E-TYPED-UNSUPPORTED: unresolved polymorphic value requires local specialization; only all-nullary enum roots admit opaque logical arguments".into());
        }
    }
    Ok(())
}

fn atom(ty: &SourceType) -> Result<Dependencies, String> {
    let mut deps = Dependencies::default();
    match ty {
        SourceType::Int
        | SourceType::Bool
        | SourceType::Unit
        | SourceType::StaticStr
        | SourceType::U8x16 => {}
        SourceType::Bytes(n) if *n <= 256 => {}
        SourceType::CU8 | SourceType::CU32 | SourceType::CU64 | SourceType::CF32 => {
            deps.native = true
        }
        SourceType::CPointer { pointee, .. } => {
            super::foreign::validate_pointer(pointee)?;
            deps.native = true;
        }
        SourceType::SystemBoxI32 | SourceType::SystemBytes => deps.owner = true,
        _ => return Err(INVALID.into()),
    }
    Ok(deps)
}

// A schema contains resolved nominal names from the checked flattened program.
// Pointer pointee spellings are C identities, not schema type variables.
fn schema_atom(ty: &TypeExpr) -> Option<SourceType> {
    Some(match ty.name.as_str() {
        "i32" => SourceType::Int,
        "bool" => SourceType::Bool,
        "unit" => SourceType::Unit,
        "str" => SourceType::StaticStr,
        "u8x16" => SourceType::U8x16,
        "u8" => SourceType::CU8,
        "u32" => SourceType::CU32,
        "u64" => SourceType::CU64,
        "f32" => SourceType::CF32,
        "SystemBoxI32" => SourceType::SystemBoxI32,
        "SystemBytes" => SourceType::SystemBytes,
        _ => return None,
    })
}

/// Independently check the full generic declaration graph, including inactive
/// variants and nominal arguments. No active constructor is an erasure proof.
fn schema_dependencies(
    decls: &[TypeDecl],
    budget: &mut Budget,
) -> Result<Vec<Dependencies>, String> {
    budget.charge(decls.len())?;
    let mut names = HashMap::new();
    for (id, decl) in decls.iter().enumerate() {
        if decl.name.is_empty()
            || names
                .insert(decl.name.as_str(), (id, decl.params.len()))
                .is_some()
        {
            return Err(INVALID.into());
        }
    }
    let mut edges = vec![BTreeSet::new(); decls.len()];
    let mut direct = vec![Dependencies::default(); decls.len()];
    let mut constructors = HashSet::new();
    for (id, decl) in decls.iter().enumerate() {
        budget.charge(decl.params.len() + decl.variants.len())?;
        let params: HashSet<_> = decl.params.iter().map(String::as_str).collect();
        if params.len() != decl.params.len() || params.contains("") || decl.variants.is_empty() {
            return Err(INVALID.into());
        }
        if let Some(fields) = &decl.record_fields {
            budget.charge(fields.len())?;
            if decl.variants.len() != 1
                || fields.len() != decl.variants[0].fields.len()
                || fields.iter().any(String::is_empty)
                || fields.iter().collect::<HashSet<_>>().len() != fields.len()
            {
                return Err(INVALID.into());
            }
        }
        fn visit(
            ty: &TypeExpr,
            params: &HashSet<&str>,
            names: &HashMap<&str, (usize, usize)>,
            edges: &mut BTreeSet<usize>,
            deps: &mut Dependencies,
            depth: usize,
            budget: &mut Budget,
        ) -> Result<(), String> {
            budget.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            if params.contains(ty.name.as_str()) {
                if !ty.args.is_empty() {
                    return Err(INVALID.into());
                }
            } else if matches!(ty.name.as_str(), "ptr" | "const_ptr") {
                if ty.args.len() != 1 || !ty.args[0].args.is_empty() {
                    return Err(INVALID.into());
                }
                deps.include(atom(&SourceType::CPointer {
                    pointee: ty.args[0].name.clone(),
                    is_const: ty.name == "const_ptr",
                })?);
            } else if let Some(leaf) = schema_atom(ty) {
                if !ty.args.is_empty() {
                    return Err(INVALID.into());
                }
                deps.include(atom(&leaf)?);
            } else {
                let (child, arity) = *names.get(ty.name.as_str()).ok_or(INVALID)?;
                if arity != ty.args.len() {
                    return Err(INVALID.into());
                }
                edges.insert(child);
                for arg in &ty.args {
                    visit(arg, params, names, edges, deps, depth + 1, budget)?;
                }
            }
            Ok(())
        }
        for variant in &decl.variants {
            if variant.name.is_empty() || !constructors.insert(variant.name.as_str()) {
                return Err(INVALID.into());
            }
            for field in &variant.fields {
                visit(
                    field,
                    &params,
                    &names,
                    &mut edges[id],
                    &mut direct[id],
                    0,
                    budget,
                )?;
            }
        }
    }
    fn finish(
        id: usize,
        edges: &[BTreeSet<usize>],
        direct: &[Dependencies],
        states: &mut [(u8, usize)],
        result: &mut [Dependencies],
        depth: usize,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.charge(1)?;
        if states[id].0 == 2 {
            return Ok(());
        }
        if states[id].0 == 1 || depth > MAX_DEPTH {
            return Err(INVALID.into());
        }
        states[id].0 = 1;
        let mut deps = direct[id];
        let mut height = 0;
        for &child in &edges[id] {
            finish(child, edges, direct, states, result, depth + 1, budget)?;
            deps.include(result[child]);
            height = height.max(states[child].1 + 1);
        }
        if height > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        states[id].1 = height;
        result[id] = deps;
        states[id].0 = 2;
        Ok(())
    }
    let mut states = vec![(0, 0); decls.len()];
    let mut result = vec![Dependencies::default(); decls.len()];
    for id in 0..decls.len() {
        finish(id, &edges, &direct, &mut states, &mut result, 0, budget)?;
    }
    Ok(result)
}

impl Metadata {
    pub fn opaque_id(&self, index: usize) -> Result<OpaqueId, String> {
        if self.binders.get(index).is_none() {
            return Err(INVALID.into());
        }
        Ok(OpaqueId {
            domain: self.domain.as_ref().ok_or(INVALID)?.clone(),
            index,
        })
    }

    pub fn from_checked(
        checked: &CheckedProgram<'_>,
        instances: &super::instances::Instances,
    ) -> Result<Self, String> {
        if instances.binders.is_empty() {
            return Ok(Self::default());
        }
        let mut budget = Budget::default();
        schema_dependencies(&checked.program.types, &mut budget)?;
        budget.charge(
            checked.program.functions.len() + instances.list.len() + instances.binders.len(),
        )?;
        let mut scope_sources = vec![0; instances.list.len()];
        for instance in &instances.list {
            scope_sources[instance.scope] = instance.source;
        }
        let result = Self {
            domain: Some(instances.domain.clone()),
            declarations: checked.program.types.clone(),
            source_functions: checked
                .program
                .functions
                .iter()
                .map(|f| f.name.clone())
                .collect(),
            scope_sources,
            function_scopes: instances.list.iter().map(|i| i.scope).collect(),
            binders: instances.binders.clone(),
        };
        result.validate_source(checked, instances)?;
        Ok(result)
    }

    /// The source relationship is a distinct boundary check. A standalone IR
    /// verifier cannot prove that a consistently replaced schema is the source.
    pub fn validate_source(
        &self,
        checked: &CheckedProgram<'_>,
        instances: &super::instances::Instances,
    ) -> Result<(), String> {
        if self.binders.is_empty() && instances.binders.is_empty() {
            return Ok(());
        }
        if self.domain.as_ref() != Some(&instances.domain)
            || self.declarations != checked.program.types
            || self.binders != instances.binders
            || self.source_functions.len() != checked.program.functions.len()
            || self
                .source_functions
                .iter()
                .zip(&checked.program.functions)
                .any(|(name, f)| name != &f.name)
            || self.function_scopes.len() != instances.list.len()
            || self.scope_sources.len() != instances.list.len()
            || self
                .function_scopes
                .iter()
                .zip(&instances.list)
                .any(|(scope, instance)| {
                    *scope != instance.scope
                        || self.scope_sources.get(*scope) != Some(&instance.source)
                })
        {
            return Err(
                "E-TYPED-LOGICAL: canonical schema or scope disagrees with checked source".into(),
            );
        }
        Ok(())
    }

    pub fn instance(&self, name: &str, args: &[SourceType]) -> Result<SymbolicInstance, String> {
        let mut budget = Budget::default();
        let deps = schema_dependencies(&self.declarations, &mut budget)?;
        let definition = self
            .declarations
            .iter()
            .position(|d| d.name == name)
            .ok_or(INVALID)?;
        if !all_nullary(&self.declarations[definition])
            || self.declarations[definition].params.len() != args.len()
        {
            return Err(INVALID.into());
        }
        fn convert(
            ty: &SourceType,
            meta: &Metadata,
            depth: usize,
            budget: &mut Budget,
        ) -> Result<LogicalType, String> {
            budget.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            Ok(match ty {
                SourceType::Var(id) if *id < meta.binders.len() => {
                    LogicalType::Opaque(meta.opaque_id(*id)?)
                }
                SourceType::Nominal(name, args) => {
                    let definition = meta
                        .declarations
                        .iter()
                        .position(|d| d.name == *name)
                        .ok_or(INVALID)?;
                    if meta.declarations[definition].params.len() != args.len() {
                        return Err(INVALID.into());
                    }
                    LogicalType::Nominal {
                        definition,
                        arguments: args
                            .iter()
                            .map(|a| convert(a, meta, depth + 1, budget))
                            .collect::<Result<_, _>>()?,
                    }
                }
                ty => {
                    atom(ty)?;
                    LogicalType::Atom(ty.clone())
                }
            })
        }
        let arguments = args
            .iter()
            .map(|arg| convert(arg, self, 0, &mut budget))
            .collect::<Result<Vec<_>, _>>()?;
        let mut dependencies = Dependencies::default();
        let mut binders = HashSet::new();
        for arg in &arguments {
            dependencies.include(self.check_type(arg, &deps, &mut binders, 0, &mut budget)?);
        }
        if binders.is_empty() {
            return Err(INVALID.into());
        }
        Ok(SymbolicInstance {
            definition,
            arguments,
            dependencies,
        })
    }

    fn check_type(
        &self,
        ty: &LogicalType,
        schema: &[Dependencies],
        binders: &mut HashSet<usize>,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Dependencies, String> {
        budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        match ty {
            LogicalType::Atom(leaf) => atom(leaf),
            LogicalType::Opaque(id) => {
                if self.domain.as_ref() != Some(&id.domain) || self.binders.get(id.index).is_none()
                {
                    return Err(INVALID.into());
                }
                binders.insert(id.index);
                Ok(Dependencies::default())
            }
            LogicalType::Nominal {
                definition,
                arguments,
            } => {
                let decl = self.declarations.get(*definition).ok_or(INVALID)?;
                if decl.params.len() != arguments.len() {
                    return Err(INVALID.into());
                }
                let mut deps = schema[*definition];
                for arg in arguments {
                    deps.include(self.check_type(arg, schema, binders, depth + 1, budget)?);
                }
                Ok(deps)
            }
        }
    }

    pub fn verify(&self, program: &TypedMain) -> Result<Facts, String> {
        self.verify_with_budget(program, &mut Budget::default())
    }

    fn verify_with_budget(
        &self,
        program: &TypedMain,
        budget: &mut Budget,
    ) -> Result<Facts, String> {
        if program.adts.len() > 1024 || program.functions.len() > 1024 {
            return Err(LIMIT.into());
        }
        let symbolic = program.adts.iter().any(|a| a.symbolic.is_some());
        if !symbolic {
            if self.domain.is_some()
                || !self.binders.is_empty()
                || !self.declarations.is_empty()
                || !self.source_functions.is_empty()
                || !self.scope_sources.is_empty()
                || !self.function_scopes.is_empty()
            {
                return Err(INVALID.into());
            }
            return Ok(Facts {
                adt_binders: vec![HashSet::new(); program.adts.len()],
                allowed: vec![HashSet::new(); program.functions.len()],
            });
        }
        let schema = schema_dependencies(&self.declarations, budget)?;
        budget.charge(
            self.binders.len()
                + self.scope_sources.len()
                + self.source_functions.len()
                + program.adts.len()
                + program.functions.len(),
        )?;
        if self.domain.is_none()
            || self.function_scopes.len() != program.functions.len()
            || self.scope_sources.len() != program.functions.len()
            || self.binders.is_empty()
            || self.source_functions.iter().any(String::is_empty)
            || self.source_functions.iter().collect::<HashSet<_>>().len()
                != self.source_functions.len()
        {
            return Err(INVALID.into());
        }
        let mut scope_set = HashSet::new();
        let mut previous_source = None;
        for &scope in &self.function_scopes {
            let source = *self.scope_sources.get(scope).ok_or(INVALID)?;
            if source >= self.source_functions.len()
                || !scope_set.insert(scope)
                || previous_source.is_some_and(|previous| previous > source)
            {
                return Err(INVALID.into());
            }
            previous_source = Some(source);
        }
        if self.function_scopes.get(program.entry) != Some(&0)
            || self
                .scope_sources
                .first()
                .and_then(|source| self.source_functions.get(*source))
                .map(String::as_str)
                != Some("main")
        {
            return Err(INVALID.into());
        }
        let mut counts = vec![0; self.scope_sources.len()];
        let mut own = vec![HashSet::new(); self.scope_sources.len()];
        for (id, binder) in self.binders.iter().enumerate() {
            let count = counts.get_mut(binder.scope).ok_or(INVALID)?;
            if binder.ordinal != *count {
                return Err(INVALID.into());
            }
            *count += 1;
            own[binder.scope].insert(id);
        }
        let mut adt_binders: Vec<HashSet<usize>> = Vec::new();
        for adt in &program.adts {
            budget.charge(adt.arguments.len())?;
            budget.charge(adt.variants.len())?;
            for variant in &adt.variants {
                budget.charge(variant.fields.len())?;
            }
            let mut binders = HashSet::new();
            if let Some(instance) = &adt.symbolic {
                let decl = self.declarations.get(instance.definition).ok_or(INVALID)?;
                if adt.origin != AdtOrigin::Source
                    || adt.owned
                    || adt.record_fields.is_some()
                    || !adt.arguments.is_empty()
                    || adt.name != decl.name
                    || !all_nullary(decl)
                    || instance.arguments.len() != decl.params.len()
                    || adt.variants.len() != decl.variants.len()
                    || adt
                        .variants
                        .iter()
                        .zip(&decl.variants)
                        .any(|(actual, expected)| {
                            actual.name != expected.name || !actual.fields.is_empty()
                        })
                {
                    return Err(INVALID.into());
                }
                let mut deps = Dependencies::default();
                for arg in &instance.arguments {
                    deps.include(self.check_type(arg, &schema, &mut binders, 0, budget)?);
                }
                if binders.is_empty() || deps != instance.dependencies {
                    return Err(INVALID.into());
                }
            }
            for ty in adt
                .arguments
                .iter()
                .chain(adt.variants.iter().flat_map(|v| &v.fields))
            {
                if let Type::Adt { id, .. } = ty {
                    let child = adt_binders.get(*id).ok_or(INVALID)?;
                    budget.charge(child.len())?;
                    // Source payload products around open enum values need the
                    // next field-schema proof. Private capture products already
                    // prove exact field/identity equality independently.
                    if adt.origin == AdtOrigin::Source && !child.is_empty() {
                        return Err(INVALID.into());
                    }
                    binders.extend(child);
                }
            }
            adt_binders.push(binders);
        }
        let mut allowed = Vec::new();
        let mut keys = HashSet::new();
        for (index, function) in program.functions.iter().enumerate() {
            let scope = self.function_scopes[index];
            budget.charge(function.params.len())?;
            if !keys.insert((
                self.scope_sources[scope],
                function
                    .params
                    .iter()
                    .map(|(_, ty, mode)| (*ty, *mode as u8))
                    .collect::<Vec<_>>(),
                function.result,
            )) {
                return Err(INVALID.into());
            }
            budget.charge(own[scope].len())?;
            let mut visible = own[scope].clone();
            for ty in function
                .params
                .iter()
                .map(|(_, ty, _)| ty)
                .chain(std::iter::once(&function.result))
            {
                if let Type::Adt { id, .. } = ty {
                    let binders = adt_binders.get(*id).ok_or(INVALID)?;
                    budget.charge(binders.len())?;
                    visible.extend(binders);
                }
            }
            allowed.push(visible);
        }
        Ok(Facts {
            adt_binders,
            allowed,
        })
    }
}

pub(super) struct Facts {
    adt_binders: Vec<HashSet<usize>>,
    allowed: Vec<HashSet<usize>>,
}
impl Facts {
    pub fn in_scope(&self, ty: Type, function: usize) -> Result<(), String> {
        if let Type::Adt { id, .. } = ty {
            let binders = self.adt_binders.get(id).ok_or(INVALID)?;
            let allowed = self.allowed.get(function).ok_or(INVALID)?;
            if !binders.is_subset(allowed) {
                return Err("E-TYPED-LOGICAL: opaque identity escaped its instance scope".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn work_charge_has_an_exact_boundary_and_preserves_exhausted_state() {
        let mut budget = Budget::default();
        budget.charge(MAX_WORK).unwrap();
        assert_eq!(budget.0, MAX_WORK);
        assert_eq!(budget.charge(1).unwrap_err(), LIMIT);
        assert_eq!(budget.0, MAX_WORK);
        budget.charge(0).unwrap();
        assert_eq!(budget.charge(usize::MAX).unwrap_err(), LIMIT);
        assert_eq!(budget.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - 1);
        let ty = SourceType::Nominal("Flag".into(), vec![SourceType::Var(0)]);
        assert_eq!(charge_type(&ty, &mut short).unwrap_err(), LIMIT);
        assert_eq!(short.0, MAX_WORK);
        let mut exact = Budget(MAX_WORK - 2);
        charge_type(&ty, &mut exact).unwrap();
        assert_eq!(exact.0, MAX_WORK);
    }

    #[test]
    fn symbolic_tree_depth_has_an_exact_inclusive_boundary() {
        let mut ty = SourceType::Var(0);
        for _ in 0..MAX_DEPTH {
            ty = SourceType::Nominal("Flag".into(), vec![ty]);
        }
        charge_type(&ty, &mut Budget::default()).unwrap();
        ty = SourceType::Nominal("Flag".into(), vec![ty]);
        assert!(charge_type(&ty, &mut Budget::default())
            .unwrap_err()
            .contains("nesting"));
    }

    #[test]
    fn verifier_charges_each_retained_variant_table_and_exact_work_boundary() {
        let parsed = crate::parser::parse("type Flag<a> = Off | On\nfn make() { Off }\nfn main() { let a = make(); let b = make(); 42 }").unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let program = crate::typed::lower_main(&checked).unwrap();
        assert_eq!(
            program
                .adts
                .iter()
                .filter(|adt| adt.symbolic.is_some())
                .count(),
            2
        );
        let mut measured = Budget::default();
        program
            .logical
            .verify_with_budget(&program, &mut measured)
            .unwrap();
        let mut exact = Budget(MAX_WORK - measured.0);
        program
            .logical
            .verify_with_budget(&program, &mut exact)
            .unwrap();
        assert_eq!(exact.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - measured.0 + 1);
        assert!(
            matches!(program.logical.verify_with_budget(&program, &mut short), Err(error) if error == LIMIT)
        );
        // One additional constructor is retained once in the schema and once
        // in each logical instance, so it costs three independent work units.
        let mut larger = program.clone();
        larger.logical.declarations[0]
            .variants
            .push(crate::ast::VariantDecl {
                name: "Extra".into(),
                fields: Vec::new(),
            });
        for adt in &mut larger.adts {
            adt.variants.push(super::super::AdtVariant {
                name: "Extra".into(),
                fields: Vec::new(),
            });
        }
        let mut larger_work = Budget::default();
        larger
            .logical
            .verify_with_budget(&larger, &mut larger_work)
            .unwrap();
        assert_eq!(larger_work.0, measured.0 + 3);
    }

    fn chain(edges: usize) -> Vec<TypeDecl> {
        (0..=edges)
            .map(|index| TypeDecl {
                name: format!("D{index}"),
                params: Vec::new(),
                record_fields: None,
                variants: vec![crate::ast::VariantDecl {
                    name: format!("V{index}"),
                    fields: if index == edges {
                        Vec::new()
                    } else {
                        vec![TypeExpr {
                            name: format!("D{}", index + 1),
                            args: Vec::new(),
                        }]
                    },
                }],
            })
            .collect()
    }

    #[test]
    fn canonical_schema_work_and_dependency_depth_are_bounded_before_expansion() {
        let decls = chain(MAX_DEPTH);
        let mut measure = Budget::default();
        schema_dependencies(&decls, &mut measure).unwrap();
        let used = measure.0;
        let mut exact = Budget(MAX_WORK - used);
        schema_dependencies(&decls, &mut exact).unwrap();
        assert_eq!(exact.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - used + 1);
        assert_eq!(schema_dependencies(&decls, &mut short).unwrap_err(), LIMIT);
        assert!(schema_dependencies(&chain(MAX_DEPTH + 1), &mut Budget::default()).is_err());
        // A cycle is a malformed schema, not a layout hint.
        let mut cyclic = chain(1);
        cyclic[1].variants[0].fields.push(TypeExpr {
            name: "D0".into(),
            args: Vec::new(),
        });
        assert!(schema_dependencies(&cyclic, &mut Budget::default()).is_err());
    }
}
