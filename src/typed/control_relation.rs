//! Canonical control correspondence. This is a mandatory relation, separate
//! from source and IR normal-return facts. Every retained source operation must
//! have a direct independently checked IR rule before either consumer runs.
use super::*;
use crate::typed::{instances, logical};
use std::collections::{BTreeMap, BTreeSet};

#[path = "control_correspondence.rs"]
mod correspondence;
#[path = "control_environment.rs"]
mod environment;
#[cfg(test)]
#[path = "independent_nonreturning_relation_tests.rs"]
mod independent_nonreturning_relation_tests;

#[derive(Debug)]
struct Instance {
    source: usize,
    scope: usize,
    params: Vec<Type>,
    result: Type,
    substitution: BTreeMap<usize, Type>,
}
#[derive(Debug)]
pub(in crate::typed) struct Frozen {
    domain: logical::Domain,
    binders: Vec<logical::OpaqueBinding>,
    witness: Witness,
    instances: Vec<Instance>,
    entry: usize,
}

impl SourceFacts<'_, '_> {
    pub(in crate::typed) fn freeze(
        self,
        instances: &instances::Instances,
    ) -> Result<Frozen, String> {
        let mut budget = Budget::default();
        budget.charge(self.witness_work)?;
        budget.charge(instances.binders.len())?;
        budget.charge(instances.list.len())?;
        let mut frozen = Vec::with_capacity(instances.list.len());
        for instance in &instances.list {
            budget.charge(instance.params.len())?;
            let mut params = Vec::with_capacity(instance.params.len());
            for ty in &instance.params {
                params.push(copy_type(ty, &mut budget, 0)?);
            }
            let result = copy_type(&instance.result, &mut budget, 0)?;
            budget.charge(instance.substitution.len())?;
            let mut substitution = BTreeMap::new();
            for (&variable, ty) in &instance.substitution {
                budget.charge(instance.substitution.len().ilog2() as usize + 1)?;
                substitution.insert(variable, copy_type(ty, &mut budget, 0)?);
            }
            frozen.push(Instance {
                source: instance.source,
                scope: instance.scope,
                params,
                result,
                substitution,
            });
        }
        Ok(Frozen {
            domain: instances.domain.clone(),
            binders: instances.binders.clone(),
            witness: self.witness,
            instances: frozen,
            entry: instances.entry,
        })
    }
}

