//! The narrow source-directed syntax for computations without runtime storage.
//! This is an admission proposal, never a proof: frozen source correspondence,
//! independent return facts and ordinary typed verification must all succeed.
use super::*;

impl Lower<'_, '_> {
    pub(super) fn interrupted_call(
        &mut self,
        expression: &Expr,
    ) -> Result<Option<InterruptedCall>, String> {
        let Some(facts) = self.control else {
            return Ok(None);
        };
        let Some(index) = self
            .instances
            .interruption(self.checked, expression, facts)?
        else {
            return Ok(None);
        };
        let Expr::Call(_, args) = expression else {
            unreachable!()
        };
        let evidence = facts.interrupted_evidence(expression, self.instances, self.substitution)?;
        self.instances.charge_control(index + 1)?;
        let mut prefix = Vec::with_capacity(index);
        for arg in &args[..index] {
            let value = self.expr(arg)?;
            if value.ty.is_owner() || value.ty.is_raw() {
                return Err("E-TYPED-UNSUPPORTED: interrupted prefix requires Copy storage".into());
            }
            prefix.push(value);
        }
        let barrier = if matches!(self.source_type(&args[index])?, InferredType::Var(_)) {
            Computation::NoNormal(self.no_normal(&args[index])?)
        } else {
            Computation::Value(self.expr(&args[index])?)
        };
        Ok(Some(InterruptedCall {
            evidence,
            prefix,
            barrier: Box::new(barrier),
        }))
    }

