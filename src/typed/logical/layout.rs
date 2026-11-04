//! Finite Copy representations for open nominal instances. The source catalog
//! is a bounded decoding of complete declarations, not a layout certificate.
//! Verification builds its own catalog and reconstructs every physical field,
//! including the complete layouts of closed descendants.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const UNSUPPORTED: &str = "E-TYPED-UNSUPPORTED: unresolved polymorphic value requires local specialization; every physical field must have a finite Copy representation";

#[derive(Debug)]
enum Schema {
    Parameter(usize),
    Atom(SourceType),
    Nominal(usize, Vec<Schema>),
}

#[derive(Debug, Default)]
struct Demand {
    // Parameter -> longest physical path to its occurrence, excluding this
    // declaration's own root. Sparse storage avoids a declarations x arity table.
    parameters: std::collections::BTreeMap<usize, usize>,
    fixed_owner: bool,
    height: usize,
}
impl Demand {
    fn include(&mut self, other: &Self, offset: usize, budget: &mut Budget) -> Result<(), String> {
        budget.charge(other.parameters.len() + 1)?;
        self.fixed_owner |= other.fixed_owner;
        self.height = self
            .height
            .max(other.height.checked_add(offset).ok_or(LIMIT)?);
        for (&parameter, &distance) in &other.parameters {
            let distance = distance.checked_add(offset).ok_or(LIMIT)?;
            self.parameters
                .entry(parameter)
                .and_modify(|d| *d = (*d).max(distance))
                .or_insert(distance);
        }
        if self.height > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        Ok(())
    }
}

pub(in super::super) struct Catalog {
    names: HashMap<String, usize>,
    fields: Vec<Vec<Vec<Schema>>>,
    arities: Vec<usize>,
    products: Vec<bool>,
    demands: Vec<Demand>,
    pub(in super::super) dependencies: Vec<Dependencies>,
}

// Reserve identifier scans/copies and table storage before decoding or hashing
// any complete schema. K1's existing schema counter remains unchanged.
pub(in super::super) fn charge_schema(
    decls: &[TypeDecl],
    budget: &mut Budget,
) -> Result<(), String> {
    fn node(ty: &TypeExpr, depth: usize, budget: &mut Budget) -> Result<(), String> {
        budget.charge(1)?;
        budget.charge(ty.name.len())?;
        budget.charge(ty.args.len())?;
        if matches!(ty.name.as_str(), "ptr" | "const_ptr") && ty.args.len() == 1 {
            budget.charge(ty.args[0].name.len().checked_mul(3).ok_or(LIMIT)?)?;
        }
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        for arg in &ty.args {
            node(arg, depth + 1, budget)?;
        }
        Ok(())
    }
    budget.charge(decls.len())?;
    for decl in decls {
        budget.charge(decl.name.len())?;
        budget.charge(decl.params.len())?;
        for param in &decl.params {
            budget.charge(param.len())?;
        }
        if let Some(fields) = &decl.record_fields {
            budget.charge(fields.len())?;
            for field in fields {
                budget.charge(field.len())?;
            }
        }
        budget.charge(decl.variants.len())?;
        for variant in &decl.variants {
            budget.charge(variant.name.len())?;
            budget.charge(variant.fields.len())?;
            for field in &variant.fields {
                node(field, 0, budget)?;
            }
        }
    }
    Ok(())
}

