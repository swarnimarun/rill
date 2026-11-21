//! Bounded, conservative normal-return facts for the staged control-result work.
//!
//! This module does not admit a layout, replace a computation, or prove source to
//! IR correspondence. Source and IR builders independently reconstruct positive
//! Boolean circuits. False means that no finite normal-return derivation exists;
//! true is only an over-approximation. In particular traps, literal conditions,
//! result-type annotations, and cycles are never trusted as false seeds.
//!
//! Admission requires canonical source/IR correspondence and bounded recursive
//! consumers, composed at the public verifier boundary.
use super::{
    CallArg, Computation, DiscardExpr, DiscardKind, ExprKind, ExprNode, ForeignArg, NoNormalExpr,
    NoNormalKind, OwnerInit, TypedMain,
};
use crate::ast::{BinaryOp, Expr, ParamMode, Pattern};
use crate::check::{CheckedProgram, Type};
use std::collections::{HashMap, VecDeque};

#[path = "control_bounds.rs"]
mod bounds;
#[path = "control_interrupted.rs"]
mod interrupted;
#[path = "control_relation.rs"]
mod relation;
pub(super) use bounds::has_extension;
pub(super) use interrupted::InterruptedEvidence;
pub(super) use relation::Frozen;

const INVALID: &str = "E-TYPED-CONTROL: invalid control proof";
const LIMIT: &str = "E-TYPED-CONTROL-COMPLEXITY: control work limit exceeded";
const MAX_WORK: usize = 100_000;
const MAX_TYPE_DEPTH: usize = 128;
const MAX_INSTANCES: usize = 1024;

