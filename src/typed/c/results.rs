//! Layoutless control results use an internal void ABI and no result slot.
use super::*;
use crate::typed::{NoNormalExpr, NoNormalKind};

impl Emitter<'_> {
    pub(super) fn interrupted(&mut self, call: &crate::typed::InterruptedCall) -> EmitOutcome {
        for value in &call.prefix {
            if matches!(self.expr(value), EmitOutcome::NoNormal) {
                return EmitOutcome::NoNormal;
            }
        }
        match &*call.barrier {
            crate::typed::Computation::Value(value) => {
                if let EmitOutcome::Value(_) = self.expr(value) {
                    self.out.push_str("rl_trap();\n");
                }
            }
            crate::typed::Computation::NoNormal(value) => {
                self.no_normal(value);
            }
        }
        EmitOutcome::NoNormal
    }

    pub(super) fn no_normal(&mut self, mut expression: &NoNormalExpr) -> EmitOutcome {
        loop {
            match &expression.kind {
                NoNormalKind::InterruptedCall(call) => return self.interrupted(call),
                NoNormalKind::Call(callee, args) => {
                    if let Some(values) = self.call_arguments(*callee, args) {
                        writeln!(
                            self.out,
                            "rl_fn_{callee}({});\nrl_trap();",
                            values.join(", ")
                        )
                        .unwrap();
                    }
                    return EmitOutcome::NoNormal;
                }
                NoNormalKind::Let {
                    local, value, body, ..
                } => {
                    let value_type = value.ty;
                    let value = emit_value!(self.expr(value));
                    self.add_cost(value_type);
                    writeln!(
                        self.out,
                        "{} rl_l_{} = {value};",
                        c_type(value_type),
                        local.0
                    )
                    .unwrap();
                    expression = body;
                }
                NoNormalKind::NoStorageLet { value, .. } => expression = value,
                NoNormalKind::If { condition, yes, no } => {
                    let condition = emit_value!(self.expr(condition));
                    writeln!(self.out, "if ({condition}) {{").unwrap();
                    self.no_normal(yes);
                    self.out.push_str("} else {\n");
                    self.no_normal(no);
                    self.out.push_str("}\n");
                    return EmitOutcome::NoNormal;
                }
                NoNormalKind::Match { value, arms } => {
                    let value_type = value.ty;
                    let value = emit_value!(self.expr(value));
                    for (index, arm) in arms.iter().enumerate() {
                        if index != 0 {
                            self.out.push_str("else ");
                        }
                        let condition = copy_match::condition(&arm.pattern, &value);
                        writeln!(self.out, "if ({condition}) {{").unwrap();
                        self.bind_pattern(&arm.pattern, value_type, &value);
                        self.no_normal(&arm.body);
                        self.out.push_str("}\n");
                    }
                    self.out.push_str("else { rl_trap(); }\n");
                    return EmitOutcome::NoNormal;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::{Computation, NoStorageBinding, ResultRepr};

    fn fixture() -> TypedMain {
        let source = crate::parser::parse("type Flag<a> = Off | On fn main() { Off }").unwrap();
        crate::typed::lower_main(&crate::check::analyze(&source).unwrap()).unwrap()
    }
    #[test]
    fn bare_no_normal_c_abi_has_no_result_slot_or_printer() {
        let mut program = fixture();
        let identity = program.logical.opaque_id(0).unwrap();
        program.functions[program.entry].result = ResultRepr::NoNormal(identity.clone());
        program.functions[program.entry].body = Computation::NoNormal(NoNormalExpr {
            occurrence: 0,
            result: identity,
            kind: NoNormalKind::Call(program.entry, vec![]),
        });
        // Deliberately test only emission; public emit_c still rejects this
        // unproved fixture. It is not evidence of source/IR correspondence.
        assert!(emit_c(&program).is_err());
        let source = emit_verified(&program).unwrap();
        assert!(source.contains("static void rl_fn_0(void)"));
        assert!(source.contains("rl_fn_0();\nrl_trap();"));
        let main = source.split("int main(void)").nth(1).unwrap();
        assert_eq!(main.trim(), "{ rl_fn_0(); rl_trap(); }");
        let function = source.split("static void rl_fn_0(void) {").nth(1).unwrap();
        assert!(!function.contains("rl_t_"));
    }
    #[test]
    fn concrete_no_storage_sequence_keeps_its_abi_but_allocates_no_sequence_slot() {
        let mut program = fixture();
        let identity = program.logical.opaque_id(0).unwrap();
        program.functions[program.entry].result = ResultRepr::Value(Type::Int);
        program.functions[program.entry].body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::NoStorageLet {
                binding: NoStorageBinding {
                    domain: identity.domain.clone(),
                    occurrence: 0,
                },
                value: Box::new(NoNormalExpr {
                    occurrence: 0,
                    result: identity,
                    kind: NoNormalKind::Call(program.entry, vec![]),
                }),
                body: Box::new(ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Int(42),
                }),
            },
        }
        .into();
        assert!(emit_c(&program).is_err());
        let source = emit_verified(&program).unwrap();
        assert!(source.contains("static int32_t rl_fn_0(void)"));
        let function = source
            .split("static int32_t rl_fn_0(void) {")
            .nth(1)
            .unwrap()
            .split("int main(void)")
            .next()
            .unwrap();
        assert!(!function.contains("rl_t_"));
        assert!(!function.contains("INT32_C(42)"));
        assert!(function.contains("rl_fn_0();\nrl_trap();"));
    }
}