impl Catalog {
    pub(in super::super) fn new(decls: &[TypeDecl], budget: &mut Budget) -> Result<Self, String> {
        charge_schema(decls, budget)?;
        let dependencies = schema_dependencies(decls, budget)?;
        // The validation index above borrowed names. This retained index
        // additionally copies and hashes each complete declaration name.
        budget.charge(decls.len())?;
        for declaration in decls {
            budget.charge(declaration.name.len().checked_mul(2).ok_or(LIMIT)?)?;
        }
        let names: HashMap<_, _> = decls
            .iter()
            .enumerate()
            .map(|(id, d)| (d.name.clone(), id))
            .collect();
        fn decode(
            ty: &TypeExpr,
            params: &HashMap<&str, usize>,
            names: &HashMap<String, usize>,
            budget: &mut Budget,
        ) -> Result<Schema, String> {
            budget.charge(1)?;
            // Parameter lookup plus possible nominal lookup are distinct
            // hashes of the spelling, including on a parameter miss.
            budget.charge(ty.name.len().checked_mul(2).ok_or(LIMIT)?)?;
            if let Some(&parameter) = params.get(ty.name.as_str()) {
                return Ok(Schema::Parameter(parameter));
            }
            if matches!(ty.name.as_str(), "ptr" | "const_ptr") {
                budget.charge(ty.args[0].name.len())?;
                return Ok(Schema::Atom(SourceType::CPointer {
                    pointee: ty.args[0].name.clone(),
                    is_const: ty.name == "const_ptr",
                }));
            }
            if let Some(atom) = schema_atom(ty) {
                return Ok(Schema::Atom(atom));
            }
            let definition = *names.get(&ty.name).ok_or(INVALID)?;
            budget.charge(ty.args.len())?;
            Ok(Schema::Nominal(
                definition,
                ty.args
                    .iter()
                    .map(|arg| decode(arg, params, names, budget))
                    .collect::<Result<_, _>>()?,
            ))
        }
        budget.charge(decls.len().checked_mul(5).ok_or(LIMIT)?)?;
        let mut fields = Vec::with_capacity(decls.len());
        for decl in decls {
            budget.charge(decl.params.len() + decl.variants.len())?;
            for param in &decl.params {
                budget.charge(param.len())?;
            }
            let params = decl
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| (p.as_str(), i))
                .collect();
            let mut variants = Vec::with_capacity(decl.variants.len());
            for variant in &decl.variants {
                budget.charge(variant.fields.len())?;
                variants.push(
                    variant
                        .fields
                        .iter()
                        .map(|f| decode(f, &params, &names, budget))
                        .collect::<Result<_, _>>()?,
                );
            }
            fields.push(variants);
        }
        fn summary(
            node: &Schema,
            all: &[Vec<Vec<Schema>>],
            states: &mut [u8],
            demands: &mut [Demand],
            depth: usize,
            budget: &mut Budget,
        ) -> Result<Demand, String> {
            budget.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            let mut result = Demand::default();
            match node {
                Schema::Parameter(p) => {
                    budget.charge(1)?;
                    result.parameters.insert(*p, 0);
                }
                Schema::Atom(leaf) => {
                    result.fixed_owner = atom(leaf)?.owner;
                }
                Schema::Nominal(id, args) => {
                    finish(*id, all, states, demands, depth + 1, budget)?;
                    result.fixed_owner = demands[*id].fixed_owner;
                    result.height = demands[*id].height.checked_add(1).ok_or(LIMIT)?;
                    // Copy only the sparse demanded edges. Recursing into an
                    // erased argument here would conflate identity and storage.
                    budget.charge(demands[*id].parameters.len())?;
                    let required: Vec<_> = demands[*id]
                        .parameters
                        .iter()
                        .map(|(&p, &d)| (p, d))
                        .collect();
                    for (parameter, distance) in required {
                        let arg = summary(
                            args.get(parameter).ok_or(INVALID)?,
                            all,
                            states,
                            demands,
                            depth + 1,
                            budget,
                        )?;
                        result.include(&arg, distance.checked_add(1).ok_or(LIMIT)?, budget)?;
                    }
                }
            }
            if result.height > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            Ok(result)
        }
        fn finish(
            id: usize,
            fields: &[Vec<Vec<Schema>>],
            states: &mut [u8],
            demands: &mut [Demand],
            depth: usize,
            budget: &mut Budget,
        ) -> Result<(), String> {
            budget.charge(1)?;
            if states[id] == 2 {
                return Ok(());
            }
            if states[id] != 0 || depth > MAX_DEPTH {
                return Err(INVALID.into());
            }
            states[id] = 1;
            let mut result = Demand::default();
            for variant in &fields[id] {
                budget.charge(1)?;
                for field in variant {
                    result.include(
                        &summary(field, fields, states, demands, depth, budget)?,
                        0,
                        budget,
                    )?;
                }
            }
            demands[id] = result;
            states[id] = 2;
            Ok(())
        }
        let mut demands: Vec<_> = (0..decls.len()).map(|_| Demand::default()).collect();
        let mut states = vec![0; decls.len()];
        for id in 0..decls.len() {
            finish(id, &fields, &mut states, &mut demands, 0, budget)?;
        }
        Ok(Self {
            names,
            fields,
            arities: decls.iter().map(|d| d.params.len()).collect(),
            products: decls.iter().map(|d| !all_nullary(d)).collect(),
            demands,
            dependencies,
        })
    }

    pub(in super::super) fn definition(
        &self,
        name: &str,
        budget: &mut Budget,
    ) -> Result<usize, String> {
        budget.charge(name.len() + 1)?;
        self.names.get(name).copied().ok_or_else(|| INVALID.into())
    }

    pub(in super::super) fn validate_type(
        &self,
        ty: &SourceType,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        match ty {
            SourceType::Var(_) => {}
            SourceType::Nominal(name, args) => {
                let id = self.definition(name, budget)?;
                if args.len() != self.arities[id] {
                    return Err(INVALID.into());
                }
                budget.charge(args.len())?;
                for arg in args {
                    self.validate_type(arg, depth + 1, budget)?;
                }
            }
            leaf => {
                charge_atom(leaf, budget)?;
                atom(leaf)?;
            }
        }
        Ok(())
    }

    pub(in super::super) fn finite(
        &self,
        ty: &SourceType,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        match ty {
            SourceType::Var(_) => Err(UNSUPPORTED.into()),
            SourceType::Nominal(name, args) => {
                let id = self.definition(name, budget)?;
                if args.len() != self.arities[id] {
                    return Err(INVALID.into());
                }
                let demand = &self.demands[id];
                if demand.fixed_owner {
                    return Err(UNSUPPORTED.into());
                }
                let root = depth.checked_add(1).ok_or(LIMIT)?;
                if root.checked_add(demand.height).ok_or(LIMIT)? > MAX_DEPTH {
                    return Err(LIMIT.into());
                }
                budget.charge(demand.parameters.len())?;
                for (&parameter, &distance) in &demand.parameters {
                    self.finite(
                        &args[parameter],
                        root.checked_add(distance).ok_or(LIMIT)?,
                        budget,
                    )?;
                }
                Ok(())
            }
            leaf => {
                charge_atom(leaf, budget)?;
                if atom(leaf)?.owner {
                    Err(UNSUPPORTED.into())
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(in super::super) fn fields(
        &self,
        definition: usize,
        arguments: &[SourceType],
        decls: &[TypeDecl],
        budget: &mut Budget,
    ) -> Result<Vec<Vec<SourceType>>, String> {
        fn build(
            node: &Schema,
            args: &[SourceType],
            decls: &[TypeDecl],
            depth: usize,
            budget: &mut Budget,
        ) -> Result<SourceType, String> {
            budget.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            Ok(match node {
                Schema::Parameter(i) => {
                    let value = args.get(*i).ok_or(INVALID)?;
                    charge_source(value, depth, budget)?;
                    value.clone()
                }
                Schema::Atom(atom) => {
                    charge_atom(atom, budget)?;
                    atom.clone()
                }
                Schema::Nominal(id, children) => {
                    budget.charge(decls[*id].name.len())?;
                    budget.charge(children.len())?;
                    SourceType::Nominal(
                        decls[*id].name.clone(),
                        children
                            .iter()
                            .map(|c| build(c, args, decls, depth + 1, budget))
                            .collect::<Result<_, _>>()?,
                    )
                }
            })
        }
        let variants = self.fields.get(definition).ok_or(INVALID)?;
        if arguments.len() != self.arities[definition] {
            return Err(INVALID.into());
        }
        budget.charge(variants.len())?;
        let mut result = Vec::with_capacity(variants.len());
        for variant in variants {
            if variant.len() > 4096 {
                return Err(LIMIT.into());
            }
            budget.charge(variant.len())?;
            result.push(
                variant
                    .iter()
                    .map(|f| build(f, arguments, decls, 0, budget))
                    .collect::<Result<_, _>>()?,
            );
        }
        Ok(result)
    }
}

fn charge_atom(ty: &SourceType, budget: &mut Budget) -> Result<(), String> {
    if let SourceType::CPointer { pointee, .. } = ty {
        budget.charge(pointee.len())?;
    }
    Ok(())
}

pub(in super::super) fn charge_source(
    ty: &SourceType,
    depth: usize,
    budget: &mut Budget,
) -> Result<(), String> {
    budget.charge(1)?;
    if depth > MAX_DEPTH {
        return Err(LIMIT.into());
    }
    if let SourceType::Nominal(name, args) = ty {
        budget.charge(name.len())?;
        budget.charge(args.len())?;
        for arg in args {
            charge_source(arg, depth + 1, budget)?;
        }
    } else {
        charge_atom(ty, budget)?;
    }
    Ok(())
}

/// One lazy source catalog and one cumulative admission budget per lowering.
/// Closed programs and K1-only nullary programs never activate K2 preparation.
#[derive(Default)]
pub(in super::super) struct Admission {
    catalog: RefCell<Option<Rc<Catalog>>>,
    work: RefCell<Budget>,
}
impl Admission {
    pub(in super::super) fn active(&self) -> bool {
        self.catalog.borrow().is_some()
    }
    pub(in super::super) fn reserve_type(
        &self,
        ty: &SourceType,
        copies: usize,
    ) -> Result<(), String> {
        if self.active() {
            let mut work = self.work.borrow_mut();
            for _ in 0..copies {
                charge_source(ty, 0, &mut work)?;
            }
        }
        Ok(())
    }
    pub(in super::super) fn reserve_entries(&self, count: usize) -> Result<(), String> {
        if self.active() {
            self.work.borrow_mut().charge(count)?;
        }
        Ok(())
    }

    pub(in super::super) fn catalog(
        &self,
        checked: &CheckedProgram<'_>,
    ) -> Result<Rc<Catalog>, String> {
        if let Some(catalog) = self.catalog.borrow().as_ref() {
            return Ok(Rc::clone(catalog));
        }
        let catalog = Rc::new(Catalog::new(
            &checked.program.types,
            &mut Budget::default(),
        )?);
        *self.catalog.borrow_mut() = Some(Rc::clone(&catalog));
        Ok(catalog)
    }
    pub(in super::super) fn is_product(
        &self,
        checked: &CheckedProgram<'_>,
        ty: &SourceType,
    ) -> Result<bool, String> {
        if let Some(catalog) = self.catalog.borrow().as_ref() {
            let mut work = self.work.borrow_mut();
            charge_source(ty, 0, &mut work)?;
            if !has_opaque(ty) {
                return Ok(false);
            }
            return match ty {
                SourceType::Nominal(name, _) => {
                    Ok(catalog.products[catalog.definition(name, &mut work)?])
                }
                _ => Ok(false),
            };
        }
        if !has_opaque(ty) {
            return Ok(false);
        }
        if let SourceType::Nominal(name, _) = ty {
            let id = {
                // K1 keeps its existing admission contract until a non-nullary
                // root activates the indexed, bounded product machinery.
                checked
                    .program
                    .types
                    .iter()
                    .position(|d| d.name == *name)
                    .ok_or(INVALID)?
            };
            return Ok(!all_nullary(&checked.program.types[id]));
        }
        Ok(false)
    }
    pub(in super::super) fn admit(
        &self,
        checked: &CheckedProgram<'_>,
        ty: &SourceType,
    ) -> Result<(), String> {
        if !self.active() && !self.is_product(checked, ty)? {
            return super::admit(checked, ty);
        }
        let catalog = self.catalog(checked)?;
        let mut work = self.work.borrow_mut();
        catalog.validate_type(ty, 0, &mut work)?;
        catalog.finite(ty, 0, &mut work)
    }
    pub(in super::super) fn charge_substitution(
        &self,
        checked: &CheckedProgram<'_>,
        ty: &SourceType,
        context: &super::super::instances::Substitution,
    ) -> Result<(), String> {
        // Determine the root without expanding its argument tree. Arguments in
        // context are already frozen, never inference substitutions to recurse.
        let root = if let SourceType::Var(id) = ty {
            context.get(id).ok_or(INVALID)?
        } else {
            ty
        };
        let SourceType::Nominal(name, _) = root else {
            if self.active() {
                // This path still copies an atom through the ordinary closed
                // substitution helper, including a complete pointer spelling.
                let mut work = self.work.borrow_mut();
                work.charge(1)?;
                charge_source(root, 0, &mut work)?;
            }
            return Ok(());
        };
        let product = if self.active() {
            true
        } else {
            !checked
                .program
                .types
                .iter()
                .find(|d| d.name == *name)
                .is_some_and(all_nullary)
        };
        if !product {
            return Ok(());
        }
        self.catalog(checked)?;
        fn walk(
            ty: &SourceType,
            context: &super::super::instances::Substitution,
            depth: usize,
            work: &mut Budget,
        ) -> Result<(), String> {
            work.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            match ty {
                SourceType::Var(id) => charge_source(context.get(id).ok_or(INVALID)?, depth, work),
                SourceType::Nominal(name, args) => {
                    work.charge(name.len())?;
                    work.charge(args.len())?;
                    for arg in args {
                        walk(arg, context, depth + 1, work)?;
                    }
                    Ok(())
                }
                leaf => charge_atom(leaf, work),
            }
        }
        walk(ty, context, 0, &mut self.work.borrow_mut())
    }
}

#[derive(Clone, Copy)]
enum Environment<'a> {
    Logical(&'a [LogicalType]),
    Closed(&'a [Type]),
}
impl<'a> Environment<'a> {
    fn get(self, index: usize) -> Result<View<'a>, String> {
        match self {
            Self::Logical(args) => args
                .get(index)
                .map(View::Logical)
                .ok_or_else(|| INVALID.into()),
            Self::Closed(args) => args
                .get(index)
                .copied()
                .map(View::Closed)
                .ok_or_else(|| INVALID.into()),
        }
    }
}
#[derive(Clone, Copy)]
enum View<'a> {
    Logical(&'a LogicalType),
    // Closed nominal arguments must not smuggle a symbolic runtime child.
    Closed(Type),
    Runtime(Type),
    Schema(&'a Schema, Environment<'a>),
}
#[derive(Clone, Copy)]
enum Arguments<'a> {
    Environment(Environment<'a>),
    Schema(&'a [Schema], Environment<'a>),
}
impl<'a> Arguments<'a> {
    fn len(self) -> usize {
        match self {
            Self::Environment(Environment::Logical(args)) => args.len(),
            Self::Environment(Environment::Closed(args)) => args.len(),
            Self::Schema(args, _) => args.len(),
        }
    }
    fn get(self, index: usize) -> Result<View<'a>, String> {
        match self {
            Self::Environment(env) => env.get(index),
            Self::Schema(args, env) => args
                .get(index)
                .map(|arg| View::Schema(arg, env))
                .ok_or_else(|| INVALID.into()),
        }
    }
}
#[derive(Clone, Copy)]
enum Identity<'a> {
    Atom(Atom<'a>),
    Opaque(&'a OpaqueId),
    Nominal(usize, Arguments<'a>),
}
#[derive(Clone, Copy)]
enum Atom<'a> {
    Source(&'a SourceType),
    Runtime(Type),
}

struct Verifier<'a, 'b> {
    meta: &'a Metadata,
    program: &'a TypedMain,
    catalog: &'a Catalog,
    budget: &'b mut Budget,
    heights: Vec<Option<usize>>,
}
impl<'a> Verifier<'a, '_> {
    fn runtime_identity(&mut self, ty: Type, closed: bool) -> Result<Identity<'a>, String> {
        let Type::Adt { id, owned } = ty else {
            return Ok(Identity::Atom(Atom::Runtime(ty)));
        };
        let adt = self.program.adts.get(id).ok_or(INVALID)?;
        if adt.origin != AdtOrigin::Source || adt.owned != owned {
            return Err(INVALID.into());
        }
        let (definition, arguments) = if let Some(instance) = &adt.symbolic {
            if closed || !adt.arguments.is_empty() {
                return Err(INVALID.into());
            }
            (
                instance.definition,
                Environment::Logical(&instance.arguments),
            )
        } else {
            (
                self.catalog.definition(&adt.name, self.budget)?,
                Environment::Closed(&adt.arguments),
            )
        };
        let decl = self.meta.declarations.get(definition).ok_or(INVALID)?;
        self.budget.charge(decl.name.len())?;
        self.budget.charge(adt.name.len())?;
        let arguments = Arguments::Environment(arguments);
        if adt.name != decl.name || arguments.len() != decl.params.len() {
            return Err(INVALID.into());
        }
        Ok(Identity::Nominal(definition, arguments))
    }

    fn identity(&mut self, view: View<'a>) -> Result<Identity<'a>, String> {
        self.budget.charge(1)?;
        match view {
            View::Logical(LogicalType::Atom(atom)) | View::Schema(Schema::Atom(atom), _) => {
                Ok(Identity::Atom(Atom::Source(atom)))
            }
            View::Logical(LogicalType::Opaque(id)) => {
                if self.meta.domain.as_ref() != Some(&id.domain)
                    || self.meta.binders.get(id.index).is_none()
                {
                    return Err(INVALID.into());
                }
                Ok(Identity::Opaque(id))
            }
            View::Logical(LogicalType::Nominal {
                definition,
                arguments,
            }) => {
                if self.catalog.arities.get(*definition) != Some(&arguments.len()) {
                    return Err(INVALID.into());
                }
                Ok(Identity::Nominal(
                    *definition,
                    Arguments::Environment(Environment::Logical(arguments)),
                ))
            }
            View::Schema(Schema::Parameter(index), env) => self.identity(env.get(*index)?),
            View::Schema(Schema::Nominal(definition, args), env) => {
                Ok(Identity::Nominal(*definition, Arguments::Schema(args, env)))
            }
            View::Closed(ty) => self.runtime_identity(ty, true),
            View::Runtime(ty) => self.runtime_identity(ty, false),
        }
    }

    fn source_atom_matches(&mut self, expected: &SourceType, actual: Type) -> Result<bool, String> {
        charge_atom(expected, self.budget)?;
        atom(expected)?;
        Ok(match (expected, actual) {
            (SourceType::Int, Type::Int)
            | (SourceType::Bool, Type::Bool)
            | (SourceType::Unit, Type::Unit)
            | (SourceType::StaticStr, Type::StaticStr)
            | (SourceType::U8x16, Type::U8x16)
            | (SourceType::CU8, Type::CU8)
            | (SourceType::CU32, Type::CU32)
            | (SourceType::CU64, Type::CU64)
            | (SourceType::CF32, Type::CF32)
            | (SourceType::SystemBoxI32, Type::OwnedI32)
            | (SourceType::SystemBytes, Type::OwnedBytes) => true,
            (SourceType::Bytes(a), Type::Bytes(b)) => *a == b && b <= 256,
            (SourceType::CPointer { pointee, is_const }, Type::RawPointer(id)) => {
                let actual = self
                    .program
                    .native
                    .pointers
                    .get(id.0 as usize)
                    .ok_or(INVALID)?;
                self.budget.charge(actual.pointee.len())?;
                actual.pointee == *pointee && actual.is_const == *is_const
            }
            _ => false,
        })
    }

    fn equal_atoms(&mut self, left: Atom<'a>, right: Atom<'a>) -> Result<bool, String> {
        match (left, right) {
            (Atom::Source(a), Atom::Source(b)) => {
                charge_atom(a, self.budget)?;
                charge_atom(b, self.budget)?;
                atom(a)?;
                atom(b)?;
                Ok(a == b)
            }
            (Atom::Source(a), Atom::Runtime(b)) | (Atom::Runtime(b), Atom::Source(a)) => {
                self.source_atom_matches(a, b)
            }
            (Atom::Runtime(a), Atom::Runtime(b)) => {
                // Validate even an equal closed pointer identity before using it
                // as a logical argument of a physically phantom declaration.
                for ty in [a, b] {
                    match ty {
                        Type::RawPointer(id) => {
                            let pointer = self
                                .program
                                .native
                                .pointers
                                .get(id.0 as usize)
                                .ok_or(INVALID)?;
                            self.budget.charge(pointer.pointee.len())?;
                            super::super::foreign::validate_pointer(&pointer.pointee)?;
                        }
                        Type::Bytes(n) if n > 256 => return Err(INVALID.into()),
                        Type::Adt { .. } => return Err(INVALID.into()),
                        _ => {}
                    }
                }
                Ok(a == b)
            }
        }
    }

    fn equal(&mut self, expected: View<'a>, actual: View<'a>, depth: usize) -> Result<(), String> {
        self.budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        let expected = self.identity(expected)?;
        let actual = self.identity(actual)?;
        match (expected, actual) {
            (Identity::Atom(a), Identity::Atom(b)) if self.equal_atoms(a, b)? => Ok(()),
            (Identity::Opaque(a), Identity::Opaque(b)) if a == b => Ok(()),
            (Identity::Nominal(a, expected), Identity::Nominal(b, actual))
                if a == b && expected.len() == actual.len() =>
            {
                self.budget.charge(expected.len())?;
                for index in 0..expected.len() {
                    self.equal(expected.get(index)?, actual.get(index)?, depth + 1)?;
                }
                Ok(())
            }
            _ => Err(INVALID.into()),
        }
    }

    fn field(
        &mut self,
        expected: View<'a>,
        actual: Type,
        parent: usize,
        depth: usize,
    ) -> Result<usize, String> {
        self.budget.charge(1)?;
        if depth > MAX_DEPTH || actual.is_owner() {
            return Err(INVALID.into());
        }
        // Opaque leaves cannot match any runtime atom. The nominal identity
        // comparison includes every logical argument, not only stored ones.
        self.equal(expected, View::Runtime(actual), 0)?;
        if let Type::Adt { id, .. } = actual {
            if id >= parent {
                return Err(INVALID.into());
            }
            return self.entry(id, depth + 1);
        }
        Ok(0)
    }

    fn entry(&mut self, id: usize, depth: usize) -> Result<usize, String> {
        self.budget.charge(1)?;
        if depth > MAX_DEPTH {
            return Err(LIMIT.into());
        }
        if let Some(height) = *self.heights.get(id).ok_or(INVALID)? {
            return Ok(height);
        }
        let adt = &self.program.adts[id];
        if adt.owned {
            return Err(INVALID.into());
        }
        let Identity::Nominal(definition, Arguments::Environment(environment)) =
            self.runtime_identity(Type::Adt { id, owned: false }, false)?
        else {
            return Err(INVALID.into());
        };
        let decl = &self.meta.declarations[definition];
        if adt.variants.len() != decl.variants.len() {
            return Err(INVALID.into());
        }
        match (&decl.record_fields, &adt.record_fields) {
            (None, None) => {}
            (Some(expected), Some(actual)) if expected.len() == actual.len() => {
                self.budget.charge(expected.len())?;
                for (a, b) in expected.iter().zip(actual) {
                    self.budget.charge(a.len())?;
                    self.budget.charge(b.len())?;
                    if a != b {
                        return Err(INVALID.into());
                    }
                }
            }
            _ => return Err(INVALID.into()),
        }
        self.budget.charge(decl.variants.len())?;
        let mut height = 1;
        for (index, (expected, actual)) in decl.variants.iter().zip(&adt.variants).enumerate() {
            if expected.fields.len() != actual.fields.len() || actual.fields.len() > 4096 {
                return Err(INVALID.into());
            }
            self.budget.charge(expected.name.len())?;
            self.budget.charge(actual.name.len())?;
            if expected.name != actual.name {
                return Err(INVALID.into());
            }
            self.budget.charge(actual.fields.len())?;
            for (schema, &ty) in self.catalog.fields[definition][index]
                .iter()
                .zip(&actual.fields)
            {
                let child_height = self.field(View::Schema(schema, environment), ty, id, depth)?;
                height = height.max(child_height.checked_add(1).ok_or(LIMIT)?);
                if height > MAX_DEPTH {
                    return Err(LIMIT.into());
                }
            }
        }
        // Cache only after all descendants, identity, kind and ordered fields
        // have been checked. A matching child name alone proves nothing.
        self.heights[id] = Some(height);
        Ok(height)
    }
}

pub(in super::super) fn verify(
    meta: &Metadata,
    program: &TypedMain,
    catalog: &Catalog,
    budget: &mut Budget,
) -> Result<(), String> {
    budget.charge(program.adts.len())?;
    let mut verifier = Verifier {
        meta,
        program,
        catalog,
        budget,
        heights: vec![None; program.adts.len()],
    };
    for (id, adt) in program.adts.iter().enumerate() {
        if adt.symbolic.is_some() {
            verifier.entry(id, 0)?;
        }
    }
    Ok(())
}

impl Catalog {
    pub(in super::super) fn instance(
        &self,
        meta: &Metadata,
        name: &str,
        args: &[SourceType],
        budget: &mut Budget,
    ) -> Result<SymbolicInstance, String> {
        let definition = self.definition(name, budget)?;
        if args.len() != self.arities[definition] {
            return Err(INVALID.into());
        }
        fn convert(
            ty: &SourceType,
            meta: &Metadata,
            catalog: &Catalog,
            depth: usize,
            budget: &mut Budget,
        ) -> Result<LogicalType, String> {
            budget.charge(1)?;
            if depth > MAX_DEPTH {
                return Err(LIMIT.into());
            }
            Ok(match ty {
                SourceType::Var(index) => LogicalType::Opaque(meta.opaque_id(*index)?),
                SourceType::Nominal(name, args) => {
                    let definition = catalog.definition(name, budget)?;
                    if args.len() != catalog.arities[definition] {
                        return Err(INVALID.into());
                    }
                    budget.charge(args.len())?;
                    LogicalType::Nominal {
                        definition,
                        arguments: args
                            .iter()
                            .map(|arg| convert(arg, meta, catalog, depth + 1, budget))
                            .collect::<Result<_, _>>()?,
                    }
                }
                atom_type => {
                    charge_atom(atom_type, budget)?;
                    atom(atom_type)?;
                    LogicalType::Atom(atom_type.clone())
                }
            })
        }
        budget.charge(args.len())?;
        let arguments = args
            .iter()
            .map(|arg| convert(arg, meta, self, 0, budget))
            .collect::<Result<Vec<_>, _>>()?;
        let mut dependencies = self.dependencies[definition];
        let mut binders = HashSet::new();
        for arg in &arguments {
            dependencies.include(meta.check_type(
                arg,
                &self.dependencies,
                &mut binders,
                0,
                budget,
            )?);
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
}

pub(in super::super) fn charge_table(decl: &TypeDecl, budget: &mut Budget) -> Result<(), String> {
    budget.charge(decl.name.len())?;
    if let Some(fields) = &decl.record_fields {
        budget.charge(fields.len())?;
        for name in fields {
            budget.charge(name.len())?;
        }
    }
    budget.charge(decl.variants.len())?;
    for variant in &decl.variants {
        if variant.fields.len() > 4096 {
            return Err(LIMIT.into());
        }
        budget.charge(variant.name.len())?;
        budget.charge(variant.fields.len())?;
    }
    Ok(())
}

pub(in super::super) fn charge_logical(
    ty: &LogicalType,
    depth: usize,
    budget: &mut Budget,
) -> Result<(), String> {
    budget.charge(1)?;
    if depth > MAX_DEPTH {
        return Err(LIMIT.into());
    }
    match ty {
        LogicalType::Atom(leaf) => charge_atom(leaf, budget),
        LogicalType::Opaque(_) => Ok(()),
        LogicalType::Nominal { arguments, .. } => {
            budget.charge(arguments.len())?;
            for arg in arguments {
                charge_logical(arg, depth + 1, budget)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Vec<TypeDecl> {
        crate::parser::parse("type Flag<a> = Off | On\ntype Mark<a> = Mark(i32)\ntype Option<a> = None | Some(a)\ntype Link<a> = Link(a, Flag<a>)\ntype Erased<a> = Erased(Flag<Option<a>>)\ntype Native<a> = Native(u32)\ntype Owner<a> = Owner(SystemBoxI32)").unwrap().types
    }
    fn nominal(name: &str, argument: SourceType) -> SourceType {
        SourceType::Nominal(name.into(), vec![argument])
    }

    #[test]
    fn active_admission_reserves_closed_pointer_copies_before_substitution() {
        let parsed =
            crate::parser::parse("type Mark<a> = Mark(i32)\nfn main() { Mark(42) }").unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let ty = SourceType::CPointer {
            pointee: "ExactPointee".into(),
            is_const: true,
        };
        let mut context = super::super::super::instances::Substitution::new();
        context.insert(7, ty.clone());
        for input in [&ty, &SourceType::Var(7)] {
            let admission = Admission::default();
            admission.catalog(&checked).unwrap();
            let bytes = "ExactPointee".len();
            *admission.work.borrow_mut() = Budget(MAX_WORK - bytes - 2);
            admission
                .charge_substitution(&checked, input, &context)
                .unwrap();
            assert_eq!(admission.work.borrow().0, MAX_WORK);
            *admission.work.borrow_mut() = Budget(MAX_WORK - bytes - 1);
            assert_eq!(
                admission
                    .charge_substitution(&checked, input, &context)
                    .unwrap_err(),
                LIMIT
            );
        }
    }

    #[test]
    fn retained_name_index_charges_copy_and_hash_separately() {
        let mut declarations = crate::parser::parse("type A = Tag").unwrap().types;
        let mut before = Budget::default();
        Catalog::new(&declarations, &mut before).unwrap();
        declarations[0].name = "LongResolvedDeclarationIdentity".into();
        let mut after = Budget::default();
        Catalog::new(&declarations, &mut after).unwrap();
        // One validation hash, then the retained index's copy and hash.
        assert_eq!(after.0 - before.0, 3 * (declarations[0].name.len() - 1));
    }

    #[test]
    fn physical_demands_and_full_dependencies_are_distinct() {
        let catalog = Catalog::new(&schema(), &mut Budget::default()).unwrap();
        for id in [0, 1, 4, 5] {
            assert!(catalog.demands[id].parameters.is_empty());
        }
        for id in [2, 3] {
            assert_eq!(catalog.demands[id].parameters.get(&0), Some(&0));
        }
        assert!(catalog.dependencies[5].native);
        assert!(catalog.dependencies[6].owner);
        assert!(catalog.demands[6].fixed_owner);
        for ty in [
            nominal("Mark", SourceType::Var(0)),
            nominal("Erased", SourceType::Var(0)),
            nominal("Link", nominal("Flag", SourceType::Var(0))),
            nominal("Option", nominal("Flag", SourceType::Var(0))),
        ] {
            catalog
                .validate_type(&ty, 0, &mut Budget::default())
                .unwrap();
            catalog.finite(&ty, 0, &mut Budget::default()).unwrap();
        }
        for ty in [
            SourceType::Var(0),
            nominal("Option", SourceType::Var(0)),
            nominal("Link", nominal("Option", SourceType::Var(0))),
            nominal("Owner", SourceType::Var(0)),
        ] {
            assert!(catalog
                .finite(&ty, 0, &mut Budget::default())
                .unwrap_err()
                .contains("UNSUPPORTED"));
        }
    }

    #[test]
    fn catalog_work_is_precharged_with_exact_boundary() {
        let declarations = schema();
        let mut measured = Budget::default();
        Catalog::new(&declarations, &mut measured).unwrap();
        let mut exact = Budget(MAX_WORK - measured.0);
        Catalog::new(&declarations, &mut exact).unwrap();
        assert_eq!(exact.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - measured.0 + 1);
        assert!(matches!(Catalog::new(&declarations, &mut short), Err(e) if e == LIMIT));
        let mut malformed = declarations.clone();
        malformed[0].variants[0].fields.push(TypeExpr {
            name: "Flag".into(),
            args: vec![TypeExpr {
                name: "a".into(),
                args: Vec::new(),
            }],
        });
        assert!(Catalog::new(&malformed, &mut Budget::default()).is_err());
    }

    #[test]
    fn repeated_substitution_charges_copied_argument_trees_before_growth() {
        let declarations = crate::parser::parse("type Pair<a> = Pair(a,a)\ntype Flag<a> = Off")
            .unwrap()
            .types;
        let catalog = Catalog::new(&declarations, &mut Budget::default()).unwrap();
        let args = vec![nominal("Flag", nominal("Flag", SourceType::Var(0)))];
        let mut measured = Budget::default();
        let fields = catalog
            .fields(0, &args, &declarations, &mut measured)
            .unwrap();
        assert_eq!(fields[0], [args[0].clone(), args[0].clone()]);
        let mut exact = Budget(MAX_WORK - measured.0);
        assert_eq!(
            catalog.fields(0, &args, &declarations, &mut exact).unwrap(),
            fields
        );
        assert_eq!(exact.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - measured.0 + 1);
        assert_eq!(
            catalog
                .fields(0, &args, &declarations, &mut short)
                .unwrap_err(),
            LIMIT
        );
    }

    #[test]
    fn product_representation_proof_has_exact_cumulative_boundary() {
        let parsed = crate::parser::parse(
            "type Fixed = Fixed(i32)\ntype Mark<a> = Mark(Fixed)\nfn main() { Mark(Fixed(42)) }",
        )
        .unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let program = super::super::super::lower_main(&checked).unwrap();
        let catalog = Catalog::new(&program.logical.declarations, &mut Budget::default()).unwrap();
        let mut measured = Budget::default();
        verify(&program.logical, &program, &catalog, &mut measured).unwrap();
        let mut exact = Budget(MAX_WORK - measured.0);
        verify(&program.logical, &program, &catalog, &mut exact).unwrap();
        assert_eq!(exact.0, MAX_WORK);
        let mut short = Budget(MAX_WORK - measured.0 + 1);
        assert_eq!(
            verify(&program.logical, &program, &catalog, &mut short).unwrap_err(),
            LIMIT
        );
    }

    #[test]
    fn physical_depth_composes_schema_and_actual_argument_spines() {
        let declarations = crate::parser::parse("type Boxed<a> = Boxed(a)\ntype Flag<a> = Off")
            .unwrap()
            .types;
        let catalog = Catalog::new(&declarations, &mut Budget::default()).unwrap();
        let mut ty = nominal("Flag", SourceType::Var(0));
        for _ in 1..MAX_DEPTH {
            ty = nominal("Boxed", ty);
        }
        catalog.finite(&ty, 0, &mut Budget::default()).unwrap();
        ty = nominal("Boxed", ty);
        assert_eq!(
            catalog.finite(&ty, 0, &mut Budget::default()).unwrap_err(),
            LIMIT
        );
    }
}