#[derive(Default)]
pub(super) struct Budget {
    used: usize,
}
impl Budget {
    pub(super) fn charge(&mut self, amount: usize) -> Result<(), String> {
        self.used = self
            .used
            .checked_add(amount)
            .filter(|total| *total <= MAX_WORK)
            .ok_or(LIMIT)?;
        Ok(())
    }
    pub(super) fn reserve_type(&mut self, ty: &Type, depth: usize) -> Result<(), String> {
        self.charge(1)?;
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        match ty {
            Type::Nominal(name, args) => {
                self.charge(name.len())?;
                self.charge(args.len())?;
                for arg in args {
                    self.reserve_type(arg, depth + 1)?;
                }
            }
            Type::CPointer { pointee, .. } => self.charge(pointee.len())?,
            _ => {}
        }
        Ok(())
    }
    pub(super) fn text(&mut self, value: &str) -> Result<String, String> {
        self.charge(value.len())?;
        self.charge(1)?;
        Ok(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GateId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Pending,
    All,
    Any,
}
#[derive(Debug)]
struct Gate {
    operator: Operator,
    inputs: Vec<GateId>,
}
#[derive(Default)]
struct Circuit {
    gates: Vec<Gate>,
}
impl Circuit {
    fn reserve(&mut self, budget: &mut Budget) -> Result<GateId, String> {
        budget.charge(1)?;
        let id = GateId(self.gates.len());
        self.gates.push(Gate {
            operator: Operator::Pending,
            inputs: Vec::new(),
        });
        Ok(id)
    }
    fn define(
        &mut self,
        id: GateId,
        operator: Operator,
        inputs: impl ExactSizeIterator<Item = GateId>,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.charge(1)?;
        budget.charge(inputs.len())?;
        if operator == Operator::Pending
            || self
                .gates
                .get(id.0)
                .is_none_or(|g| g.operator != Operator::Pending)
        {
            return Err(INVALID.into());
        }
        let mut edges = Vec::with_capacity(inputs.len());
        for input in inputs {
            if input.0 >= self.gates.len() {
                return Err(INVALID.into());
            }
            edges.push(input);
        }
        self.gates[id.0] = Gate {
            operator,
            inputs: edges,
        };
        Ok(())
    }
    #[cfg(test)]
    fn all(
        &mut self,
        inputs: impl ExactSizeIterator<Item = GateId>,
        budget: &mut Budget,
    ) -> Result<GateId, String> {
        let id = self.reserve(budget)?;
        self.define(id, Operator::All, inputs, budget)?;
        Ok(id)
    }
    fn any(
        &mut self,
        inputs: impl ExactSizeIterator<Item = GateId>,
        budget: &mut Budget,
    ) -> Result<GateId, String> {
        let id = self.reserve(budget)?;
        self.define(id, Operator::Any, inputs, budget)?;
        Ok(id)
    }

    /// The least fixed point of a positive finite circuit. Each gate changes at
    /// most once. Duplicate dependencies are intentional and counted separately.
    fn solve(&self, budget: &mut Budget) -> Result<Vec<bool>, String> {
        budget.charge(self.gates.len())?;
        for gate in &self.gates {
            budget.charge(gate.inputs.len())?;
            if gate.operator == Operator::Pending
                || gate.inputs.iter().any(|id| id.0 >= self.gates.len())
            {
                return Err(INVALID.into());
            }
        }
        let count = self.gates.len();
        // Precharge every table and the bounded queue before allocation.
        budget.charge(count.checked_mul(4).ok_or(LIMIT)?)?;
        let mut dependents = vec![Vec::new(); count];
        let mut remaining = vec![0usize; count];
        let mut values = vec![false; count];
        let mut queue = VecDeque::with_capacity(count);
        for (id, gate) in self.gates.iter().enumerate() {
            budget.charge(1)?;
            remaining[id] = gate.inputs.len();
            for input in &gate.inputs {
                budget.charge(1)?;
                dependents[input.0].push(id);
            }
            if gate.operator == Operator::All && gate.inputs.is_empty() {
                budget.charge(1)?;
                values[id] = true;
                queue.push_back(id);
            }
        }
        while let Some(input) = queue.pop_front() {
            budget.charge(1)?;
            for &dependent in &dependents[input] {
                budget.charge(1)?;
                if values[dependent] {
                    continue;
                }
                let becomes_true = match self.gates[dependent].operator {
                    Operator::All => {
                        remaining[dependent] =
                            remaining[dependent].checked_sub(1).ok_or(INVALID)?;
                        remaining[dependent] == 0
                    }
                    Operator::Any => true,
                    Operator::Pending => return Err(INVALID.into()),
                };
                if becomes_true {
                    budget.charge(1)?;
                    values[dependent] = true;
                    queue.push_back(dependent);
                }
            }
        }
        Ok(values)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct NodeId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PatternId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BindingId(usize);

/// Parent plus child role is a compact structural occurrence path. It avoids
/// quadratic copies of full ancestor paths along long statement/let chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Left,
    Right,
    Condition,
    Then,
    Else,
    LetValue,
    LetBody,
    AssignValue,
    Argument(usize),
    RecordField(usize),
    MatchValue,
    MatchArm(usize),
    WhileBody,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceNode {
    source: usize,
    parent: Option<(NodeId, Role)>,
    elaborated_local: bool,
    ty: Type,
    kind: SourceKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum CallTarget {
    Function(usize),
    // Builtins, constructors and native names retain their exact resolved
    // spelling. They are all conservatively possibly returning in this pass.
    Other(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reference {
    Local(BindingId),
    Global(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    Pending,
    Int(i32),
    Str(String),
    CStr(String),
    Bool(bool),
    Unit,
    Var(Reference),
    Move(Reference),
    Inout(Reference),
    InoutField(Reference, Vec<String>),
    Record(String, Vec<(String, NodeId)>),
    Field(Reference, Vec<String>),
    FieldAssign(Reference, Vec<String>, NodeId),
    Binary(BinaryOp, NodeId, NodeId),
    If(NodeId, NodeId, NodeId),
    Let(BindingId, NodeId, NodeId),
    Assign(Reference, NodeId),
    Call(CallTarget, Vec<ParamMode>, Vec<NodeId>),
    Match(NodeId, Vec<(PatternId, NodeId)>),
    While(NodeId, NodeId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum SourcePattern {
    Pending,
    Wildcard,
    Binding(BindingId),
    Int(i32),
    Bool(bool),
    Unit,
    Constructor(String, Vec<PatternId>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum BindingSite {
    Parameter { source: usize, ordinal: usize },
    Let(NodeId),
    Pattern(PatternId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    site: BindingSite,
    name: String,
    mutable: bool,
    mode: ParamMode,
    uses: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFunction {
    source: usize,
    name: String,
    params: Vec<(BindingId, Type)>,
    quantified: Vec<usize>,
    result: Type,
    body: NodeId,
}
/// Canonical syntax data, not a lowering-produced certificate. It includes all
/// branches and tails, exact operands and lexical uses, even after divergence.
#[derive(Debug, Default, PartialEq)]
struct Witness {
    environment: SourceEnvironment,
    functions: Vec<SourceFunction>,
    nodes: Vec<SourceNode>,
    patterns: Vec<SourcePattern>,
    bindings: Vec<Binding>,
}

/// The proof retains all declarations and native configuration, including unused
/// declarations. A well-formed replacement manifest is not evidence that the
/// original declarations survived. The tree is charged before any clone.
#[derive(Debug, Default, PartialEq)]
struct SourceEnvironment {
    native: crate::c_import::Config,
    constants: Vec<crate::ast::ExternConst>,
    allocation: Option<crate::ast::AllocationTypes>,
    types: Vec<crate::ast::TypeDecl>,
    externs: Vec<crate::ast::ExternFunction>,
    function_names: Vec<String>,
}
impl SourceEnvironment {
    fn capture(program: &crate::ast::Program, budget: &mut Budget) -> Result<Self, String> {
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
        fn type_expr(
            ty: &crate::ast::TypeExpr,
            depth: usize,
            budget: &mut Budget,
        ) -> Result<(), String> {
            if depth > MAX_TYPE_DEPTH {
                return Err(LIMIT.into());
            }
            text(&ty.name, budget)?;
            budget.charge(ty.args.len())?;
            for arg in &ty.args {
                type_expr(arg, depth + 1, budget)?;
            }
            Ok(())
        }
        budget.charge(7)?;
        for values in [
            &program.native.includes,
            &program.native.links,
            &program.native.packages,
        ] {
            budget.charge(values.len())?;
            for value in values {
                text(value, budget)?;
            }
        }
        budget.charge(program.constants.len())?;
        for constant in &program.constants {
            text(&constant.name, budget)?;
            external(&constant.ty, budget)?;
        }
        if let Some(allocation) = &program.allocation_types {
            for value in [
                &allocation.result_type,
                &allocation.ok_constructor,
                &allocation.err_constructor,
                &allocation.error_type,
                &allocation.invalid_length_constructor,
                &allocation.size_overflow_constructor,
                &allocation.out_of_memory_constructor,
            ] {
                text(value, budget)?;
            }
        }
        budget.charge(program.types.len())?;
        for declaration in &program.types {
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
                    type_expr(field, 0, budget)?;
                }
            }
        }
        budget.charge(program.externs.len())?;
        for function in &program.externs {
            text(&function.name, budget)?;
            budget.charge(function.params.len())?;
            for parameter in &function.params {
                text(&parameter.name, budget)?;
                external(&parameter.ty, budget)?;
            }
            external(&function.result, budget)?;
        }
        budget.charge(program.functions.len())?;
        for function in &program.functions {
            text(&function.name, budget)?;
        }
        Ok(Self {
            native: program.native.clone(),
            constants: program.constants.clone(),
            allocation: program.allocation_types.clone(),
            types: program.types.clone(),
            externs: program.externs.clone(),
            function_names: program.functions.iter().map(|f| f.name.clone()).collect(),
        })
    }
}

pub(super) fn copy_type(ty: &Type, budget: &mut Budget, depth: usize) -> Result<Type, String> {
    budget.charge(1)?;
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    Ok(match ty {
        Type::Nominal(name, args) => {
            let name = budget.text(name)?;
            budget.charge(args.len())?;
            let mut result = Vec::with_capacity(args.len());
            for arg in args {
                result.push(copy_type(arg, budget, depth + 1)?);
            }
            Type::Nominal(name, result)
        }
        Type::CPointer { pointee, is_const } => Type::CPointer {
            pointee: budget.text(pointee)?,
            is_const: *is_const,
        },
        ty => ty.clone(),
    })
}

struct SourceBuilder<'a, 's> {
    checked: &'a CheckedProgram<'s>,
    budget: Budget,
    circuit: Circuit,
    witness: Witness,
    names: HashMap<&'s str, usize>,
    functions: HashMap<usize, GateId>,
    pending_functions: VecDeque<usize>,
    expression_ids: HashMap<*const Expr, NodeId>,
    flows: Vec<GateId>,
    scope: HashMap<&'s str, Vec<BindingId>>,
}
#[derive(Clone, Copy)]
enum Visit<'s> {
    Expr(&'s Expr, NodeId),
    Bind(&'s str, BindingId),
    Unbind(&'s str, BindingId),
}
impl<'a, 's> SourceBuilder<'a, 's> {
    fn new(checked: &'a CheckedProgram<'s>) -> Result<Self, String> {
        let mut budget = Budget::default();
        budget.charge(checked.program.functions.len())?;
        let mut names = HashMap::new();
        for (source, function) in checked.program.functions.iter().enumerate() {
            budget.charge(function.name.len())?;
            if names.insert(function.name.as_str(), source).is_some() {
                return Err(INVALID.into());
            }
        }
        let environment = SourceEnvironment::capture(checked.program, &mut budget)?;
        Ok(Self {
            checked,
            budget,
            circuit: Circuit::default(),
            witness: Witness {
                environment,
                ..Witness::default()
            },
            names,
            functions: HashMap::new(),
            pending_functions: VecDeque::new(),
            expression_ids: HashMap::new(),
            flows: Vec::new(),
            scope: HashMap::new(),
        })
    }
    fn function(&mut self, source: usize) -> Result<GateId, String> {
        self.budget.charge(1)?;
        if let Some(id) = self.functions.get(&source) {
            return Ok(*id);
        }
        if source >= self.checked.program.functions.len() {
            return Err(INVALID.into());
        }
        if self.functions.len() >= MAX_INSTANCES {
            return Err(LIMIT.into());
        }
        self.budget.charge(2)?;
        let id = self.circuit.reserve(&mut self.budget)?;
        self.functions.insert(source, id);
        self.pending_functions.push_back(source);
        Ok(id)
    }
    fn node(
        &mut self,
        expr: &'s Expr,
        source: usize,
        parent: Option<(NodeId, Role)>,
    ) -> Result<NodeId, String> {
        self.budget.charge(3)?;
        let ty = copy_type(
            self.checked.expression_type(expr).ok_or(INVALID)?,
            &mut self.budget,
            0,
        )?;
        let id = NodeId(self.witness.nodes.len());
        if self
            .expression_ids
            .insert(expr as *const Expr, id)
            .is_some()
        {
            return Err(INVALID.into());
        }
        self.witness.nodes.push(SourceNode {
            source,
            parent,
            elaborated_local: self.checked.polymorphic_local(expr).is_some(),
            ty,
            kind: SourceKind::Pending,
        });
        self.flows.push(self.circuit.reserve(&mut self.budget)?);
        Ok(id)
    }
    fn binding(
        &mut self,
        site: BindingSite,
        name: &str,
        mutable: bool,
        mode: ParamMode,
    ) -> Result<BindingId, String> {
        let name = self.budget.text(name)?;
        self.budget.charge(1)?;
        let id = BindingId(self.witness.bindings.len());
        self.witness.bindings.push(Binding {
            site,
            name,
            mutable,
            mode,
            uses: 0,
        });
        Ok(id)
    }
    fn bind(&mut self, name: &'s str, id: BindingId) -> Result<(), String> {
        self.budget.charge(name.len())?;
        self.budget.charge(2)?;
        self.scope.entry(name).or_default().push(id);
        Ok(())
    }
    fn unbind(&mut self, name: &'s str, id: BindingId) -> Result<(), String> {
        self.budget.charge(name.len())?;
        self.budget.charge(1)?;
        let bindings = self.scope.get_mut(name).ok_or(INVALID)?;
        if bindings.pop() != Some(id) {
            return Err(INVALID.into());
        }
        if bindings.is_empty() {
            self.scope.remove(name);
        }
        Ok(())
    }
    fn reference(&mut self, name: &str) -> Result<Reference, String> {
        self.budget.charge(name.len())?;
        self.budget.charge(1)?;
        if let Some(&id) = self.scope.get(name).and_then(|ids| ids.last()) {
            let binding = self.witness.bindings.get_mut(id.0).ok_or(INVALID)?;
            binding.uses = binding.uses.checked_add(1).ok_or(LIMIT)?;
            Ok(Reference::Local(id))
        } else {
            Ok(Reference::Global(self.budget.text(name)?))
        }
    }
    fn path(&mut self, path: &[String]) -> Result<Vec<String>, String> {
        self.budget.charge(path.len())?;
        path.iter().map(|part| self.budget.text(part)).collect()
    }
    fn pattern(
        &mut self,
        pattern: &'s Pattern,
        bindings: &mut Vec<(&'s str, BindingId)>,
        depth: usize,
    ) -> Result<PatternId, String> {
        self.budget.charge(1)?;
        if depth > MAX_TYPE_DEPTH {
            return Err(LIMIT.into());
        }
        let id = PatternId(self.witness.patterns.len());
        self.witness.patterns.push(SourcePattern::Pending);
        let kind = match pattern {
            Pattern::Wildcard => SourcePattern::Wildcard,
            Pattern::Binding(name) => {
                let binding =
                    self.binding(BindingSite::Pattern(id), name, false, ParamMode::Value)?;
                self.budget.charge(1)?;
                bindings.push((name, binding));
                SourcePattern::Binding(binding)
            }
            Pattern::Int(value) => SourcePattern::Int(*value),
            Pattern::Bool(value) => SourcePattern::Bool(*value),
            Pattern::Unit => SourcePattern::Unit,
            Pattern::Constructor(name, fields) => {
                let name = self.budget.text(name)?;
                self.budget.charge(fields.len())?;
                let mut ids = Vec::with_capacity(fields.len());
                for field in fields {
                    ids.push(self.pattern(field, bindings, depth + 1)?);
                }
                SourcePattern::Constructor(name, ids)
            }
        };
        self.witness.patterns[id.0] = kind;
        Ok(id)
    }
    fn push(&mut self, visits: &mut Vec<Visit<'s>>, visit: Visit<'s>) -> Result<(), String> {
        self.budget.charge(1)?;
        visits.push(visit);
        Ok(())
    }
    fn child(
        &mut self,
        expr: &'s Expr,
        parent: NodeId,
        role: Role,
        visits: &mut Vec<Visit<'s>>,
    ) -> Result<NodeId, String> {
        let source = self.witness.nodes[parent.0].source;
        let id = self.node(expr, source, Some((parent, role)))?;
        self.push(visits, Visit::Expr(expr, id))?;
        Ok(id)
    }
    fn flow(&self, node: NodeId) -> GateId {
        self.flows[node.0]
    }
    fn define(
        &mut self,
        node: NodeId,
        inputs: impl ExactSizeIterator<Item = GateId>,
    ) -> Result<(), String> {
        self.circuit
            .define(self.flow(node), Operator::All, inputs, &mut self.budget)
    }
    fn expression(
        &mut self,
        expr: &'s Expr,
        id: NodeId,
        visits: &mut Vec<Visit<'s>>,
    ) -> Result<(), String> {
        self.budget.charge(1)?;
        let kind = match expr {
            Expr::Lambda(..) | Expr::Apply(..) | Expr::FunctionOp(..) => {
                return Err(
                    "E-TYPED-UNSUPPORTED: callable control proofs require closure conversion"
                        .into(),
                );
            }
            Expr::Int(value) => SourceKind::Int(*value),
            Expr::Str(value) => SourceKind::Str(self.budget.text(value)?),
            Expr::CStr(value) => SourceKind::CStr(self.budget.text(value)?),
            Expr::Bool(value) => SourceKind::Bool(*value),
            Expr::Unit => SourceKind::Unit,
            Expr::Var(name) => SourceKind::Var(self.reference(name)?),
            Expr::Move(name) => SourceKind::Move(self.reference(name)?),
            Expr::Inout(name) => SourceKind::Inout(self.reference(name)?),
            Expr::InoutField(name, path) => {
                SourceKind::InoutField(self.reference(name)?, self.path(path)?)
            }
            Expr::Field(name, path) => SourceKind::Field(self.reference(name)?, self.path(path)?),
            Expr::Assign(name, value) => {
                let reference = self.reference(name)?;
                let value = self.child(value, id, Role::AssignValue, visits)?;
                self.define(id, [self.flow(value)].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::Assign(reference, value);
                return Ok(());
            }
            Expr::FieldAssign(name, path, value) => {
                let reference = self.reference(name)?;
                let path = self.path(path)?;
                let value = self.child(value, id, Role::AssignValue, visits)?;
                self.define(id, [self.flow(value)].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::FieldAssign(reference, path, value);
                return Ok(());
            }
            Expr::Binary(op, left, right) => {
                let right = self.child(right, id, Role::Right, visits)?;
                let left = self.child(left, id, Role::Left, visits)?;
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.define(id, [self.flow(left)].into_iter())?;
                } else {
                    self.define(id, [self.flow(left), self.flow(right)].into_iter())?;
                }
                self.witness.nodes[id.0].kind = SourceKind::Binary(*op, left, right);
                return Ok(());
            }
            Expr::If(condition, yes, no) => {
                let no = self.child(no, id, Role::Else, visits)?;
                let yes = self.child(yes, id, Role::Then, visits)?;
                let condition = self.child(condition, id, Role::Condition, visits)?;
                let arms = self.circuit.any(
                    [self.flow(yes), self.flow(no)].into_iter(),
                    &mut self.budget,
                )?;
                self.define(id, [self.flow(condition), arms].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::If(condition, yes, no);
                return Ok(());
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                let binding = self.binding(
                    BindingSite::Let(id),
                    name,
                    matches!(expr, Expr::LetMut(..)),
                    ParamMode::Value,
                )?;
                self.push(visits, Visit::Unbind(name, binding))?;
                let body = self.child(body, id, Role::LetBody, visits)?;
                self.push(visits, Visit::Bind(name, binding))?;
                let value = self.child(value, id, Role::LetValue, visits)?;
                self.define(id, [self.flow(value), self.flow(body)].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::Let(binding, value, body);
                return Ok(());
            }
            Expr::While(condition, body) => {
                let body = self.child(body, id, Role::WhileBody, visits)?;
                let condition = self.child(condition, id, Role::Condition, visits)?;
                self.define(id, [self.flow(condition)].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::While(condition, body);
                return Ok(());
            }
            Expr::Call(name, arguments) => {
                self.budget.charge(name.len())?;
                let source = self.names.get(name.as_str()).copied();
                let (target, callee, modes) = if let Some(source) = source {
                    let callee = self.function(source)?;
                    let function = &self.checked.program.functions[source];
                    if function.params.len() != arguments.len() {
                        return Err(INVALID.into());
                    }
                    self.budget.charge(function.params.len())?;
                    let modes = function.params.iter().map(|p| p.mode).collect();
                    (CallTarget::Function(source), Some(callee), modes)
                } else {
                    (CallTarget::Other(self.budget.text(name)?), None, Vec::new())
                };
                self.budget.charge(arguments.len())?;
                let mut ids = Vec::with_capacity(arguments.len());
                for (ordinal, argument) in arguments.iter().enumerate().rev() {
                    ids.push(self.child(argument, id, Role::Argument(ordinal), visits)?);
                }
                self.budget.charge(ids.len())?;
                ids.reverse();
                let input_count = ids
                    .len()
                    .checked_add(usize::from(callee.is_some()))
                    .ok_or(LIMIT)?;
                self.budget.charge(input_count)?;
                let mut inputs = Vec::with_capacity(input_count);
                for argument in &ids {
                    inputs.push(self.flow(*argument));
                }
                if let Some(callee) = callee {
                    inputs.push(callee);
                }
                self.define(id, inputs.into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::Call(target, modes, ids);
                return Ok(());
            }
            Expr::Record(name, fields) => {
                let name = self.budget.text(name)?;
                self.budget.charge(fields.len())?;
                let mut ids = Vec::with_capacity(fields.len());
                for (ordinal, (name, value)) in fields.iter().enumerate().rev() {
                    let name = self.budget.text(name)?;
                    let value = self.child(value, id, Role::RecordField(ordinal), visits)?;
                    ids.push((name, value));
                }
                self.budget.charge(ids.len())?;
                ids.reverse();
                let inputs = ids.iter().map(|(_, child)| self.flows[child.0]);
                self.circuit
                    .define(self.flow(id), Operator::All, inputs, &mut self.budget)?;
                self.witness.nodes[id.0].kind = SourceKind::Record(name, ids);
                return Ok(());
            }
            Expr::Match(value, arms) => {
                self.budget.charge(arms.len())?;
                let mut ids = Vec::with_capacity(arms.len());
                for (ordinal, arm) in arms.iter().enumerate().rev() {
                    let mut bindings = Vec::new();
                    let pattern = self.pattern(&arm.pattern, &mut bindings, 0)?;
                    self.budget.charge(bindings.len())?;
                    for &(name, binding) in &bindings {
                        self.push(visits, Visit::Unbind(name, binding))?;
                    }
                    let body = self.child(&arm.body, id, Role::MatchArm(ordinal), visits)?;
                    self.budget.charge(bindings.len())?;
                    for &(name, binding) in bindings.iter().rev() {
                        self.push(visits, Visit::Bind(name, binding))?;
                    }
                    ids.push((pattern, body));
                }
                self.budget.charge(ids.len())?;
                ids.reverse();
                let value = self.child(value, id, Role::MatchValue, visits)?;
                let flows = ids.iter().map(|(_, child)| self.flows[child.0]);
                let arms = self.circuit.any(flows, &mut self.budget)?;
                self.define(id, [self.flow(value), arms].into_iter())?;
                self.witness.nodes[id.0].kind = SourceKind::Match(value, ids);
                return Ok(());
            }
        };
        self.define(id, std::iter::empty())?;
        self.witness.nodes[id.0].kind = kind;
        Ok(())
    }
    fn build(mut self) -> Result<Self, String> {
        self.budget.charge(4)?;
        let main = *self.names.get("main").ok_or(INVALID)?;
        self.function(main)?;
        while let Some(source) = self.pending_functions.pop_front() {
            self.budget.charge(1)?;
            if !self.scope.is_empty() {
                return Err(INVALID.into());
            }
            let function = &self.checked.program.functions[source];
            self.budget.charge(function.name.len())?;
            let signature = self.checked.functions.get(&function.name).ok_or(INVALID)?;
            if signature.params.len() != function.params.len() {
                return Err(INVALID.into());
            }
            self.budget.charge(function.params.len())?;
            let mut params = Vec::with_capacity(function.params.len());
            for (ordinal, (parameter, ty)) in
                function.params.iter().zip(&signature.params).enumerate()
            {
                let binding = self.binding(
                    BindingSite::Parameter { source, ordinal },
                    &parameter.name,
                    parameter.mode == ParamMode::Inout,
                    parameter.mode,
                )?;
                self.bind(&parameter.name, binding)?;
                params.push((binding, copy_type(ty, &mut self.budget, 0)?));
            }
            let body = self.node(&function.body, source, None)?;
            let mut visits = Vec::new();
            self.push(&mut visits, Visit::Expr(&function.body, body))?;
            while let Some(visit) = visits.pop() {
                self.budget.charge(1)?;
                match visit {
                    Visit::Expr(expr, id) => self.expression(expr, id, &mut visits)?,
                    Visit::Bind(name, id) => self.bind(name, id)?,
                    Visit::Unbind(name, id) => self.unbind(name, id)?,
                }
            }
            self.budget.charge(params.len())?;
            for (parameter, (binding, _)) in function.params.iter().zip(&params).rev() {
                self.unbind(&parameter.name, *binding)?;
            }
            self.circuit.define(
                self.functions[&source],
                Operator::All,
                [self.flow(body)].into_iter(),
                &mut self.budget,
            )?;
            let name = self.budget.text(&function.name)?;
            let result = copy_type(&signature.result, &mut self.budget, 0)?;
            self.budget.charge(signature.quantified.len())?;
            let quantified = signature.quantified.clone();
            self.budget.charge(1)?;
            self.witness.functions.push(SourceFunction {
                source,
                name,
                params,
                quantified,
                result,
                body,
            });
        }
        Ok(self)
    }
}

pub(super) struct SourceFacts<'a, 's> {
    _checked: &'a CheckedProgram<'s>,
    witness: Witness,
    witness_work: usize,
    functions: HashMap<usize, GateId>,
    expression_ids: HashMap<*const Expr, NodeId>,
    flows: Vec<GateId>,
    values: Vec<bool>,
}
impl<'a, 's> SourceFacts<'a, 's> {
    pub fn build(checked: &'a CheckedProgram<'s>) -> Result<Self, String> {
        let mut builder = SourceBuilder::new(checked)?.build()?;
        let witness_work = builder.budget.used;
        let values = builder.circuit.solve(&mut builder.budget)?;
        Ok(Self {
            _checked: checked,
            witness: builder.witness,
            witness_work,
            functions: builder.functions,
            expression_ids: builder.expression_ids,
            flows: builder.flows,
            values,
        })
    }
    pub fn function_may_return(&self, source: usize) -> Option<bool> {
        self.functions.get(&source).map(|id| self.values[id.0])
    }
    pub fn expression_may_return(&self, expression: &Expr) -> Option<bool> {
        self.expression_ids
            .get(&(expression as *const Expr))
            .map(|id| self.values[self.flows[id.0].0])
    }
    #[cfg(test)]
    pub fn binding_used(&self, expression: &Expr) -> Option<bool> {
        let id = self.expression_ids.get(&(expression as *const Expr))?;
        let SourceKind::Let(binding, _, _) = self.witness.nodes[id.0].kind else {
            return None;
        };
        Some(self.witness.bindings[binding.0].uses != 0)
    }
    pub fn global_constructor_reference(&self, expression: &Expr) -> Result<bool, String> {
        let id = self.occurrence(expression)?;
        Ok(matches!(
            &self.witness.nodes[id].kind,
            SourceKind::Var(Reference::Global(_)) | SourceKind::Call(CallTarget::Other(_), _, _)
        ))
    }
    pub fn occurrence(&self, expression: &Expr) -> Result<usize, String> {
        self.expression_ids
            .get(&(expression as *const Expr))
            .map(|id| id.0)
            .ok_or(INVALID.into())
    }
    pub fn discard_binding(
        &self,
        expression: &Expr,
        domain: &super::logical::Domain,
    ) -> Result<super::NoStorageBinding, String> {
        let id = self.occurrence(expression)?;
        let SourceKind::Let(binding, _, _) = &self.witness.nodes[id].kind else {
            return Err(INVALID.into());
        };
        let canonical = &self.witness.bindings[binding.0];
        if canonical.mutable || canonical.uses != 0 {
            return Err(
                "E-TYPED-UNSUPPORTED: discarded initializer requires an unused immutable binding"
                    .into(),
            );
        }
        Ok(super::NoStorageBinding {
            domain: domain.clone(),
            occurrence: binding.0,
        })
    }
    pub fn no_storage_binding(
        &self,
        expression: &Expr,
        domain: &super::logical::Domain,
    ) -> Result<super::NoStorageBinding, String> {
        let id = self.occurrence(expression)?;
        let SourceKind::Let(binding, value, _) = &self.witness.nodes[id].kind else {
            return Err(INVALID.into());
        };
        let canonical = &self.witness.bindings[binding.0];
        if canonical.mutable || canonical.uses != 0 || self.values[self.flows[value.0].0] {
            return Err("E-TYPED-UNSUPPORTED: layoutless initializer requires an unused immutable nonreturning binding".into());
        }
        Ok(super::NoStorageBinding {
            domain: domain.clone(),
            occurrence: binding.0,
        })
    }
    pub fn check_pure_subset(&self) -> Result<(), String> {
        self.check_depth()?;
        const UNSUPPORTED: &str = "E-TYPED-UNSUPPORTED: no-normal or discarded result lowering requires pure Copy computations without local shape elaboration";
        let mut budget = Budget::default();
        budget.charge(self.witness_work)?;
        let environment = &self.witness.environment;
        let mut schema_budget = super::logical::Budget::default();
        let dependencies =
            super::logical::schema_dependencies(&environment.types, &mut schema_budget)?;
        budget.charge(environment.types.len())?;
        let mut schemas = HashMap::new();
        for (declaration, dependency) in environment.types.iter().zip(dependencies) {
            budget.charge(declaration.name.len())?;
            schemas.insert(declaration.name.as_str(), dependency);
        }
        fn pure_type(
            ty: &Type,
            schemas: &HashMap<&str, super::logical::Dependencies>,
            budget: &mut Budget,
            depth: usize,
        ) -> Result<(), String> {
            budget.charge(1)?;
            if depth > MAX_TYPE_DEPTH {
                return Err(LIMIT.into());
            }
            match ty {
                Type::Int
                | Type::Bool
                | Type::Unit
                | Type::StaticStr
                | Type::U8x16
                | Type::Var(_) => {}
                Type::Bytes(n) if *n <= 256 => {}
                Type::Nominal(name, args) => {
                    budget.charge(name.len())?;
                    let dependency = schemas.get(name.as_str()).ok_or(INVALID)?;
                    if dependency.owner || dependency.native {
                        return Err(UNSUPPORTED.into());
                    }
                    budget.charge(args.len())?;
                    for arg in args {
                        pure_type(arg, schemas, budget, depth + 1)?;
                    }
                }
                _ => return Err(UNSUPPORTED.into()),
            }
            Ok(())
        }
        budget.charge(
            environment
                .externs
                .len()
                .checked_add(environment.constants.len())
                .ok_or(LIMIT)?,
        )?;
        let mut foreign = std::collections::HashSet::new();
        for function in &environment.externs {
            budget.charge(function.name.len())?;
            foreign.insert(function.name.as_str());
        }
        for constant in &environment.constants {
            budget.charge(constant.name.len())?;
            foreign.insert(constant.name.as_str());
        }
        budget.charge(self.witness.functions.len())?;
        for function in &self.witness.functions {
            budget.charge(function.params.len())?;
            for (_, ty) in &function.params {
                pure_type(ty, &schemas, &mut budget, 0)?;
            }
            pure_type(&function.result, &schemas, &mut budget, 0)?;
        }
        budget.charge(self.witness.nodes.len())?;
        for node in &self.witness.nodes {
            if node.elaborated_local {
                return Err(UNSUPPORTED.into());
            }
            pure_type(&node.ty, &schemas, &mut budget, 0)?;
            if let SourceKind::Call(CallTarget::Other(name), _, _)
            | SourceKind::Var(Reference::Global(name)) = &node.kind
            {
                budget.charge(name.len())?;
                if foreign.contains(name.as_str()) {
                    return Err(UNSUPPORTED.into());
                }
            }
        }
        Ok(())
    }
    /// Reconstruct from actual checked source, not from the retained witness.
    /// This is only source/witness equality. It is not a source/IR relation.
    #[cfg(test)]
    pub fn verify_source(&self, checked: &CheckedProgram<'_>) -> Result<(), String> {
        let mut rebuilt = SourceBuilder::new(checked)?.build()?;
        rebuilt.budget.charge(self.witness_work)?;
        if self.witness != rebuilt.witness {
            return Err(
                "E-TYPED-CONTROL: checked source differs from retained control witness".into(),
            );
        }
        Ok(())
    }
}

/// Exact-instance facts reconstructed solely from actual stored IR operations.
/// There is deliberately no access to the source circuit or any claimed fact.
pub(super) struct IrFacts<'a> {
    _program: &'a TypedMain,
    expressions: HashMap<*const ExprNode, GateId>,
    functions: Vec<GateId>,
    no_normal: HashMap<*const NoNormalExpr, GateId>,
    discarded: HashMap<*const DiscardExpr, GateId>,
    values: Vec<bool>,
}
#[derive(Clone, Copy)]
enum IrNode<'a> {
    Value(&'a ExprNode),
    NoNormal(&'a NoNormalExpr),
    Discarded(&'a DiscardExpr),
}
struct IrBuilder<'a> {
    program: &'a TypedMain,
    budget: Budget,
    circuit: Circuit,
    functions: Vec<GateId>,
    expressions: HashMap<*const ExprNode, GateId>,
    no_normal: HashMap<*const NoNormalExpr, GateId>,
    discarded: HashMap<*const DiscardExpr, GateId>,
    pending: Vec<(IrNode<'a>, GateId)>,
}
impl<'a> IrBuilder<'a> {
    fn expression(&mut self, expression: &'a ExprNode) -> Result<GateId, String> {
        self.budget.charge(1)?;
        if self
            .expressions
            .contains_key(&(expression as *const ExprNode))
        {
            // Structured IR nodes have one structural occurrence. Function-call
            // cycles use the reserved function gates, never expression sharing.
            return Err(INVALID.into());
        }
        let id = self.circuit.reserve(&mut self.budget)?;
        self.budget.charge(2)?;
        self.expressions.insert(expression as *const ExprNode, id);
        self.pending.push((IrNode::Value(expression), id));
        Ok(id)
    }
    fn no_normal(&mut self, expression: &'a NoNormalExpr) -> Result<GateId, String> {
        self.budget.charge(1)?;
        if self
            .no_normal
            .contains_key(&(expression as *const NoNormalExpr))
        {
            return Err(INVALID.into());
        }
        let id = self.circuit.reserve(&mut self.budget)?;
        self.budget.charge(2)?;
        self.no_normal.insert(expression as *const NoNormalExpr, id);
        self.pending.push((IrNode::NoNormal(expression), id));
        Ok(id)
    }
    fn discarded(&mut self, expression: &'a DiscardExpr) -> Result<GateId, String> {
        self.budget.charge(1)?;
        if self
            .discarded
            .contains_key(&(expression as *const DiscardExpr))
        {
            return Err(INVALID.into());
        }
        let id = self.circuit.reserve(&mut self.budget)?;
        self.budget.charge(2)?;
        self.discarded.insert(expression as *const DiscardExpr, id);
        self.pending.push((IrNode::Discarded(expression), id));
        Ok(id)
    }
    fn discarded_node(&mut self, expression: &'a DiscardExpr, id: GateId) -> Result<(), String> {
        self.budget.charge(1)?;
        let inputs = match &expression.kind {
            DiscardKind::Value(value) => {
                self.budget.charge(1)?;
                vec![self.expression(value)?]
            }
            DiscardKind::FromNoNormal(value) => {
                self.budget.charge(1)?;
                vec![self.no_normal(value)?]
            }
            DiscardKind::Call(callee, args) => self.call(*callee, args)?,
            DiscardKind::Constructor { fields, .. } => {
                self.budget.charge(fields.len())?;
                let mut inputs = Vec::with_capacity(fields.len());
                for field in fields {
                    inputs.push(self.discarded(field)?);
                }
                inputs
            }
            DiscardKind::Record { fields, .. } => {
                self.budget.charge(fields.len())?;
                let mut inputs = Vec::with_capacity(fields.len());
                for (_, field) in fields {
                    inputs.push(self.discarded(field)?);
                }
                inputs
            }
            DiscardKind::Let { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.expression(value)?, self.discarded(body)?]
            }
            DiscardKind::DiscardLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.discarded(value)?, self.discarded(body)?]
            }
            DiscardKind::NoStorageLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.no_normal(value)?, self.discarded(body)?]
            }
            DiscardKind::If { condition, yes, no } => {
                let condition = self.expression(condition)?;
                let yes = self.discarded(yes)?;
                let no = self.discarded(no)?;
                let arms = self.circuit.any([yes, no].into_iter(), &mut self.budget)?;
                self.budget.charge(2)?;
                vec![condition, arms]
            }
            DiscardKind::Match { value, arms } => {
                if arms.is_empty() {
                    return Err(INVALID.into());
                }
                let value = self.expression(value)?;
                self.budget.charge(arms.len())?;
                let mut flows = Vec::with_capacity(arms.len());
                for arm in arms {
                    flows.push(self.discarded(&arm.body)?);
                }
                let arms = self.circuit.any(flows.into_iter(), &mut self.budget)?;
                self.budget.charge(2)?;
                vec![value, arms]
            }
        };
        self.circuit
            .define(id, Operator::All, inputs.into_iter(), &mut self.budget)
    }
    fn call(&mut self, callee: usize, args: &'a [CallArg]) -> Result<Vec<GateId>, String> {
        let function = self.program.functions.get(callee).ok_or(INVALID)?;
        if args.len() != function.params.len() {
            return Err(INVALID.into());
        }
        let callee = *self.functions.get(callee).ok_or(INVALID)?;
        self.budget
            .charge(args.len().checked_add(1).ok_or(LIMIT)?)?;
        let mut inputs = Vec::with_capacity(args.len() + 1);
        for argument in args {
            self.budget.charge(1)?;
            match argument {
                CallArg::Value(value) | CallArg::ReadTemporary(value) => {
                    inputs.push(self.expression(value)?)
                }
                CallArg::ReadCopy(_)
                | CallArg::InoutCopy(_)
                | CallArg::ReadField(_)
                | CallArg::InoutField(_)
                | CallArg::Move(_)
                | CallArg::ReadOwner(_)
                | CallArg::InoutOwner(_) => {}
            }
        }
        inputs.push(callee);
        Ok(inputs)
    }
    fn interrupted(&mut self, call: &'a super::InterruptedCall) -> Result<Vec<GateId>, String> {
        self.budget
            .charge(call.prefix.len().checked_add(1).ok_or(LIMIT)?)?;
        let mut inputs = Vec::with_capacity(call.prefix.len() + 1);
        for value in &call.prefix {
            inputs.push(self.expression(value)?);
        }
        inputs.push(match &*call.barrier {
            Computation::Value(value) => self.expression(value)?,
            Computation::Discarded(value) => self.discarded(value)?,
            Computation::NoNormal(value) => self.no_normal(value)?,
        });
        Ok(inputs)
    }
    fn no_normal_node(&mut self, expression: &'a NoNormalExpr, id: GateId) -> Result<(), String> {
        self.budget.charge(1)?;
        // Neither the NoNormal tag nor its logical identity contributes a false
        // seed. Every actual child/callee is independently reconstructed.
        let inputs = match &expression.kind {
            NoNormalKind::InterruptedCall(call) => self.interrupted(call)?,
            NoNormalKind::Call(callee, args) => self.call(*callee, args)?,
            NoNormalKind::Let { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.expression(value)?, self.no_normal(body)?]
            }
            NoNormalKind::DiscardLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.discarded(value)?, self.no_normal(body)?]
            }
            NoNormalKind::NoStorageLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.no_normal(value)?, self.no_normal(body)?]
            }
            NoNormalKind::If { condition, yes, no } => {
                let condition = self.expression(condition)?;
                let yes = self.no_normal(yes)?;
                let no = self.no_normal(no)?;
                let arms = self.circuit.any([yes, no].into_iter(), &mut self.budget)?;
                self.budget.charge(2)?;
                vec![condition, arms]
            }
            NoNormalKind::Match { value, arms } => {
                if arms.is_empty() {
                    return Err(INVALID.into());
                }
                let value = self.expression(value)?;
                self.budget.charge(arms.len())?;
                let mut flows = Vec::with_capacity(arms.len());
                for arm in arms {
                    flows.push(self.no_normal(&arm.body)?);
                }
                let arms = self.circuit.any(flows.into_iter(), &mut self.budget)?;
                self.budget.charge(2)?;
                vec![value, arms]
            }
        };
        self.circuit
            .define(id, Operator::All, inputs.into_iter(), &mut self.budget)
    }
    fn children(
        &mut self,
        expressions: impl ExactSizeIterator<Item = &'a ExprNode>,
    ) -> Result<Vec<GateId>, String> {
        self.budget.charge(expressions.len())?;
        let mut result = Vec::with_capacity(expressions.len());
        for expression in expressions {
            result.push(self.expression(expression)?);
        }
        Ok(result)
    }
    fn node(&mut self, expression: &'a ExprNode, id: GateId) -> Result<(), String> {
        self.budget.charge(1)?;
        let inputs = match &expression.kind {
            ExprKind::InterruptedCall(call) => self.interrupted(call)?,
            ExprKind::Call(callee, args) => self.call(*callee, args)?,
            ExprKind::DiscardLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.discarded(value)?, self.expression(body)?]
            }
            ExprKind::NoStorageLet { value, body, .. } => {
                self.budget.charge(2)?;
                vec![self.no_normal(value)?, self.expression(body)?]
            }
            ExprKind::ForeignCall(_, args) => {
                self.budget.charge(args.len())?;
                let mut inputs = Vec::with_capacity(args.len());
                for argument in args {
                    self.budget.charge(1)?;
                    if let ForeignArg::Value(value) = argument {
                        inputs.push(self.expression(value)?);
                    }
                }
                inputs
            }
            ExprKind::Let { value, body, .. } => {
                self.children([value.as_ref(), body.as_ref()].into_iter())?
            }
            ExprKind::OwnerLet { init, body, .. } => {
                let value = match init {
                    OwnerInit::Alloc(value)
                    | OwnerInit::BytesAlloc(value)
                    | OwnerInit::Call(value)
                    | OwnerInit::Expr(value) => Some(value.as_ref()),
                    OwnerInit::Move(_) => None,
                };
                self.budget.charge(2)?;
                let mut inputs = Vec::with_capacity(2);
                if let Some(value) = value {
                    inputs.push(self.expression(value)?);
                }
                inputs.push(self.expression(body)?);
                inputs
            }
            ExprKind::If(condition, yes, no) => {
                let condition = self.expression(condition)?;
                let yes = self.expression(yes)?;
                let no = self.expression(no)?;
                let arms = self.circuit.any([yes, no].into_iter(), &mut self.budget)?;
                self.circuit.define(
                    id,
                    Operator::All,
                    [condition, arms].into_iter(),
                    &mut self.budget,
                )?;
                return Ok(());
            }
            ExprKind::CopyMatch { value, arms } => {
                if arms.is_empty() {
                    return Err(INVALID.into());
                }
                let value = self.expression(value)?;
                let arms = self.children(arms.iter().map(|arm| &arm.body))?;
                let arms = self.circuit.any(arms.into_iter(), &mut self.budget)?;
                self.circuit.define(
                    id,
                    Operator::All,
                    [value, arms].into_iter(),
                    &mut self.budget,
                )?;
                return Ok(());
            }
            ExprKind::Match { value, arms } => {
                if arms.is_empty() {
                    return Err(INVALID.into());
                }
                let value = self.expression(value)?;
                let arms = self.children(arms.iter().map(|arm| &arm.body))?;
                let arms = self.circuit.any(arms.into_iter(), &mut self.budget)?;
                self.circuit.define(
                    id,
                    Operator::All,
                    [value, arms].into_iter(),
                    &mut self.budget,
                )?;
                return Ok(());
            }
            ExprKind::Binary(BinaryOp::And | BinaryOp::Or, left, right) => {
                let left = self.expression(left)?;
                self.expression(right)?;
                self.circuit
                    .define(id, Operator::All, [left].into_iter(), &mut self.budget)?;
                return Ok(());
            }
            ExprKind::While(condition, body) => {
                let condition = self.expression(condition)?;
                self.expression(body)?;
                self.circuit.define(
                    id,
                    Operator::All,
                    [condition].into_iter(),
                    &mut self.budget,
                )?;
                return Ok(());
            }
            ExprKind::TryAlloc { input: value, .. }
            | ExprKind::ForeignConvert(_, value)
            | ExprKind::IsNull(value)
            | ExprKind::FieldAssign(_, value)
            | ExprKind::ByteLen(value)
            | ExprKind::BytesLen(value)
            | ExprKind::OwnerBytesGet(_, value)
            | ExprKind::OwnerWrite(_, value)
            | ExprKind::Assign(_, value) => self.children([value.as_ref()].into_iter())?,
            ExprKind::Binary(_, left, right)
            | ExprKind::ByteAt(left, right)
            | ExprKind::BytesGet(left, right)
            | ExprKind::OwnerBytesSet(_, left, right) => {
                self.children([left.as_ref(), right.as_ref()].into_iter())?
            }
            ExprKind::BytesSet(value, index, byte) => {
                self.children([value.as_ref(), index.as_ref(), byte.as_ref()].into_iter())?
            }
            ExprKind::Record { fields, .. } => {
                self.children(fields.iter().map(|(_, value)| value))?
            }
            ExprKind::Constructor { fields, .. } | ExprKind::Vector(_, fields) => {
                self.children(fields.iter())?
            }
            ExprKind::Int(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Read(_)
            | ExprKind::FieldRead(_)
            | ExprKind::ForeignConst(_)
            | ExprKind::NullPointer
            | ExprKind::BytesNew(_)
            | ExprKind::OwnerBytesLen(_)
            | ExprKind::OwnerRead(_)
            | ExprKind::OwnerMove(_) => Vec::new(),
        };
        self.circuit
            .define(id, Operator::All, inputs.into_iter(), &mut self.budget)
    }
    fn build(mut self) -> Result<IrFacts<'a>, String> {
        if self.program.functions.len() > MAX_INSTANCES
            || self.program.entry >= self.program.functions.len()
        {
            return Err(INVALID.into());
        }
        self.budget.charge(self.program.functions.len())?;
        for _ in &self.program.functions {
            self.functions.push(self.circuit.reserve(&mut self.budget)?);
        }
        self.budget.charge(self.program.functions.len())?;
        for (ordinal, function) in self.program.functions.iter().enumerate() {
            let body = match &function.body {
                Computation::Value(value) => self.expression(value)?,
                Computation::Discarded(value) => self.discarded(value)?,
                Computation::NoNormal(value) => self.no_normal(value)?,
            };
            self.circuit.define(
                self.functions[ordinal],
                Operator::All,
                [body].into_iter(),
                &mut self.budget,
            )?;
        }
        while let Some((expression, id)) = self.pending.pop() {
            self.budget.charge(1)?;
            match expression {
                IrNode::Value(value) => self.node(value, id)?,
                IrNode::NoNormal(value) => self.no_normal_node(value, id)?,
                IrNode::Discarded(value) => self.discarded_node(value, id)?,
            }
        }
        let values = self.circuit.solve(&mut self.budget)?;
        Ok(IrFacts {
            _program: self.program,
            expressions: self.expressions,
            functions: self.functions,
            no_normal: self.no_normal,
            discarded: self.discarded,
            values,
        })
    }
}
impl<'a> IrFacts<'a> {
    pub fn build(program: &'a TypedMain) -> Result<Self, String> {
        IrBuilder {
            program,
            budget: Budget::default(),
            circuit: Circuit::default(),
            functions: Vec::new(),
            expressions: HashMap::new(),
            no_normal: HashMap::new(),
            discarded: HashMap::new(),
            pending: Vec::new(),
        }
        .build()
    }
    pub fn function_may_return(&self, function: usize) -> Option<bool> {
        self.functions.get(function).map(|id| self.values[id.0])
    }
    pub fn computation_may_return(&self, expression: &Computation) -> Option<bool> {
        match expression {
            Computation::Value(value) => self
                .expressions
                .get(&(value as *const ExprNode))
                .map(|id| self.values[id.0]),
            Computation::NoNormal(value) => self.no_normal_may_return(value),
            Computation::Discarded(value) => self
                .discarded
                .get(&(value as *const DiscardExpr))
                .map(|id| self.values[id.0]),
        }
    }
    pub fn no_normal_may_return(&self, expression: &NoNormalExpr) -> Option<bool> {
        self.no_normal
            .get(&(expression as *const NoNormalExpr))
            .map(|id| self.values[id.0])
    }
}

#[cfg(test)]
mod circuit_tests {
    use super::*;

    #[test]
    fn exact_work_boundary_is_not_a_false_result() {
        let mut budget = Budget { used: MAX_WORK - 1 };
        assert!(budget.charge(1).is_ok());
        assert_eq!(budget.used, MAX_WORK);
        assert_eq!(budget.charge(1).unwrap_err(), LIMIT);
        assert_eq!(Budget::default().charge(usize::MAX).unwrap_err(), LIMIT);
    }

    #[test]
    fn pending_and_out_of_range_edges_are_invalid() {
        let mut budget = Budget::default();
        let mut graph = Circuit::default();
        let id = graph.reserve(&mut budget).unwrap();
        assert_eq!(graph.solve(&mut budget).unwrap_err(), INVALID);
        assert_eq!(
            graph
                .define(id, Operator::All, [GateId(9)].into_iter(), &mut budget)
                .unwrap_err(),
            INVALID
        );
        graph
            .define(id, Operator::All, std::iter::empty(), &mut budget)
            .unwrap();
        assert_eq!(
            graph
                .define(id, Operator::Any, std::iter::empty(), &mut budget)
                .unwrap_err(),
            INVALID
        );
    }

    #[test]
    fn cycle_with_no_seed_stays_false_and_base_arm_propagates() {
        let mut budget = Budget::default();
        let mut graph = Circuit::default();
        let a = graph.reserve(&mut budget).unwrap();
        let b = graph.reserve(&mut budget).unwrap();
        graph
            .define(a, Operator::All, [b].into_iter(), &mut budget)
            .unwrap();
        graph
            .define(b, Operator::All, [a].into_iter(), &mut budget)
            .unwrap();
        assert_eq!(graph.solve(&mut budget).unwrap(), [false, false]);
        let returning = graph.all(std::iter::empty(), &mut budget).unwrap();
        let base = graph.any([a, returning].into_iter(), &mut budget).unwrap();
        assert!(graph.solve(&mut budget).unwrap()[base.0]);
        // Independently mutate the actual graph edge. No prior false summary is
        // reused, and a new returning route reaches the recursive group.
        graph.gates[b.0] = Gate {
            operator: Operator::Any,
            inputs: vec![a, base],
        };
        let values = graph.solve(&mut budget).unwrap();
        assert!(values[a.0] && values[b.0]);
    }

    #[test]
    fn duplicate_edges_and_high_fan_in_are_counted_exactly() {
        let mut budget = Budget::default();
        let mut graph = Circuit::default();
        let yes = graph.all(std::iter::empty(), &mut budget).unwrap();
        let no = graph.any(std::iter::empty(), &mut budget).unwrap();
        let repeated = graph
            .all(std::iter::repeat_n(yes, 2048), &mut budget)
            .unwrap();
        let mixed = graph.all([yes, no, yes].into_iter(), &mut budget).unwrap();
        let values = graph.solve(&mut budget).unwrap();
        assert!(values[repeated.0]);
        assert!(!values[mixed.0]);
    }

    #[test]
    fn deep_circuit_propagates_iteratively_without_an_iteration_cutoff() {
        let mut budget = Budget::default();
        let mut graph = Circuit::default();
        let mut previous = graph.all(std::iter::empty(), &mut budget).unwrap();
        for _ in 0..2048 {
            previous = graph.all([previous].into_iter(), &mut budget).unwrap();
        }
        assert!(graph.solve(&mut budget).unwrap()[previous.0]);
    }

    #[test]
    fn exhausted_solver_cannot_be_mistaken_for_no_return() {
        let mut budget = Budget::default();
        let mut graph = Circuit::default();
        let id = graph.all(std::iter::empty(), &mut budget).unwrap();
        let mut exhausted = Budget { used: MAX_WORK };
        assert_eq!(graph.solve(&mut exhausted).unwrap_err(), LIMIT);
        assert!(graph.solve(&mut budget).unwrap()[id.0]);
    }
}