fn variables(
    ty: &Type,
    classes: &mut BTreeSet<usize>,
    budget: &mut Budget,
    depth: usize,
) -> Result<(), String> {
    budget.charge(1)?;
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    match ty {
        Type::Var(variable) => {
            budget.charge(classes.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            classes.insert(*variable);
        }
        Type::Nominal(name, args) => {
            budget.charge(name.len())?;
            budget.charge(args.len())?;
            for arg in args {
                variables(arg, classes, budget, depth + 1)?;
            }
        }
        Type::CPointer { pointee, .. } => budget.charge(pointee.len())?,
        _ => {}
    }
    Ok(())
}

fn substitute(
    ty: &Type,
    context: &BTreeMap<usize, Type>,
    budget: &mut Budget,
    depth: usize,
) -> Result<Type, String> {
    budget.charge(1)?;
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    Ok(match ty {
        Type::Var(id) => {
            budget.charge(context.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            copy_type(context.get(id).ok_or(INVALID)?, budget, depth)?
        }
        Type::Nominal(name, args) => {
            let name = budget.text(name)?;
            budget.charge(args.len())?;
            let mut result = Vec::with_capacity(args.len());
            for arg in args {
                result.push(substitute(arg, context, budget, depth + 1)?);
            }
            Type::Nominal(name, result)
        }
        _ => copy_type(ty, budget, depth)?,
    })
}

fn constrain(
    pattern: &Type,
    actual: &Type,
    quantified: &BTreeSet<usize>,
    context: &mut BTreeMap<usize, Type>,
    budget: &mut Budget,
    depth: usize,
) -> Result<(), String> {
    budget.charge(1)?;
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    budget.charge(quantified.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
    budget.charge(context.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
    match (pattern, actual) {
        (Type::Var(variable), actual)
            if quantified.contains(variable) || context.contains_key(variable) =>
        {
            budget.charge(context.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            let actual = copy_type(actual, budget, depth)?;
            if let Some(previous) = context.get(variable) {
                if previous != &actual {
                    return Err(INVALID.into());
                }
            } else {
                context.insert(*variable, actual);
            }
        }
        (Type::Nominal(a, aa), Type::Nominal(b, bb)) => {
            budget.charge(a.len().checked_add(b.len()).ok_or(LIMIT)?)?;
            if a != b || aa.len() != bb.len() {
                return Err(INVALID.into());
            }
            budget.charge(aa.len())?;
            for (a, b) in aa.iter().zip(bb) {
                constrain(a, b, quantified, context, budget, depth + 1)?;
            }
        }
        (a, b) => {
            // The copied bounded trees precharge this exact equality, including
            // pointer spellings. Source variable IDs never substitute themselves.
            let a = copy_type(a, budget, depth)?;
            let b = copy_type(b, budget, depth)?;
            if a != b {
                return Err(INVALID.into());
            }
        }
    }
    Ok(())
}

impl Frozen {
    pub(in crate::typed) fn verify_source(
        &self,
        checked: &CheckedProgram<'_>,
    ) -> Result<(), String> {
        let mut rebuilt = SourceBuilder::new(checked)?.build()?;
        self.charge(&mut rebuilt.budget)?;
        if self.witness != rebuilt.witness {
            return Err(INVALID.into());
        }
        Ok(())
    }

    fn freeze_remaining(
        &self,
        scope: usize,
        variables: &BTreeSet<usize>,
        context: &mut BTreeMap<usize, Type>,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.charge(self.binders.len())?;
        let mut own = self
            .binders
            .iter()
            .enumerate()
            .filter(|(_, binder)| binder.scope == scope);
        for variable in variables {
            budget.charge(variables.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            if context.contains_key(variable) {
                continue;
            }
            let (id, binder) = own.next().ok_or(INVALID)?;
            // Canonical ordinal is checked for every scope before this routine.
            budget.charge(1)?;
            if binder.scope != scope {
                return Err(INVALID.into());
            }
            context.insert(*variable, Type::Var(id));
        }
        if own.next().is_some() {
            return Err(INVALID.into());
        }
        Ok(())
    }

    fn signatures(&self, budget: &mut Budget) -> Result<(), String> {
        budget.charge(self.witness.nodes.len())?;
        if self.witness.nodes.iter().any(|node| node.elaborated_local) {
            return Err("E-TYPED-UNSUPPORTED: no-normal results do not combine with local shape elaboration".into());
        }
        if self.instances.len() > MAX_INSTANCES
            || self.entry >= self.instances.len()
            || self.instances[self.entry].scope != 0
        {
            return Err(INVALID.into());
        }
        budget.charge(self.instances.len())?;
        let mut scopes = vec![false; self.instances.len()];
        budget.charge(self.instances.len())?;
        let mut counts = vec![0; self.instances.len()];
        budget.charge(self.binders.len())?;
        for binder in &self.binders {
            let count = counts.get_mut(binder.scope).ok_or(INVALID)?;
            if binder.ordinal != *count {
                return Err(INVALID.into());
            }
            *count = count.checked_add(1).ok_or(LIMIT)?;
        }
        let mut source_functions = BTreeMap::new();
        budget.charge(self.witness.functions.len())?;
        for function in &self.witness.functions {
            let declared = self
                .witness
                .environment
                .function_names
                .get(function.source)
                .ok_or(INVALID)?;
            budget.charge(
                declared
                    .len()
                    .checked_add(function.name.len())
                    .ok_or(LIMIT)?,
            )?;
            if declared != &function.name {
                return Err(INVALID.into());
            }
            budget.charge(
                self.witness
                    .functions
                    .len()
                    .checked_add(1)
                    .ok_or(LIMIT)?
                    .ilog2() as usize
                    + 1,
            )?;
            if source_functions.insert(function.source, function).is_some() {
                return Err(INVALID.into());
            }
        }
        let mut keys = std::collections::HashSet::new();
        for (slot, instance) in self.instances.iter().enumerate() {
            budget.charge(instance.params.len().checked_add(1).ok_or(LIMIT)?)?;
            for ty in instance
                .params
                .iter()
                .chain(std::iter::once(&instance.result))
            {
                copy_type(ty, budget, 0)?;
            }
            if !keys.insert((instance.source, &instance.params, &instance.result)) {
                return Err(INVALID.into());
            }
            let used = scopes.get_mut(instance.scope).ok_or(INVALID)?;
            if *used {
                return Err(INVALID.into());
            }
            *used = true;
            budget
                .charge(source_functions.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            let source = source_functions.get(&instance.source).ok_or(INVALID)?;
            if source.params.len() != instance.params.len()
                || (slot == self.entry && (source.name != "main" || !instance.params.is_empty()))
            {
                return Err(INVALID.into());
            }
            let mut all_variables = BTreeSet::new();
            for (_, ty) in &source.params {
                variables(ty, &mut all_variables, budget, 0)?;
            }
            variables(&source.result, &mut all_variables, budget, 0)?;
            budget.charge(self.witness.nodes.len())?;
            for node in &self.witness.nodes {
                if node.source == instance.source {
                    variables(&node.ty, &mut all_variables, budget, 0)?;
                }
            }
            budget.charge(source.quantified.len())?;
            let mut quantified = BTreeSet::new();
            for variable in &source.quantified {
                budget.charge(
                    source.quantified.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1,
                )?;
                budget.charge(
                    all_variables.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1,
                )?;
                if !all_variables.contains(variable) || !quantified.insert(*variable) {
                    return Err(INVALID.into());
                }
            }
            let mut reconstructed = BTreeMap::new();
            if slot == self.entry {
                self.freeze_remaining(instance.scope, &all_variables, &mut reconstructed, budget)?;
            }
            for ((_, pattern), actual) in source.params.iter().zip(&instance.params) {
                constrain(pattern, actual, &quantified, &mut reconstructed, budget, 0)?;
            }
            constrain(
                &source.result,
                &instance.result,
                &quantified,
                &mut reconstructed,
                budget,
                0,
            )?;
            if slot != self.entry {
                self.freeze_remaining(instance.scope, &all_variables, &mut reconstructed, budget)?;
            }
            // Compare exact final signatures again after own-class freezing.
            for ((_, pattern), actual) in source.params.iter().zip(&instance.params) {
                if substitute(pattern, &reconstructed, budget, 0)? != copy_type(actual, budget, 0)?
                {
                    return Err(INVALID.into());
                }
            }
            if substitute(&source.result, &reconstructed, budget, 0)?
                != copy_type(&instance.result, budget, 0)?
            {
                return Err(INVALID.into());
            }
            budget.charge(instance.substitution.len())?;
            for ty in instance.substitution.values() {
                copy_type(ty, budget, 0)?;
            }
            if reconstructed != instance.substitution {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }

    fn charge(&self, budget: &mut Budget) -> Result<(), String> {
        // Charge the entire retained witness before equality or any relation
        // scan. No stored work counter or may-return annotation is trusted.
        budget.charge(self.binders.len())?;
        budget.charge(self.instances.len())?;
        for instance in &self.instances {
            budget.charge(instance.params.len())?;
            for ty in instance
                .params
                .iter()
                .chain(std::iter::once(&instance.result))
            {
                copy_type(ty, budget, 0)?;
            }
            budget.charge(instance.substitution.len())?;
            for ty in instance.substitution.values() {
                copy_type(ty, budget, 0)?;
            }
        }
        charge_witness(&self.witness, budget)
    }
}

fn charge_witness(witness: &Witness, budget: &mut Budget) -> Result<(), String> {
    fn text(value: &str, budget: &mut Budget) -> Result<(), String> {
        budget.charge(1)?;
        budget.charge(value.len())
    }
    fn external(ty: &crate::ast::ExternType, budget: &mut Budget) -> Result<(), String> {
        budget.charge(1)?;
        if let crate::ast::ExternType::Pointer { pointee, .. } = ty {
            text(pointee, budget)?;
        }
        Ok(())
    }
    fn schema(ty: &crate::ast::TypeExpr, depth: usize, budget: &mut Budget) -> Result<(), String> {
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        text(&ty.name, budget)?;
        budget.charge(ty.args.len())?;
        for arg in &ty.args {
            schema(arg, depth + 1, budget)?;
        }
        Ok(())
    }
    fn reference(reference: &Reference, budget: &mut Budget) -> Result<(), String> {
        budget.charge(1)?;
        if let Reference::Global(name) = reference {
            text(name, budget)?;
        }
        Ok(())
    }
    fn path(parts: &[String], budget: &mut Budget) -> Result<(), String> {
        budget.charge(parts.len())?;
        for part in parts {
            text(part, budget)?;
        }
        Ok(())
    }
    let environment = &witness.environment;
    for list in [
        &environment.native.includes,
        &environment.native.links,
        &environment.native.packages,
        &environment.function_names,
    ] {
        path(list, budget)?;
    }
    budget.charge(environment.constants.len())?;
    for constant in &environment.constants {
        text(&constant.name, budget)?;
        external(&constant.ty, budget)?;
    }
    if let Some(allocation) = &environment.allocation {
        for name in [
            &allocation.result_type,
            &allocation.ok_constructor,
            &allocation.err_constructor,
            &allocation.error_type,
            &allocation.invalid_length_constructor,
            &allocation.size_overflow_constructor,
            &allocation.out_of_memory_constructor,
        ] {
            text(name, budget)?;
        }
    }
    budget.charge(environment.externs.len())?;
    for function in &environment.externs {
        text(&function.name, budget)?;
        budget.charge(function.params.len())?;
        for param in &function.params {
            text(&param.name, budget)?;
            external(&param.ty, budget)?;
        }
        external(&function.result, budget)?;
    }
    budget.charge(environment.types.len())?;
    for declaration in &environment.types {
        text(&declaration.name, budget)?;
        path(&declaration.params, budget)?;
        if let Some(fields) = &declaration.record_fields {
            path(fields, budget)?;
        }
        budget.charge(declaration.variants.len())?;
        for variant in &declaration.variants {
            text(&variant.name, budget)?;
            budget.charge(variant.fields.len())?;
            for field in &variant.fields {
                schema(field, 0, budget)?;
            }
        }
    }
    budget.charge(witness.functions.len())?;
    for function in &witness.functions {
        text(&function.name, budget)?;
        budget.charge(function.quantified.len())?;
        budget.charge(function.params.len())?;
        for (_, ty) in &function.params {
            copy_type(ty, budget, 0)?;
        }
        copy_type(&function.result, budget, 0)?;
    }
    budget.charge(witness.bindings.len())?;
    for binding in &witness.bindings {
        text(&binding.name, budget)?;
    }
    budget.charge(witness.patterns.len())?;
    for pattern in &witness.patterns {
        match pattern {
            SourcePattern::Pending => return Err(INVALID.into()),
            SourcePattern::Constructor(name, fields) => {
                text(name, budget)?;
                budget.charge(fields.len())?;
            }
            _ => {}
        }
    }
    budget.charge(witness.nodes.len())?;
    for node in &witness.nodes {
        copy_type(&node.ty, budget, 0)?;
        match &node.kind {
            SourceKind::Pending => return Err(INVALID.into()),
            SourceKind::Str(value) | SourceKind::CStr(value) => text(value, budget)?,
            SourceKind::Var(value) | SourceKind::Move(value) | SourceKind::Inout(value) => {
                reference(value, budget)?
            }
            SourceKind::Field(value, parts)
            | SourceKind::InoutField(value, parts)
            | SourceKind::FieldAssign(value, parts, _) => {
                reference(value, budget)?;
                path(parts, budget)?;
            }
            SourceKind::Assign(value, _) => reference(value, budget)?,
            SourceKind::Record(name, fields) => {
                text(name, budget)?;
                budget.charge(fields.len())?;
                for (name, _) in fields {
                    text(name, budget)?;
                }
            }
            SourceKind::Call(target, modes, args) => {
                if let CallTarget::Other(name) = target {
                    text(name, budget)?;
                }
                budget.charge(modes.len())?;
                budget.charge(args.len())?;
            }
            SourceKind::Match(_, arms) => budget.charge(arms.len())?,
            _ => {}
        }
    }
    Ok(())
}

impl Frozen {
    /// Reconstruct the conservative source equation system from frozen syntax.
    /// No source-flow bit is retained as a trusted certificate.
    fn source_returns(&self, budget: &mut Budget) -> Result<Vec<bool>, String> {
        let mut circuit = Circuit::default();
        budget.charge(self.witness.functions.len())?;
        let mut functions = HashMap::new();
        for function in &self.witness.functions {
            budget.charge(1)?;
            if functions
                .insert(function.source, circuit.reserve(budget)?)
                .is_some()
            {
                return Err(INVALID.into());
            }
        }
        budget.charge(self.witness.nodes.len())?;
        let mut nodes = Vec::with_capacity(self.witness.nodes.len());
        for _ in &self.witness.nodes {
            nodes.push(circuit.reserve(budget)?);
        }
        let node = |id: NodeId| nodes.get(id.0).copied().ok_or(INVALID.to_string());
        for (id, expression) in self.witness.nodes.iter().enumerate() {
            budget.charge(1)?;
            let inputs = match &expression.kind {
                SourceKind::Pending => return Err(INVALID.into()),
                SourceKind::Binary(BinaryOp::And | BinaryOp::Or, left, _)
                | SourceKind::While(left, _) => {
                    budget.charge(1)?;
                    vec![node(*left)?]
                }
                SourceKind::Binary(_, left, right) | SourceKind::Let(_, left, right) => {
                    budget.charge(2)?;
                    vec![node(*left)?, node(*right)?]
                }
                SourceKind::Assign(_, value) | SourceKind::FieldAssign(_, _, value) => {
                    budget.charge(1)?;
                    vec![node(*value)?]
                }
                SourceKind::If(condition, yes, no) => {
                    let arms = circuit.any([node(*yes)?, node(*no)?].into_iter(), budget)?;
                    budget.charge(2)?;
                    vec![node(*condition)?, arms]
                }
                SourceKind::Match(value, arms) => {
                    if arms.is_empty() {
                        return Err(INVALID.into());
                    }
                    budget.charge(arms.len())?;
                    let mut values = Vec::with_capacity(arms.len());
                    for (_, arm) in arms {
                        values.push(node(*arm)?);
                    }
                    let arms = circuit.any(values.into_iter(), budget)?;
                    budget.charge(2)?;
                    vec![node(*value)?, arms]
                }
                SourceKind::Call(target, _, args) => {
                    budget.charge(args.len().checked_add(1).ok_or(LIMIT)?)?;
                    let mut inputs = Vec::with_capacity(args.len() + 1);
                    for arg in args {
                        inputs.push(node(*arg)?);
                    }
                    if let CallTarget::Function(source) = target {
                        inputs.push(*functions.get(source).ok_or(INVALID)?);
                    }
                    inputs
                }
                SourceKind::Record(_, fields) => {
                    budget.charge(fields.len())?;
                    let mut inputs = Vec::with_capacity(fields.len());
                    for (_, field) in fields {
                        inputs.push(node(*field)?);
                    }
                    inputs
                }
                SourceKind::Int(_)
                | SourceKind::Bool(_)
                | SourceKind::Str(_)
                | SourceKind::CStr(_)
                | SourceKind::Unit
                | SourceKind::Var(_)
                | SourceKind::Move(_)
                | SourceKind::Inout(_)
                | SourceKind::InoutField(_, _)
                | SourceKind::Field(_, _) => Vec::new(),
            };
            circuit.define(nodes[id], Operator::All, inputs.into_iter(), budget)?;
        }
        budget.charge(self.witness.functions.len())?;
        for function in &self.witness.functions {
            circuit.define(
                *functions.get(&function.source).ok_or(INVALID)?,
                Operator::All,
                [node(function.body)?].into_iter(),
                budget,
            )?;
        }
        let values = circuit.solve(budget)?;
        budget.charge(nodes.len())?;
        Ok(nodes.into_iter().map(|node| values[node.0]).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freeze(source: &str) -> Frozen {
        let parsed = crate::parser::parse(source).unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let (_, instances) = crate::typed::lower_main_traced(&checked, None, None).unwrap();
        let frozen = SourceFacts::build(&checked)
            .unwrap()
            .freeze(&instances)
            .unwrap();
        frozen.verify_source(&checked).unwrap();
        frozen
    }
    fn signatures(frozen: &Frozen) -> Result<(), String> {
        let mut budget = Budget::default();
        frozen.charge(&mut budget)?;
        frozen.signatures(&mut budget)
    }
    #[test]
    fn signature_relation_reconstructs_closed_quantified_and_weak_classes() {
        for source in [
            "fn identity(x) { x } fn main() { identity(42) }",
            "type Flag<a> = Off | On fn same(x) { x } fn main() { let value = same(Off); same(value) }",
            "type Flag<a> = Off | On fn again(n, value) { if n == 0 { value } else { again(n - 1, value) } } fn main() { again(2, Off) }",
            "fn again(n) { again(n) + 1 } fn main() { again(0) }",
        ] {
            let frozen = freeze(source);
            signatures(&frozen).unwrap_or_else(|error| panic!("{error}\n{source}"));
        }
    }
    #[test]
    fn signature_relation_rejects_missing_extra_or_consistently_altered_maps() {
        let mut frozen = freeze("fn identity(x) { x } fn main() { identity(42) }");
        let slot = frozen
            .instances
            .iter()
            .position(|i| !i.substitution.is_empty())
            .unwrap();
        let saved = frozen.instances[slot].substitution.clone();
        frozen.instances[slot].substitution.clear();
        assert!(signatures(&frozen).is_err());
        frozen.instances[slot].substitution = saved;
        frozen.instances[slot]
            .substitution
            .insert(usize::MAX, Type::Int);
        assert!(signatures(&frozen).is_err());
        frozen.instances[slot].substitution.remove(&usize::MAX);
        for ty in frozen.instances[slot].substitution.values_mut() {
            *ty = Type::Bool;
        }
        assert!(signatures(&frozen).is_err());
    }
    #[test]
    fn signature_relation_checks_entry_scope_own_ordinals_and_source_quantifiers() {
        let mut frozen = freeze("type Flag<a> = Off | On fn main() { Off }");
        assert_eq!(frozen.domain, frozen.domain.clone());
        frozen.binders[0].ordinal = 1;
        assert!(signatures(&frozen).is_err());
        frozen.binders[0].ordinal = 0;
        frozen.instances[frozen.entry].scope = 1;
        assert!(signatures(&frozen).is_err());
        let mut frozen = freeze("fn identity(x) { x } fn main() { identity(42) }");
        let source = frozen
            .witness
            .functions
            .iter_mut()
            .find(|f| f.name == "identity")
            .unwrap();
        assert!(!source.quantified.is_empty());
        source.quantified.clear();
        assert!(signatures(&frozen).is_err());
    }
    #[test]
    fn retained_generalized_initializers_are_explicitly_outside_this_slice() {
        let frozen = freeze(
            "type Flag<a> = Off | On fn same(x) { x } fn main() { let value = Off; same(value) }",
        );
        assert!(signatures(&frozen)
            .unwrap_err()
            .starts_with("E-TYPED-UNSUPPORTED:"));
    }
}
