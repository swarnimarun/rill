//! Retained call evidence is source syntax, not an executable fake instance.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::typed) struct InterruptedEvidence {
    pub(super) domain: super::super::logical::Domain,
    pub(super) occurrence: NodeId,
    pub(super) target: usize,
    pub(super) parameters: Vec<Type>,
    pub(super) result: Type,
    definition: SourceFunction,
    pub(super) syntax: Vec<(NodeId, SourceNode)>,
    patterns: Vec<SourcePattern>,
    bindings: Vec<Binding>,
}

impl InterruptedEvidence {
    pub(super) fn charge(&self, budget: &mut Budget) -> Result<(), String> {
        budget.charge(self.parameters.len())?;
        for ty in &self.parameters {
            budget.reserve_type(ty, 0)?;
        }
        budget.reserve_type(&self.result, 0)?;
        budget.charge(self.syntax.len())?;
        for (_, node) in &self.syntax {
            relation::charge_node(node, budget)?;
        }
        relation::charge_function(&self.definition, budget)?;
        budget.charge(self.patterns.len())?;
        for pattern in &self.patterns {
            relation::charge_pattern(pattern, budget)?;
        }
        budget.charge(self.bindings.len())?;
        for binding in &self.bindings {
            budget.charge(binding.name.len().saturating_add(1))?;
        }
        Ok(())
    }
    pub(super) fn capture(
        witness: &Witness,
        domain: &super::super::logical::Domain,
        occurrence: NodeId,
        parameters: Vec<Type>,
        result: Type,
        budget: &mut Budget,
    ) -> Result<Self, String> {
        // Reserve all retained syntax, signature, pattern and binding copies and
        // their later exact comparisons before cloning any node. This deliberately
        // conservative whole-witness bound also limits nested-call quadratic work.
        relation::charge_witness(witness, budget)?;
        relation::charge_witness(witness, budget)?;
        let node = witness.nodes.get(occurrence.0).ok_or(INVALID)?;
        let SourceKind::Call(CallTarget::Function(target), _, _) = &node.kind else {
            return Err(INVALID.into());
        };
        budget.charge(witness.functions.len())?;
        let definition = witness
            .functions
            .iter()
            .find(|f| f.source == *target)
            .ok_or(INVALID)?
            .clone();
        budget.charge(witness.nodes.len().checked_mul(2).ok_or(LIMIT)?)?;
        let mut selected = vec![false; witness.nodes.len()];
        let mut syntax = Vec::new();
        for (index, node) in witness.nodes.iter().enumerate() {
            let belongs = index == occurrence.0
                || node
                    .parent
                    .is_some_and(|(parent, _)| selected.get(parent.0) == Some(&true));
            selected[index] = belongs;
            if belongs {
                syntax.push((NodeId(index), node.clone()));
            }
        }
        Ok(Self {
            domain: domain.clone(),
            occurrence,
            target: *target,
            parameters,
            result,
            definition,
            syntax,
            patterns: witness.patterns.clone(),
            bindings: witness.bindings.clone(),
        })
    }
}

