//! Direct structural relation, not a replay of lowerer decisions or a flow hash.
use super::*;
use crate::typed::{
    AdtOrigin, CallArg, Computation, CopyPattern, ExprKind, ExprNode, FieldPlace, LocalId,
    LogicalType, NoNormalExpr, NoNormalKind, ResultRepr, Type as RuntimeType, TypedMain,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
enum Actual<'a> {
    Value(&'a ExprNode),
    NoNormal(&'a NoNormalExpr),
}

impl Frozen {
    pub(super) fn logical_source(
        &self,
        ty: &LogicalType,
        budget: &mut Budget,
        depth: usize,
    ) -> Result<Type, String> {
        budget.charge(1)?;
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        Ok(match ty {
            LogicalType::Atom(ty) if !matches!(ty, Type::Var(_) | Type::Nominal(..)) => {
                copy_type(ty, budget, depth)?
            }
            LogicalType::Opaque(id)
                if id.domain == self.domain && id.index < self.binders.len() =>
            {
                Type::Var(id.index)
            }
            LogicalType::Nominal {
                definition,
                arguments,
            } => {
                let declaration = self
                    .witness
                    .environment
                    .types
                    .get(*definition)
                    .ok_or(INVALID)?;
                if declaration.params.len() != arguments.len() {
                    return Err(INVALID.into());
                }
                budget.charge(arguments.len())?;
                let mut args = Vec::with_capacity(arguments.len());
                for arg in arguments {
                    args.push(self.logical_source(arg, budget, depth + 1)?);
                }
                Type::Nominal(budget.text(&declaration.name)?, args)
            }
            _ => return Err(INVALID.into()),
        })
    }
    pub(super) fn runtime_type(
        &self,
        ty: RuntimeType,
        program: &TypedMain,
        budget: &mut Budget,
        depth: usize,
    ) -> Result<Type, String> {
        budget.charge(1)?;
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        Ok(match ty {
            RuntimeType::Int => Type::Int,
            RuntimeType::Bool => Type::Bool,
            RuntimeType::Unit => Type::Unit,
            RuntimeType::StaticStr => Type::StaticStr,
            RuntimeType::U8x16 => Type::U8x16,
            RuntimeType::Bytes(n) if n <= 256 => Type::Bytes(n),
            RuntimeType::Adt { id, owned: false } => {
                let adt = program.adts.get(id).ok_or(INVALID)?;
                if adt.origin != AdtOrigin::Source || adt.owned {
                    return Err(INVALID.into());
                }
                let args = if let Some(symbolic) = &adt.symbolic {
                    let declaration = self
                        .witness
                        .environment
                        .types
                        .get(symbolic.definition)
                        .ok_or(INVALID)?;
                    budget.charge(
                        adt.name
                            .len()
                            .checked_add(declaration.name.len())
                            .ok_or(LIMIT)?,
                    )?;
                    if declaration.name != adt.name
                        || declaration.params.len() != symbolic.arguments.len()
                        || symbolic.dependencies.owner
                        || symbolic.dependencies.native
                    {
                        return Err(INVALID.into());
                    }
                    budget.charge(symbolic.arguments.len())?;
                    let mut args = Vec::with_capacity(symbolic.arguments.len());
                    for arg in &symbolic.arguments {
                        args.push(self.logical_source(arg, budget, depth + 1)?);
                    }
                    args
                } else {
                    budget.charge(adt.arguments.len())?;
                    let mut args = Vec::with_capacity(adt.arguments.len());
                    for arg in &adt.arguments {
                        args.push(self.runtime_type(*arg, program, budget, depth + 1)?);
                    }
                    args
                };
                Type::Nominal(budget.text(&adt.name)?, args)
            }
            _ => {
                return Err(
                    "E-TYPED-UNSUPPORTED: no-normal control results require pure Copy values"
                        .into(),
                )
            }
        })
    }
    fn result_type(
        &self,
        result: &ResultRepr,
        program: &TypedMain,
        budget: &mut Budget,
    ) -> Result<Type, String> {
        match result {
            ResultRepr::Value(ty) => self.runtime_type(*ty, program, budget, 0),
            ResultRepr::NoNormal(id)
                if id.domain == self.domain && id.index < self.binders.len() =>
            {
                budget.charge(1)?;
                Ok(Type::Var(id.index))
            }
            _ => Err(INVALID.into()),
        }
    }
    fn witness_owners(&self, budget: &mut Budget) -> Result<Vec<usize>, String> {
        budget.charge(self.witness.functions.len())?;
        let sources = self
            .witness
            .functions
            .iter()
            .map(|f| f.source)
            .collect::<HashSet<_>>();
        budget.charge(self.witness.patterns.len())?;
        let mut owners = vec![None; self.witness.patterns.len()];
        budget.charge(self.witness.bindings.len())?;
        let mut declared = vec![false; self.witness.bindings.len()];
        let mut declare =
            |id: BindingId, site: BindingSite, pattern_or_let: bool| -> Result<(), String> {
                let binding = self.witness.bindings.get(id.0).ok_or(INVALID)?;
                let used = declared.get_mut(id.0).ok_or(INVALID)?;
                if *used
                    || binding.site != site
                    || binding.name.is_empty()
                    || (pattern_or_let && binding.mode != ParamMode::Value)
                    || (matches!(site, BindingSite::Pattern(_)) && binding.mutable)
                    || (matches!(site, BindingSite::Parameter { .. })
                        && binding.mutable != (binding.mode == ParamMode::Inout))
                {
                    return Err(INVALID.into());
                }
                *used = true;
                Ok(())
            };
        budget.charge(self.witness.functions.len())?;
        for function in &self.witness.functions {
            budget.charge(function.params.len())?;
            for (ordinal, (binding, _)) in function.params.iter().enumerate() {
                declare(
                    *binding,
                    BindingSite::Parameter {
                        source: function.source,
                        ordinal,
                    },
                    false,
                )?;
            }
        }
        let mut pending = Vec::new();
        budget.charge(self.witness.nodes.len())?;
        for (id, node) in self.witness.nodes.iter().enumerate() {
            budget.charge(1)?;
            if let SourceKind::Let(binding, _, _) = &node.kind {
                declare(*binding, BindingSite::Let(NodeId(id)), true)?;
            }
            if !sources.contains(&node.source) {
                return Err(INVALID.into());
            }
            if let SourceKind::Match(_, arms) = &node.kind {
                budget.charge(arms.len())?;
                for (pattern, _) in arms {
                    pending.push((*pattern, node.source, 0));
                }
            }
        }
        while let Some((id, source, depth)) = pending.pop() {
            budget.charge(1)?;
            if depth > MAX_TYPE_DEPTH {
                return Err(LIMIT.into());
            }
            let owner = owners.get_mut(id.0).ok_or(INVALID)?;
            if owner.replace(source).is_some() {
                return Err(INVALID.into());
            }
            let pattern = self.witness.patterns.get(id.0).ok_or(INVALID)?;
            if let SourcePattern::Binding(binding) = pattern {
                declare(*binding, BindingSite::Pattern(id), true)?;
            }
            if let SourcePattern::Constructor(_, fields) = pattern {
                budget.charge(fields.len())?;
                for field in fields {
                    pending.push((*field, source, depth + 1));
                }
            }
        }
        budget.charge(declared.len())?;
        if declared.iter().any(|used| !used) {
            return Err(INVALID.into());
        }
        budget.charge(owners.len())?;
        owners
            .into_iter()
            .map(|owner| owner.ok_or(INVALID.to_string()))
            .collect()
    }
    pub(in crate::typed) fn verify_structure(&self, program: &TypedMain) -> Result<(), String> {
        let mut budget = Budget::default();
        super::super::bounds::preflight_ir(program)?;
        self.witness.depth(&mut budget)?;
        self.charge(&mut budget)?;
        self.signatures(&mut budget)?;
        self.check_environment(program, &mut budget)?;
        let pattern_owners = self.witness_owners(&mut budget)?;
        let source_returns = self.source_returns(&mut budget)?;
        let ir_facts = IrFacts::build(program)?;
        budget.charge(self.instances.len())?;
        let mut edges = vec![Vec::new(); self.instances.len()];
        if self.instances.len() != program.functions.len() || self.entry != program.entry {
            return Err(INVALID.into());
        }
        // Stored lexical-use counts are only caches. Recount actual reference
        // occurrences before using an unused binding to justify no storage.
        budget.charge(self.witness.bindings.len())?;
        let mut uses = vec![0usize; self.witness.bindings.len()];
        budget.charge(self.witness.nodes.len())?;
        for node in &self.witness.nodes {
            let reference = match &node.kind {
                SourceKind::Var(r)
                | SourceKind::Move(r)
                | SourceKind::Inout(r)
                | SourceKind::InoutField(r, _)
                | SourceKind::Field(r, _)
                | SourceKind::FieldAssign(r, _, _)
                | SourceKind::Assign(r, _) => Some(r),
                _ => None,
            };
            if let Some(Reference::Local(id)) = reference {
                let count = uses.get_mut(id.0).ok_or(INVALID)?;
                *count = count.checked_add(1).ok_or(LIMIT)?;
            }
        }
        budget.charge(uses.len())?;
        if uses
            .iter()
            .zip(&self.witness.bindings)
            .any(|(actual, binding)| *actual != binding.uses)
        {
            return Err(INVALID.into());
        }
        for (slot, instance) in self.instances.iter().enumerate() {
            budget.charge(self.witness.functions.len())?;
            let source = self
                .witness
                .functions
                .iter()
                .find(|f| f.source == instance.source)
                .ok_or(INVALID)?;
            let actual = &program.functions[slot];
            if matches!(actual.result, ResultRepr::NoNormal(_))
                && ir_facts.function_may_return(slot) != Some(false)
            {
                return Err(INVALID.into());
            }
            if actual.params.len() != source.params.len()
                || !actual.param_cleanup.is_empty()
                || self.result_type(&actual.result, program, &mut budget)?
                    != copy_type(&instance.result, &mut budget, 0)?
            {
                return Err(INVALID.into());
            }
            let mut relation = Relation {
                frozen: self,
                program,
                instance,
                budget: &mut budget,
                locals: HashMap::new(),
                local_ids: HashSet::new(),
                pending: Vec::new(),
                seen: HashSet::new(),
                pattern_owners: &pattern_owners,
                source_returns: &source_returns,
                ir_facts: &ir_facts,
                edges: &mut edges[slot],
            };
            relation.budget.charge(actual.params.len())?;
            for (ordinal, ((binding, _), (local, ty, mode))) in
                source.params.iter().zip(&actual.params).enumerate()
            {
                let canonical = self.witness.bindings.get(binding.0).ok_or(INVALID)?;
                if canonical.site
                    != (BindingSite::Parameter {
                        source: instance.source,
                        ordinal,
                    })
                    || canonical.mode != *mode
                    || self.runtime_type(*ty, program, relation.budget, 0)?
                        != copy_type(&instance.params[ordinal], relation.budget, 0)?
                {
                    return Err(INVALID.into());
                }
                relation.bind(*binding, *local)?;
            }
            let body = match &actual.body {
                Computation::Value(value) => Actual::Value(value),
                Computation::NoNormal(value) => Actual::NoNormal(value),
            };
            if self
                .witness
                .nodes
                .get(source.body.0)
                .ok_or(INVALID)?
                .parent
                .is_some()
            {
                return Err(INVALID.into());
            }
            relation.queue(source.body, body)?;
            while let Some((source, actual)) = relation.pending.pop() {
                relation.node(source, actual)?;
            }
            relation.budget.charge(self.witness.nodes.len())?;
            if self.witness.nodes.iter().enumerate().any(|(id, node)| {
                node.source == instance.source && !relation.seen.contains(&NodeId(id))
            }) {
                return Err(INVALID.into());
            }
        }
        budget.charge(self.instances.len().checked_mul(2).ok_or(LIMIT)?)?;
        let mut reached = vec![false; self.instances.len()];
        let mut queue = VecDeque::with_capacity(self.instances.len());
        reached[self.entry] = true;
        queue.push_back(self.entry);
        while let Some(caller) = queue.pop_front() {
            budget.charge(1)?;
            budget.charge(edges[caller].len())?;
            for &callee in &edges[caller] {
                let visited = reached.get_mut(callee).ok_or(INVALID)?;
                if !*visited {
                    *visited = true;
                    queue.push_back(callee);
                }
            }
        }
        budget.charge(reached.len())?;
        if reached.iter().any(|visited| !visited) {
            return Err(INVALID.into());
        }
        Ok(())
    }
}

struct Relation<'a, 'b> {
    frozen: &'a Frozen,
    program: &'a TypedMain,
    instance: &'a Instance,
    budget: &'b mut Budget,
    locals: HashMap<BindingId, LocalId>,
    local_ids: HashSet<LocalId>,
    pending: Vec<(NodeId, Actual<'a>)>,
    seen: HashSet<NodeId>,
    pattern_owners: &'a [usize],
    source_returns: &'a [bool],
    ir_facts: &'b IrFacts<'a>,
    edges: &'b mut Vec<usize>,
}
impl<'a> Relation<'a, '_> {
    fn bind(&mut self, source: BindingId, actual: LocalId) -> Result<(), String> {
        self.budget.charge(2)?;
        if self.locals.insert(source, actual).is_some() || !self.local_ids.insert(actual) {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn local(&mut self, reference: &Reference, actual: LocalId) -> Result<(), String> {
        self.budget.charge(1)?;
        if !matches!(reference, Reference::Local(binding) if self.locals.get(binding) == Some(&actual))
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn queue(&mut self, source: NodeId, actual: Actual<'a>) -> Result<(), String> {
        self.budget.charge(2)?;
        let canonical = self.frozen.witness.nodes.get(source.0).ok_or(INVALID)?;
        if canonical.source != self.instance.source || !self.seen.insert(source) {
            return Err(INVALID.into());
        }
        self.pending.push((source, actual));
        Ok(())
    }
    fn child(
        &mut self,
        parent: NodeId,
        source: NodeId,
        role: Role,
        actual: Actual<'a>,
    ) -> Result<(), String> {
        self.budget.charge(1)?;
        if self
            .frozen
            .witness
            .nodes
            .get(source.0)
            .ok_or(INVALID)?
            .parent
            != Some((parent, role))
        {
            return Err(INVALID.into());
        }
        self.queue(source, actual)
    }
    fn source_type(&mut self, source: NodeId) -> Result<Type, String> {
        let node = self.frozen.witness.nodes.get(source.0).ok_or(INVALID)?;
        substitute(&node.ty, &self.instance.substitution, self.budget, 0)
    }
    fn place(
        &mut self,
        reference: &Reference,
        fields: &[String],
        actual: &FieldPlace,
    ) -> Result<(), String> {
        self.local(reference, actual.root)?;
        if fields.len() != actual.path.len() {
            return Err(INVALID.into());
        }
        self.budget.charge(fields.len())?;
        for (field, step) in fields.iter().zip(&actual.path) {
            let names = self
                .program
                .adts
                .get(step.record)
                .and_then(|adt| adt.record_fields.as_ref())
                .ok_or(INVALID)?;
            let name = names.get(step.field).ok_or(INVALID)?;
            self.budget
                .charge(field.len().checked_add(name.len()).ok_or(LIMIT)?)?;
            if field != name {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
    fn no_storage(
        &mut self,
        parent: NodeId,
        binding: BindingId,
        actual: &crate::typed::NoStorageBinding,
    ) -> Result<(), String> {
        self.budget.charge(1)?;
        let canonical = self.frozen.witness.bindings.get(binding.0).ok_or(INVALID)?;
        if canonical.site != BindingSite::Let(parent)
            || canonical.mutable
            || canonical.uses != 0
            || actual.domain != self.frozen.domain
            || actual.occurrence != binding.0
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn call(
        &mut self,
        node: NodeId,
        source: usize,
        modes: &[ParamMode],
        args: &[NodeId],
        callee: usize,
        actual: &'a [CallArg],
    ) -> Result<(), String> {
        self.budget.charge(1)?;
        self.edges.push(callee);
        let target = self.frozen.instances.get(callee).ok_or(INVALID)?;
        let function = self.program.functions.get(callee).ok_or(INVALID)?;
        if target.source != source
            || args.len() != actual.len()
            || modes.len() != actual.len()
            || function.params.len() != actual.len()
            || self.source_type(node)? != copy_type(&target.result, self.budget, 0)?
        {
            return Err(INVALID.into());
        }
        self.budget.charge(actual.len())?;
        for (index, ((source, actual), mode)) in args.iter().zip(actual).zip(modes).enumerate() {
            if function.params[index].2 != *mode
                || self.source_type(*source)?
                    != copy_type(target.params.get(index).ok_or(INVALID)?, self.budget, 0)?
            {
                return Err(INVALID.into());
            }
            let canonical = self.frozen.witness.nodes.get(source.0).ok_or(INVALID)?;
            if canonical.parent != Some((node, Role::Argument(index))) {
                return Err(INVALID.into());
            }
            match (mode, &canonical.kind, actual) {
                (ParamMode::Value, _, CallArg::Value(value)) => {
                    self.queue(*source, Actual::Value(value))?
                }
                (ParamMode::Read, kind, CallArg::ReadTemporary(value))
                    if !matches!(
                        kind,
                        SourceKind::Var(Reference::Local(_))
                            | SourceKind::Field(Reference::Local(_), _)
                    ) =>
                {
                    self.queue(*source, Actual::Value(value))?
                }
                (ParamMode::Read, SourceKind::Var(reference), CallArg::ReadCopy(local))
                | (ParamMode::Inout, SourceKind::Inout(reference), CallArg::InoutCopy(local)) => {
                    self.local(reference, *local)?;
                    self.mark(*source)?;
                }
                (
                    ParamMode::Read,
                    SourceKind::Field(reference, fields),
                    CallArg::ReadField(place),
                )
                | (
                    ParamMode::Inout,
                    SourceKind::InoutField(reference, fields),
                    CallArg::InoutField(place),
                ) => {
                    self.place(reference, fields, place)?;
                    self.mark(*source)?;
                }
                _ => return Err(INVALID.into()),
            }
        }
        Ok(())
    }
    fn interrupted(
        &mut self,
        source: NodeId,
        target: usize,
        modes: &[ParamMode],
        args: &[NodeId],
        actual: &'a crate::typed::InterruptedCall,
    ) -> Result<(), String> {
        let index = actual.prefix.len();
        self.budget.charge(args.len())?;
        if index >= args.len()
            || modes.len() != args.len()
            || modes.iter().any(|m| *m != ParamMode::Value)
            || self.source_returns.get(args[index].0) != Some(&false)
            || args[..index]
                .iter()
                .any(|arg| self.source_returns.get(arg.0) != Some(&true))
            || self.ir_facts.computation_may_return(&actual.barrier) != Some(false)
        {
            return Err(INVALID.into());
        }
        self.budget.charge(args.len())?;
        let mut parameters = Vec::with_capacity(args.len());
        for arg in args {
            parameters.push(self.source_type(*arg)?);
        }
        let result = self.source_type(source)?;
        let expected = super::super::interrupted::InterruptedEvidence::capture(
            &self.frozen.witness,
            &self.frozen.domain,
            source,
            parameters,
            result,
            self.budget,
        )?;
        // Precharge the supplied evidence itself before equality, even if forged
        // to be larger/deeper than the canonical source call.
        actual.evidence.charge(self.budget)?;
        if actual.evidence != expected || expected.target != target {
            return Err(INVALID.into());
        }
        self.budget.charge(self.frozen.witness.functions.len())?;
        let definition = self
            .frozen
            .witness
            .functions
            .iter()
            .find(|f| f.source == target)
            .ok_or(INVALID)?;
        if definition.params.len() != args.len() {
            return Err(INVALID.into());
        }
        let mut all_variables = BTreeSet::new();
        for (_, ty) in &definition.params {
            variables(ty, &mut all_variables, self.budget, 0)?;
        }
        variables(&definition.result, &mut all_variables, self.budget, 0)?;
        self.budget.charge(self.frozen.witness.nodes.len())?;
        for node in &self.frozen.witness.nodes {
            if node.source == target {
                variables(&node.ty, &mut all_variables, self.budget, 0)?;
            }
        }
        self.budget.charge(definition.quantified.len())?;
        let mut quantified = BTreeSet::new();
        for variable in &definition.quantified {
            self.budget.charge(
                definition
                    .quantified
                    .len()
                    .checked_add(1)
                    .ok_or(LIMIT)?
                    .ilog2() as usize
                    + 1,
            )?;
            self.budget
                .charge(all_variables.len().checked_add(1).ok_or(LIMIT)?.ilog2() as usize + 1)?;
            if !all_variables.contains(variable) || !quantified.insert(*variable) {
                return Err(INVALID.into());
            }
        }
        let mut substitution = BTreeMap::new();
        for ((binding, pattern), actual) in definition.params.iter().zip(&expected.parameters) {
            if self
                .frozen
                .witness
                .bindings
                .get(binding.0)
                .ok_or(INVALID)?
                .mode
                != ParamMode::Value
            {
                return Err(INVALID.into());
            }
            constrain(
                pattern,
                actual,
                &quantified,
                &mut substitution,
                self.budget,
                0,
            )?;
        }
        constrain(
            &definition.result,
            &expected.result,
            &quantified,
            &mut substitution,
            self.budget,
            0,
        )?;
        for (ordinal, value) in actual.prefix.iter().enumerate() {
            self.child(
                source,
                args[ordinal],
                Role::Argument(ordinal),
                Actual::Value(value),
            )?;
        }
        let barrier = match &*actual.barrier {
            Computation::Value(value) => Actual::Value(value),
            Computation::NoNormal(value) => Actual::NoNormal(value),
        };
        self.child(source, args[index], Role::Argument(index), barrier)?;
        // Omitted runtime syntax is still present on both sides of the relation.
        // Its source-owned subtree is marked only after equality and independent
        // source/IR proof of the preceding barrier. No suffix layout is admitted.
        self.budget.charge(self.frozen.witness.nodes.len())?;
        let mut omitted = vec![false; self.frozen.witness.nodes.len()];
        for (ordinal, arg) in args.iter().enumerate().skip(index + 1) {
            let node = self.frozen.witness.nodes.get(arg.0).ok_or(INVALID)?;
            if node.parent != Some((source, Role::Argument(ordinal))) {
                return Err(INVALID.into());
            }
            omitted[arg.0] = true;
        }
        self.budget.charge(self.frozen.witness.nodes.len())?;
        for (id, node) in self.frozen.witness.nodes.iter().enumerate() {
            if node
                .parent
                .is_some_and(|(parent, _)| omitted.get(parent.0) == Some(&true))
            {
                omitted[id] = true;
            }
            if omitted[id] {
                self.mark(NodeId(id))?;
            }
        }
        Ok(())
    }
    fn mark(&mut self, source: NodeId) -> Result<(), String> {
        self.budget.charge(1)?;
        if self
            .frozen
            .witness
            .nodes
            .get(source.0)
            .ok_or(INVALID)?
            .source
            != self.instance.source
            || !self.seen.insert(source)
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn pattern(
        &mut self,
        source: PatternId,
        actual: &CopyPattern,
        depth: usize,
    ) -> Result<(), String> {
        self.budget.charge(1)?;
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        if self.pattern_owners.get(source.0) != Some(&self.instance.source) {
            return Err(INVALID.into());
        }
        match (
            self.frozen.witness.patterns.get(source.0).ok_or(INVALID)?,
            actual,
        ) {
            (SourcePattern::Wildcard, CopyPattern::Wildcard)
            | (SourcePattern::Unit, CopyPattern::Unit) => {}
            (SourcePattern::Int(a), CopyPattern::Int(b)) if a == b => {}
            (SourcePattern::Bool(a), CopyPattern::Bool(b)) if a == b => {}
            (SourcePattern::Binding(binding), CopyPattern::Binding(local)) => {
                if self
                    .frozen
                    .witness
                    .bindings
                    .get(binding.0)
                    .ok_or(INVALID)?
                    .site
                    != BindingSite::Pattern(source)
                {
                    return Err(INVALID.into());
                }
                self.bind(*binding, *local)?;
            }
            (
                SourcePattern::Constructor(name, fields),
                CopyPattern::Constructor {
                    adt,
                    variant,
                    fields: actual,
                },
            ) => {
                let variant = self
                    .program
                    .adts
                    .get(*adt)
                    .and_then(|adt| adt.variants.get(*variant))
                    .ok_or(INVALID)?;
                self.budget
                    .charge(name.len().checked_add(variant.name.len()).ok_or(LIMIT)?)?;
                if name != &variant.name || fields.len() != actual.len() {
                    return Err(INVALID.into());
                }
                self.budget.charge(fields.len())?;
                for (source, actual) in fields.iter().zip(actual) {
                    self.pattern(*source, actual, depth + 1)?;
                }
            }
            _ => return Err(INVALID.into()),
        }
        Ok(())
    }
    fn node(&mut self, source: NodeId, actual: Actual<'a>) -> Result<(), String> {
        self.budget.charge(1)?;
        let expected = self.source_type(source)?;
        let node = self.frozen.witness.nodes.get(source.0).ok_or(INVALID)?;
        match actual {
            Actual::Value(actual) => {
                if expected
                    != self
                        .frozen
                        .runtime_type(actual.ty, self.program, self.budget, 0)?
                {
                    return Err(INVALID.into());
                }
                match (&node.kind, &actual.kind) {
                    (
                        SourceKind::Call(CallTarget::Function(target), modes, args),
                        ExprKind::InterruptedCall(call),
                    ) => self.interrupted(source, *target, modes, args, call)?,
                    (SourceKind::Int(a), ExprKind::Int(b)) if a == b => {}
                    (SourceKind::Bool(a), ExprKind::Bool(b)) if a == b => {}
                    (SourceKind::Unit, ExprKind::Unit) => {}
                    (SourceKind::Str(a), ExprKind::Str(b)) => {
                        self.budget
                            .charge(a.len().checked_add(b.len()).ok_or(LIMIT)?)?;
                        if a != b {
                            return Err(INVALID.into());
                        }
                    }
                    (SourceKind::Var(reference), ExprKind::Read(local)) => {
                        self.local(reference, *local)?
                    }
                    (SourceKind::Field(reference, fields), ExprKind::FieldRead(place)) => {
                        self.place(reference, fields, place)?
                    }
                    (
                        SourceKind::FieldAssign(reference, fields, value),
                        ExprKind::FieldAssign(place, actual),
                    ) => {
                        self.place(reference, fields, place)?;
                        self.child(source, *value, Role::AssignValue, Actual::Value(actual))?;
                    }
                    (SourceKind::Assign(reference, value), ExprKind::Assign(local, actual)) => {
                        self.local(reference, *local)?;
                        self.child(source, *value, Role::AssignValue, Actual::Value(actual))?;
                    }
                    (
                        SourceKind::Binary(op, left, right),
                        ExprKind::Binary(actual_op, actual_left, actual_right),
                    ) if op == actual_op => {
                        self.child(source, *right, Role::Right, Actual::Value(actual_right))?;
                        self.child(source, *left, Role::Left, Actual::Value(actual_left))?;
                    }
                    (
                        SourceKind::If(condition, yes, no),
                        ExprKind::If(actual_condition, actual_yes, actual_no),
                    ) => {
                        self.child(source, *no, Role::Else, Actual::Value(actual_no))?;
                        self.child(source, *yes, Role::Then, Actual::Value(actual_yes))?;
                        self.child(
                            source,
                            *condition,
                            Role::Condition,
                            Actual::Value(actual_condition),
                        )?;
                    }
                    (
                        SourceKind::While(condition, body),
                        ExprKind::While(actual_condition, actual_body),
                    ) => {
                        self.child(source, *body, Role::WhileBody, Actual::Value(actual_body))?;
                        self.child(
                            source,
                            *condition,
                            Role::Condition,
                            Actual::Value(actual_condition),
                        )?;
                    }
                    (
                        SourceKind::Let(binding, value, body),
                        ExprKind::Let {
                            local,
                            mutable,
                            value: actual_value,
                            body: actual_body,
                        },
                    ) => {
                        let canonical =
                            self.frozen.witness.bindings.get(binding.0).ok_or(INVALID)?;
                        if canonical.site != BindingSite::Let(source)
                            || canonical.mutable != *mutable
                        {
                            return Err(INVALID.into());
                        }
                        self.bind(*binding, *local)?;
                        self.child(source, *body, Role::LetBody, Actual::Value(actual_body))?;
                        self.child(source, *value, Role::LetValue, Actual::Value(actual_value))?;
                    }
                    (
                        SourceKind::Let(binding, value, body),
                        ExprKind::NoStorageLet {
                            binding: actual_binding,
                            value: actual_value,
                            body: actual_body,
                        },
                    ) => {
                        self.no_storage(source, *binding, actual_binding)?;
                        self.child(source, *body, Role::LetBody, Actual::Value(actual_body))?;
                        self.child(
                            source,
                            *value,
                            Role::LetValue,
                            Actual::NoNormal(actual_value),
                        )?;
                    }
                    (
                        SourceKind::Call(CallTarget::Function(target), modes, args),
                        ExprKind::Call(callee, actual),
                    ) => self.call(source, *target, modes, args, *callee, actual)?,
                    (SourceKind::Call(CallTarget::Other(name), modes, args), _)
                        if modes.is_empty() =>
                    {
                        self.builtin(source, name, args, actual)?
                    }
                    (
                        SourceKind::Var(Reference::Global(name)),
                        ExprKind::Constructor {
                            adt,
                            variant,
                            fields,
                        },
                    ) if fields.is_empty() => {
                        self.constructor(name, *adt, *variant, 0)?;
                    }
                    (
                        SourceKind::Record(name, fields),
                        ExprKind::Record {
                            adt,
                            fields: actual,
                        },
                    ) => {
                        let record = self.program.adts.get(*adt).ok_or(INVALID)?;
                        self.budget
                            .charge(name.len().checked_add(record.name.len()).ok_or(LIMIT)?)?;
                        let names = record.record_fields.as_ref().ok_or(INVALID)?;
                        if name != &record.name || fields.len() != actual.len() {
                            return Err(INVALID.into());
                        }
                        self.budget.charge(fields.len())?;
                        for (ordinal, ((name, source_value), (field, actual_value))) in
                            fields.iter().zip(actual).enumerate()
                        {
                            let actual_name = names.get(*field).ok_or(INVALID)?;
                            self.budget
                                .charge(name.len().checked_add(actual_name.len()).ok_or(LIMIT)?)?;
                            if name != actual_name {
                                return Err(INVALID.into());
                            }
                            self.child(
                                source,
                                *source_value,
                                Role::RecordField(ordinal),
                                Actual::Value(actual_value),
                            )?;
                        }
                    }
                    (
                        SourceKind::Match(value, arms),
                        ExprKind::CopyMatch {
                            value: actual_value,
                            arms: actual_arms,
                        },
                    ) => {
                        if arms.len() != actual_arms.len() {
                            return Err(INVALID.into());
                        }
                        self.budget.charge(arms.len())?;
                        for (ordinal, ((pattern, body), actual)) in
                            arms.iter().zip(actual_arms).enumerate()
                        {
                            self.pattern(*pattern, &actual.pattern, 0)?;
                            self.child(
                                source,
                                *body,
                                Role::MatchArm(ordinal),
                                Actual::Value(&actual.body),
                            )?;
                        }
                        self.child(
                            source,
                            *value,
                            Role::MatchValue,
                            Actual::Value(actual_value),
                        )?;
                    }
                    _ => return Err(INVALID.into()),
                }
            }
            Actual::NoNormal(actual) => {
                if actual.result.domain != self.frozen.domain
                    || actual.result.index >= self.frozen.binders.len()
                    || expected != Type::Var(actual.result.index)
                    || actual.occurrence != source.0
                    || self.source_returns.get(source.0) != Some(&false)
                    || self.ir_facts.no_normal_may_return(actual) != Some(false)
                {
                    return Err(INVALID.into());
                }
                match (&node.kind, &actual.kind) {
                    (
                        SourceKind::Call(CallTarget::Function(target), modes, args),
                        NoNormalKind::InterruptedCall(call),
                    ) => self.interrupted(source, *target, modes, args, call)?,
                    (
                        SourceKind::Call(CallTarget::Function(target), modes, args),
                        NoNormalKind::Call(callee, actual),
                    ) => self.call(source, *target, modes, args, *callee, actual)?,
                    (
                        SourceKind::Let(binding, value, body),
                        NoNormalKind::Let {
                            local,
                            mutable,
                            value: actual_value,
                            body: actual_body,
                        },
                    ) => {
                        let canonical =
                            self.frozen.witness.bindings.get(binding.0).ok_or(INVALID)?;
                        if canonical.site != BindingSite::Let(source)
                            || canonical.mutable != *mutable
                        {
                            return Err(INVALID.into());
                        }
                        self.bind(*binding, *local)?;
                        self.child(source, *body, Role::LetBody, Actual::NoNormal(actual_body))?;
                        self.child(source, *value, Role::LetValue, Actual::Value(actual_value))?;
                    }
                    (
                        SourceKind::Let(binding, value, body),
                        NoNormalKind::NoStorageLet {
                            binding: actual_binding,
                            value: actual_value,
                            body: actual_body,
                        },
                    ) => {
                        self.no_storage(source, *binding, actual_binding)?;
                        self.child(source, *body, Role::LetBody, Actual::NoNormal(actual_body))?;
                        self.child(
                            source,
                            *value,
                            Role::LetValue,
                            Actual::NoNormal(actual_value),
                        )?;
                    }
                    (
                        SourceKind::If(condition, yes, no),
                        NoNormalKind::If {
                            condition: actual_condition,
                            yes: actual_yes,
                            no: actual_no,
                        },
                    ) => {
                        self.child(source, *no, Role::Else, Actual::NoNormal(actual_no))?;
                        self.child(source, *yes, Role::Then, Actual::NoNormal(actual_yes))?;
                        self.child(
                            source,
                            *condition,
                            Role::Condition,
                            Actual::Value(actual_condition),
                        )?;
                    }
                    (
                        SourceKind::Match(value, arms),
                        NoNormalKind::Match {
                            value: actual_value,
                            arms: actual_arms,
                        },
                    ) => {
                        if arms.len() != actual_arms.len() {
                            return Err(INVALID.into());
                        }
                        self.budget.charge(arms.len())?;
                        for (ordinal, ((pattern, body), actual)) in
                            arms.iter().zip(actual_arms).enumerate()
                        {
                            self.pattern(*pattern, &actual.pattern, 0)?;
                            self.child(
                                source,
                                *body,
                                Role::MatchArm(ordinal),
                                Actual::NoNormal(&actual.body),
                            )?;
                        }
                        self.child(
                            source,
                            *value,
                            Role::MatchValue,
                            Actual::Value(actual_value),
                        )?;
                    }
                    _ => return Err(INVALID.into()),
                }
            }
        }
        Ok(())
    }
    fn constructor(
        &mut self,
        name: &str,
        adt: usize,
        variant: usize,
        fields: usize,
    ) -> Result<(), String> {
        let actual = self
            .program
            .adts
            .get(adt)
            .and_then(|adt| adt.variants.get(variant))
            .ok_or(INVALID)?;
        self.budget
            .charge(name.len().checked_add(actual.name.len()).ok_or(LIMIT)?)?;
        if actual.name != name || actual.fields.len() != fields {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn builtin(
        &mut self,
        node: NodeId,
        name: &str,
        args: &[NodeId],
        actual: &'a ExprNode,
    ) -> Result<(), String> {
        self.budget.charge(name.len())?;
        self.budget.charge(args.len())?;
        let actuals: Vec<&ExprNode> = match (name, &actual.kind) {
            ("byte_len", ExprKind::ByteLen(value)) | ("bytes_len", ExprKind::BytesLen(value)) => {
                self.budget.charge(1)?;
                vec![value]
            }
            ("byte_at", ExprKind::ByteAt(a, b)) | ("bytes_get", ExprKind::BytesGet(a, b)) => {
                self.budget.charge(2)?;
                vec![a, b]
            }
            ("bytes_set", ExprKind::BytesSet(a, b, c)) => {
                self.budget.charge(3)?;
                vec![a, b, c]
            }
            ("bytes_new", ExprKind::BytesNew(n)) if args.len() == 1 => {
                let value = self.frozen.witness.nodes.get(args[0].0).ok_or(INVALID)?;
                if value.parent != Some((node, Role::Argument(0)))
                    || value.kind != SourceKind::Int(i32::try_from(*n).map_err(|_| INVALID)?)
                    || self.source_type(args[0])? != Type::Int
                {
                    return Err(INVALID.into());
                }
                return self.mark(args[0]);
            }
            (_, ExprKind::Vector(op, actuals))
                if crate::typed::VectorOp::named(name) == Some(*op) =>
            {
                self.budget.charge(actuals.len())?;
                actuals.iter().collect()
            }
            (
                _,
                ExprKind::Constructor {
                    adt,
                    variant,
                    fields,
                },
            ) => {
                self.constructor(name, *adt, *variant, args.len())?;
                self.budget.charge(fields.len())?;
                fields.iter().collect()
            }
            _ => {
                return Err(
                    "E-TYPED-UNSUPPORTED: operation has no pure Copy control correspondence rule"
                        .into(),
                )
            }
        };
        if args.len() != actuals.len() {
            return Err(INVALID.into());
        }
        for (index, (source, actual)) in args.iter().zip(actuals).enumerate() {
            self.child(node, *source, Role::Argument(index), Actual::Value(actual))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(source: &str) -> (Frozen, TypedMain) {
        let parsed =
            crate::parser::parse(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
        let checked = crate::check::analyze(&parsed).unwrap();
        let (typed, instances) = crate::typed::lower_main_traced(&checked, None, None).unwrap();
        let frozen = SourceFacts::build(&checked)
            .unwrap()
            .freeze(&instances)
            .unwrap();
        frozen.verify_source(&checked).unwrap();
        frozen
            .verify_structure(&typed)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
        (frozen, typed)
    }
    #[test]
    fn structural_relation_matches_every_pure_copy_operand_and_retained_branch() {
        for source in [
            "fn main() { 42 }",
            "fn sub(a, b) { a - b } fn main() { sub(8, 3) }",
            "fn main() { let mut n = 1; n <- n + 2; if n == 3 { n } else { 0 } }",
            "fn main() { let mut n = 0; while n < 3 { n <- n + 1; }; n }",
            "fn again(n) { if false { 42 } else { again(n) } } fn main() { again(1) }",
            "type Choice = First(i32) | Last fn main() { match First(42) { First(n) => n | Last => 0 } }",
            "type Point = { x: i32; y: i32 } fn main() { let mut p = record Point { y = 2; x = 1 }; p.x <- 3; p.y + p.x }",
            "fn pick(read n) { n } fn main() { let n = 3; pick(n) + pick(4) }",
            "fn bump(inout n) { n <- n + 1; } fn main() { let mut n = 0; bump(inout n); n }",
            "fn main() { bytes_get(bytes_set(bytes_new(4), 2, 9), 2) }",
        ] { fixture(source); }
    }
    #[test]
    fn structural_relation_rejects_same_type_literal_and_operator_changes() {
        let (frozen, mut typed) = fixture("fn main() { 8 - 3 }");
        let Computation::Value(body) = &mut typed.functions[typed.entry].body else {
            unreachable!()
        };
        let ExprKind::Binary(op, _, _) = &mut body.kind else {
            unreachable!()
        };
        *op = BinaryOp::Add;
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
        let Computation::Value(body) = &mut typed.functions[typed.entry].body else {
            unreachable!()
        };
        let ExprKind::Binary(op, _, right) = &mut body.kind else {
            unreachable!()
        };
        *op = BinaryOp::Sub;
        right.kind = ExprKind::Int(4);
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn structural_relation_rejects_reordered_argument_values_and_wrong_callee() {
        let (frozen, mut typed) = fixture("fn sub(a, b) { a - b } fn main() { sub(8, 3) }");
        let Computation::Value(body) = &mut typed.functions[typed.entry].body else {
            unreachable!()
        };
        let ExprKind::Call(_, args) = &mut body.kind else {
            unreachable!()
        };
        args.swap(0, 1);
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn structural_relation_rejects_a_changed_skipped_arm_even_when_flow_is_unchanged() {
        let (frozen, mut typed) =
            fixture("fn again(n) { if false { 42 } else { again(n) } } fn main() { again(1) }");
        let Computation::Value(body) = &mut typed.functions[0].body else {
            unreachable!()
        };
        let ExprKind::If(_, yes, _) = &mut body.kind else {
            unreachable!()
        };
        yes.kind = ExprKind::Int(43);
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    fn bare_fixture(concrete_tail: bool) -> (Frozen, TypedMain) {
        use crate::typed::{foreign::NativeManifest, logical, NoStorageBinding, TypedFunction};
        let source = if concrete_tail {
            "fn forever() { forever() } fn main() { let unused = forever(); 42 }"
        } else {
            "fn forever() { forever() } fn main() { forever() }"
        };
        let parsed = crate::parser::parse(source).unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let facts = SourceFacts::build(&checked).unwrap();
        let domain = logical::Domain::default();
        let binders = vec![logical::OpaqueBinding {
            scope: 0,
            ordinal: 0,
        }];
        let mut instances = Vec::new();
        for source in 0..2 {
            let function = facts
                .witness
                .functions
                .iter()
                .find(|f| f.source == source)
                .unwrap();
            let mut classes = BTreeSet::new();
            let mut budget = Budget::default();
            variables(&function.result, &mut classes, &mut budget, 0).unwrap();
            for node in facts
                .witness
                .nodes
                .iter()
                .filter(|node| node.source == source)
            {
                variables(&node.ty, &mut classes, &mut budget, 0).unwrap();
            }
            assert_eq!(classes.len(), 1);
            instances.push(Instance {
                source,
                scope: if source == 1 { 0 } else { 1 },
                params: vec![],
                result: if source == 1 && concrete_tail {
                    Type::Int
                } else {
                    Type::Var(0)
                },
                substitution: classes
                    .into_iter()
                    .map(|class| (class, Type::Var(0)))
                    .collect(),
            });
        }
        let frozen = Frozen {
            domain: domain.clone(),
            binders: binders.clone(),
            witness: facts.witness,
            instances,
            entry: 1,
        };
        frozen.verify_source(&checked).unwrap();
        let identity = crate::typed::OpaqueId {
            domain: domain.clone(),
            index: 0,
        };
        let mut functions = Vec::new();
        for source in 0..2 {
            let function = frozen
                .witness
                .functions
                .iter()
                .find(|f| f.source == source)
                .unwrap();
            let (result, body) = if source == 1 && concrete_tail {
                let SourceKind::Let(binding, value, _) =
                    &frozen.witness.nodes[function.body.0].kind
                else {
                    unreachable!()
                };
                (
                    ResultRepr::Value(RuntimeType::Int),
                    Computation::Value(ExprNode {
                        ty: RuntimeType::Int,
                        kind: ExprKind::NoStorageLet {
                            binding: NoStorageBinding {
                                domain: domain.clone(),
                                occurrence: binding.0,
                            },
                            value: Box::new(NoNormalExpr {
                                result: identity.clone(),
                                occurrence: value.0,
                                kind: NoNormalKind::Call(0, vec![]),
                            }),
                            body: Box::new(ExprNode {
                                ty: RuntimeType::Int,
                                kind: ExprKind::Int(42),
                            }),
                        },
                    }),
                )
            } else {
                (
                    ResultRepr::NoNormal(identity.clone()),
                    Computation::NoNormal(NoNormalExpr {
                        result: identity.clone(),
                        occurrence: function.body.0,
                        kind: NoNormalKind::Call(0, vec![]),
                    }),
                )
            };
            functions.push(TypedFunction {
                params: vec![],
                result,
                body,
                param_cleanup: vec![],
            });
        }
        let typed = TypedMain {
            logical: logical::Metadata {
                control: None,
                domain: Some(domain),
                declarations: vec![],
                source_functions: vec!["forever".into(), "main".into()],
                scope_sources: vec![1, 0],
                function_scopes: vec![1, 0],
                binders,
            },
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: vec![],
            functions,
            entry: 1,
        };
        frozen.verify_structure(&typed).unwrap();
        // This fixture tests the proof directly. It deliberately does not attach
        // that proof, so the public verifier must reject its extension tags.
        assert!(crate::typed::verify(&typed).is_err());
        (frozen, typed)
    }
    #[test]
    fn exact_source_relation_proves_bare_tail_and_retained_concrete_no_storage() {
        bare_fixture(false);
        bare_fixture(true);
    }
    #[test]
    fn exact_source_relation_rejects_wrong_no_normal_occurrence_and_foreign_brand() {
        let (frozen, mut typed) = bare_fixture(false);
        let Computation::NoNormal(body) = &mut typed.functions[0].body else {
            unreachable!()
        };
        body.occurrence = usize::MAX;
        assert!(frozen.verify_structure(&typed).is_err());
        let (frozen, mut typed) = bare_fixture(false);
        let Computation::NoNormal(body) = &mut typed.functions[0].body else {
            unreachable!()
        };
        body.result.domain = logical::Domain::default();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn exact_source_relation_rejects_no_storage_tail_and_binding_tampering() {
        let (frozen, mut typed) = bare_fixture(true);
        let Computation::Value(body) = &mut typed.functions[1].body else {
            unreachable!()
        };
        let ExprKind::NoStorageLet { body, .. } = &mut body.kind else {
            unreachable!()
        };
        body.kind = ExprKind::Int(43);
        assert!(frozen.verify_structure(&typed).is_err());
        let (frozen, mut typed) = bare_fixture(true);
        let Computation::Value(body) = &mut typed.functions[1].body else {
            unreachable!()
        };
        let ExprKind::NoStorageLet { binding, .. } = &mut body.kind else {
            unreachable!()
        };
        binding.occurrence = usize::MAX;
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn exact_environment_relation_rejects_well_formed_native_deletion() {
        let (frozen, mut typed) =
            fixture("@include(\"stdint.h\"); extern fn unused(x: i32) -> i32; fn main() { 1 }");
        typed.native.functions.clear();
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn exact_environment_relation_retains_inactive_variant_field_types() {
        let (frozen, mut typed) =
            fixture("type Choice = First(i32) | Last(bool) fn main() { First(42) }");
        typed.adts[0].variants[1].fields[0] = RuntimeType::Int;
        crate::typed::verify(&typed).unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
    #[test]
    fn standalone_relation_rejects_unowned_marked_nodes_and_orphan_patterns() {
        for source in [
            "fn pick(read n) { n } fn main() { let n = 3; pick(n) }",
            "fn bump(inout n) { n <- n + 1; } fn main() { let mut n = 3; bump(inout n); n }",
            "type P = { x: i32 } fn pick(read n) { n } fn main() { let p = record P { x = 3 }; pick(p.x) }",
            "type P = { x: i32 } fn bump(inout n) { n <- n + 1; } fn main() { let mut p = record P { x = 3 }; bump(inout p.x); p.x }",
            "fn main() { bytes_new(4) }",
        ] {
            let (mut frozen, typed) = fixture(source);
            let argument = frozen.witness.nodes.iter_mut().find(|node| matches!(node.parent, Some((_, Role::Argument(_))))).unwrap();
            argument.source = usize::MAX;
            assert!(frozen.verify_structure(&typed).is_err(), "{source}");
        }
        let (mut frozen, typed) = fixture("fn main() { 42 }");
        frozen.witness.nodes[0].source = usize::MAX;
        assert!(frozen.verify_structure(&typed).is_err());
        let (mut frozen, typed) = fixture("fn main() { 42 }");
        frozen.witness.patterns.push(SourcePattern::Wildcard);
        assert!(frozen.verify_structure(&typed).is_err());
        let (mut frozen, typed) = fixture("fn main() { 42 }");
        frozen.witness.bindings.push(Binding {
            site: BindingSite::Let(NodeId(0)),
            name: "orphan".into(),
            mutable: false,
            mode: ParamMode::Value,
            uses: 0,
        });
        assert!(frozen.verify_structure(&typed).is_err());
    }
}