    pub(super) fn no_normal(&mut self, expression: &Expr) -> Result<NoNormalExpr, String> {
        self.instances.charge_control(1)?;
        let facts = self.control.ok_or(instances::BARE_RESULT)?;
        if facts.expression_may_return(expression) != Some(false) {
            return Err(instances::BARE_RESULT.into());
        }
        let InferredType::Var(index) = self.source_type(expression)? else {
            return Err("E-TYPED-CONTROL: no-normal syntax requires a bare logical result".into());
        };
        let result = self.registry.logical.opaque_id(index)?;
        let occurrence = facts.occurrence(expression)?;
        let kind = match expression {
            Expr::Call(name, args) => {
                if let Some(call) = self.interrupted_call(expression)? {
                    return Ok(NoNormalExpr {
                        result,
                        occurrence,
                        kind: NoNormalKind::InterruptedCall(Box::new(call)),
                    });
                }
                let (callee, args) = self.call_arguments(expression, name, args)?;
                NoNormalKind::Call(callee, args)
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                if matches!(self.source_type(value)?, InferredType::Var(_)) {
                    let binding = facts.no_storage_binding(expression, &self.instances.domain)?;
                    NoNormalKind::NoStorageLet {
                        binding,
                        value: Box::new(self.no_normal(value)?),
                        body: Box::new(self.no_normal(body)?),
                    }
                } else {
                    let value = self.expr(value)?;
                    if value.ty.is_owner() || value.ty.is_raw() {
                        return Err(
                            "E-TYPED-UNSUPPORTED: no-normal prefix requires Copy storage".into(),
                        );
                    }
                    let local = self.fresh()?;
                    let mutable = matches!(expression, Expr::LetMut(..));
                    let previous = self.locals.insert(
                        name.clone(),
                        Local {
                            id: local,
                            ty: value.ty,
                            mutable,
                            initialization: Initialization::full(value.ty, &self.registry.types)?,
                            loaned: false,
                            shared: 0,
                            field_loans: Vec::new(),
                            mode: ParamMode::Value,
                        },
                    );
                    let body = self.no_normal(body);
                    if let Some(previous) = previous {
                        self.locals.insert(name.clone(), previous);
                    } else {
                        self.locals.remove(name);
                    }
                    NoNormalKind::Let {
                        local,
                        mutable,
                        value: Box::new(value),
                        body: Box::new(body?),
                    }
                }
            }
            Expr::If(condition, yes, no) => {
                let condition = self.expr(condition)?;
                let before = self.locals.clone();
                let yes = self.no_normal(yes)?;
                let after = self.locals.clone();
                self.locals = before;
                let no = self.no_normal(no)?;
                if !Self::same_states(&after, &self.locals) {
                    return Err("E-TYPED-CONTROL: incompatible branch state".into());
                }
                self.locals = after;
                NoNormalKind::If {
                    condition: Box::new(condition),
                    yes: Box::new(yes),
                    no: Box::new(no),
                }
            }
            Expr::Match(value, arms) => {
                let value = self.expr(value)?;
                if value.ty.is_owner() || value.ty.is_raw() {
                    return Err("E-TYPED-UNSUPPORTED: no-normal match requires Copy storage".into());
                }
                let outer = self.locals.clone();
                self.instances.charge_control(arms.len())?;
                let mut lowered = Vec::with_capacity(arms.len());
                let mut joined = None;
                for arm in arms {
                    self.locals = outer.clone();
                    let (pattern, _) = self.copy_pattern(&arm.pattern, value.ty)?;
                    let body = self.no_normal(&arm.body)?;
                    let after = outer
                        .iter()
                        .map(|(name, old)| {
                            let local = self
                                .locals
                                .get(name)
                                .filter(|local| local.id == old.id)
                                .unwrap_or(old)
                                .clone();
                            (name.clone(), local)
                        })
                        .collect();
                    if let Some(first) = &joined {
                        if !Self::same_states(first, &after) {
                            return Err("E-TYPED-CONTROL: incompatible match state".into());
                        }
                    } else {
                        joined = Some(after);
                    }
                    lowered.push(NoNormalArm { pattern, body });
                }
                self.locals = joined.unwrap_or(outer);
                NoNormalKind::Match {
                    value: Box::new(value),
                    arms: lowered,
                }
            }
            _ => return Err("E-TYPED-UNSUPPORTED: unsupported layoutless computation".into()),
        };
        Ok(NoNormalExpr {
            result,
            occurrence,
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{check, parser, typed};
    fn lower(source: &str) -> (crate::ast::Program, typed::TypedMain, String) {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let value = typed::lower_main(&checked).unwrap();
        let c = typed::emit_c(&value).unwrap();
        (program, value, c)
    }

    fn rejected_by_all_consumers(program: &typed::TypedMain) {
        assert!(typed::verify(program).is_err());
        assert!(typed::evaluate(program).is_err());
        assert!(typed::emit_c(program).is_err());
    }

    #[test]
    fn interrupted_eager_calls_keep_real_barriers_and_no_outer_instances() {
        for source in [
            "fn spin() { spin() } fn ignore(x) { 42 } fn main() { ignore(spin()) }",
            "fn spin() { spin() } fn ignore(x) { 42 } fn main() { ignore(ignore(spin())) }",
            "fn spin() { spin() } fn id(x) { x } fn main() { id(spin()) }",
            "type O<a> = None | Some(a) fn spin() { spin() } fn ignore(a, b) { 42 } fn main() { ignore(spin(), None) }",
            "fn spin() { spin() } fn ignore(a, b, c) { 42 } fn main() { ignore(1, spin(), 1 / 0) }",
        ] {
            let (program, typed, c) = lower(source);
            assert_eq!(typed.functions.len(), 2, "{source}");
            assert_eq!(typed::evaluate(&typed).unwrap_err(), "runtime trap", "{source}");
            assert_eq!(crate::interpreter::run_reference(&program), typed::evaluate(&typed));
            assert_eq!(crate::backend::emit(&program).unwrap(), c);
            assert!(!c.contains("rl_value"));
        }
    }

    #[test]
    fn frozen_correspondence_rejects_value_mode_return_and_cross_compilation_mutations() {
        use typed::{CallArg, Computation, ExprKind, ExprNode, NoNormalKind, ResultRepr, Type};
        let source = "fn forever(n: i32) { forever(n + 1) } fn main() { forever(0) }";
        let (_, original, _) = lower(source);
        let (_, other, _) = lower(source);
        let mut wrong_brand = original.clone();
        wrong_brand.functions[0].result = other.functions[0].result.clone();
        rejected_by_all_consumers(&wrong_brand);
        let mut returned = original.clone();
        returned.functions[0].result = ResultRepr::Value(Type::Int);
        returned.functions[0].body = Computation::Value(ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(0),
        });
        rejected_by_all_consumers(&returned);
        let mut changed = original.clone();
        let Computation::NoNormal(body) = &mut changed.functions[changed.entry].body else {
            panic!()
        };
        let NoNormalKind::Call(_, args) = &mut body.kind else {
            panic!()
        };
        let CallArg::Value(value) = &mut args[0] else {
            panic!()
        };
        value.kind = ExprKind::Int(1);
        rejected_by_all_consumers(&changed);
        let mut changed = original;
        let Computation::NoNormal(body) = &mut changed.functions[changed.entry].body else {
            panic!()
        };
        let NoNormalKind::Call(_, args) = &mut body.kind else {
            panic!()
        };
        let CallArg::Value(value) = &args[0] else {
            panic!()
        };
        args[0] = CallArg::ReadTemporary(value.clone());
        rejected_by_all_consumers(&changed);
    }

    #[test]
    fn frozen_correspondence_checks_unselected_no_storage_continuations() {
        let source = "fn forever() { forever() } fn main() { if false { let unused = forever(); 41 } else { 42 } }";
        let (_, mut changed, _) = lower(source);
        let typed::Computation::Value(body) = &mut changed.functions[changed.entry].body else {
            panic!()
        };
        let typed::ExprKind::If(_, yes, _) = &mut body.kind else {
            panic!()
        };
        let typed::ExprKind::NoStorageLet { body, .. } = &mut yes.kind else {
            panic!()
        };
        body.kind = typed::ExprKind::Int(99);
        rejected_by_all_consumers(&changed);
    }
    #[test]
    fn missing_proof_is_rejected_even_when_only_an_unselected_tag_remains() {
        use typed::{Computation, ExprKind, ExprNode, ResultRepr, Type};
        let (_, mut changed, _) = lower("fn forever() { forever() } fn main() { if false { let unused = forever(); 41 } else { 42 } }");
        changed.logical.control = None;
        changed.functions[0].result = ResultRepr::Value(Type::Int);
        changed.functions[0].body = Computation::Value(ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(0),
        });
        assert!(typed::verify(&changed)
            .unwrap_err()
            .contains("canonical source proof"));
        rejected_by_all_consumers(&changed);
    }
    #[test]
    fn separate_opaque_results_with_the_same_void_abi_are_not_interchangeable() {
        let (_, mut changed, _) = lower("fn forever(value) { forever(value) } fn main() { if false { let unused = forever(true); 1 } else { let unused = forever(0); 2 } }");
        let typed::ResultRepr::NoNormal(first) = &changed.functions[0].result else {
            panic!()
        };
        let typed::ResultRepr::NoNormal(second) = &changed.functions[1].result else {
            panic!()
        };
        assert_ne!(first, second);
        changed.functions[0].result = changed.functions[1].result.clone();
        rejected_by_all_consumers(&changed);
    }
}