impl SourceFacts<'_, '_> {
    pub(in crate::typed) fn interrupted_evidence(
        &self,
        expression: &Expr,
        instances: &super::super::instances::Instances,
        context: &super::super::instances::Substitution,
    ) -> Result<InterruptedEvidence, String> {
        let occurrence = NodeId(self.occurrence(expression)?);
        let mut budget = instances.control_budget()?;
        let Expr::Call(_, args) = expression else {
            return Err(INVALID.into());
        };
        budget.charge(args.len())?;
        let mut parameters = Vec::with_capacity(args.len());
        for arg in args {
            parameters.push(logical_substitute(
                self._checked.expression_type(arg).ok_or(INVALID)?,
                context,
                &mut budget,
                0,
            )?);
        }
        let result = logical_substitute(
            self._checked.expression_type(expression).ok_or(INVALID)?,
            context,
            &mut budget,
            0,
        )?;
        InterruptedEvidence::capture(
            &self.witness,
            &instances.domain,
            occurrence,
            parameters,
            result,
            &mut budget,
        )
    }
}
fn logical_substitute(
    ty: &Type,
    context: &super::super::instances::Substitution,
    budget: &mut Budget,
    depth: usize,
) -> Result<Type, String> {
    budget.charge(1)?;
    if depth > MAX_TYPE_DEPTH {
        return Err(LIMIT.into());
    }
    Ok(match ty {
        Type::Var(id) => copy_type(context.get(id).ok_or(INVALID)?, budget, depth)?,
        Type::Nominal(name, args) => {
            let name = budget.text(name)?;
            budget.charge(args.len())?;
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(logical_substitute(arg, context, budget, depth + 1)?);
            }
            Type::Nominal(name, values)
        }
        _ => copy_type(ty, budget, depth)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser, typed};

    fn lower(source: &str) -> typed::TypedMain {
        let parsed = parser::parse(source).unwrap();
        typed::lower_main(&check::analyze(&parsed).unwrap()).unwrap()
    }
    fn source() -> &'static str {
        "fn spin(){spin()} fn ignore(a,b,c,d){42} fn main(){ignore(11,22,spin(),33)}"
    }
    fn call(program: &mut typed::TypedMain) -> &mut typed::InterruptedCall {
        let Computation::Value(value) = &mut program.functions[program.entry].body else {
            panic!()
        };
        let ExprKind::InterruptedCall(call) = &mut value.kind else {
            panic!()
        };
        call
    }
    fn reject(program: &typed::TypedMain) {
        assert!(typed::verify(program).is_err());
        assert!(typed::evaluate(program).is_err());
        assert!(typed::emit_c(program).is_err());
    }

    #[test]
    fn altered_barrier_prefix_order_and_missing_proof_reject_before_consumers() {
        let original = lower(source());
        let mut changed = original.clone();
        call(&mut changed).prefix.swap(0, 1);
        reject(&changed);
        let mut changed = original.clone();
        call(&mut changed).prefix.pop();
        reject(&changed);
        let mut changed = original.clone();
        *call(&mut changed).barrier = Computation::Value(ExprNode {
            ty: typed::Type::Int,
            kind: ExprKind::Int(0),
        });
        assert_eq!(
            IrFacts::build(&changed)
                .unwrap()
                .function_may_return(changed.entry),
            Some(true)
        );
        reject(&changed);
        let mut changed = original;
        changed.logical.control = None;
        reject(&changed);
    }
    #[test]
    fn interrupted_identity_full_signature_and_retained_suffix_are_not_hints() {
        let original = lower(source());
        for mutation in 0..9 {
            let mut changed = original.clone();
            let call = call(&mut changed);
            match mutation {
                0 => call.evidence.domain = typed::logical::Domain::default(),
                1 => call.evidence.occurrence = NodeId(usize::MAX),
                2 => call.evidence.target = 0,
                3 => call.evidence.parameters[0] = Type::Bool,
                4 => call.evidence.result = Type::Bool,
                5 => call.evidence.definition.params.pop().map(|_| ()).unwrap(),
                6 => call.evidence.definition.quantified.clear(),
                7 => {
                    call.evidence
                        .syntax
                        .iter_mut()
                        .find(|(_, n)| matches!(n.kind, SourceKind::Int(33)))
                        .unwrap()
                        .1
                        .kind = SourceKind::Int(44)
                }
                8 => {
                    call.evidence.syntax.pop();
                }
                _ => unreachable!(),
            }
            reject(&changed);
        }
    }
    #[test]
    fn concrete_nested_barrier_flow_does_not_trust_the_interrupted_tag() {
        let mut program =
            lower("fn spin(){spin()} fn ignore(x){42} fn main(){ignore(ignore(spin()))}");
        let outer = call(&mut program);
        let Computation::Value(inner) = &mut *outer.barrier else {
            panic!()
        };
        inner.kind = ExprKind::Int(42);
        let facts = IrFacts::build(&program).unwrap();
        assert_eq!(facts.function_may_return(program.entry), Some(true));
        reject(&program);
    }
    #[test]
    fn original_checked_source_is_separate_from_its_frozen_witness() {
        let program = lower(source());
        let changed = parser::parse(&source().replace("33", "44")).unwrap();
        let checked = check::analyze(&changed).unwrap();
        assert!(program
            .logical
            .control
            .as_ref()
            .unwrap()
            .verify_source(&checked)
            .is_err());
        typed::verify(&program).unwrap();
    }
    #[test]
    fn exact_retained_evidence_budget_and_deep_forged_types_fail_hard() {
        let mut program = lower(source());
        let evidence = &call(&mut program).evidence;
        let mut measured = Budget::default();
        evidence.charge(&mut measured).unwrap();
        let cost = measured.used;
        evidence
            .charge(&mut Budget {
                used: MAX_WORK - cost,
            })
            .unwrap();
        assert_eq!(
            evidence
                .charge(&mut Budget {
                    used: MAX_WORK - cost + 1
                })
                .unwrap_err(),
            LIMIT
        );
        let mut nested = Type::Int;
        for _ in 0..129 {
            nested = Type::Nominal("Wrapper".into(), vec![nested]);
        }
        call(&mut program).evidence.parameters[0] = nested;
        assert!(typed::verify(&program)
            .unwrap_err()
            .contains("CONTROL-COMPLEXITY"));
        reject(&program);
    }
    #[test]
    fn unexpected_barrier_return_has_no_fabricated_runtime_value() {
        let mut program = lower(source());
        let interrupted = call(&mut program);
        *interrupted.barrier = Computation::Value(ExprNode {
            ty: typed::Type::Int,
            kind: ExprKind::Int(7),
        });
        let interrupted = interrupted.clone();
        let error = typed::results::evaluate_interrupted(
            &interrupted,
            &mut typed::CopyLocals::default(),
            &program,
            0,
            &mut typed::EvalState::default(),
            &mut HashMap::new(),
        )
        .unwrap_err();
        assert!(error.contains("unexpectedly returned"));
    }
}
