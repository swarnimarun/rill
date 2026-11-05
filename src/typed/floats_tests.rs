use super::*;
use crate::interpreter::Value;

fn lower(source: &str) -> TypedMain {
    let program = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&program).unwrap()).unwrap()
}

fn rejects(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(evaluate(program).is_err());
    assert!(emit_c(program).is_err());
}

fn body(program: &mut TypedMain) -> &mut ExprNode {
    program.functions[program.entry]
        .body
        .value_expr_mut()
        .unwrap()
}

#[test]
fn independent_verifier_checks_float_arity_types_results_and_nested_children() {
    let base = lower("fn main() { f32_add(f32(40), f32(2)) }");
    for mutation in 0..5 {
        let mut bad = base.clone();
        let node = body(&mut bad);
        let ExprKind::Float(op, args) = &mut node.kind else {
            panic!()
        };
        match mutation {
            0 => {
                args.pop();
            }
            1 => {
                args.push(args[0].clone());
            }
            2 => {
                args[0] = ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Int(40),
                };
            }
            3 => {
                *op = FloatOp::Lt;
            }
            _ => {
                let ExprKind::ForeignConvert(_, input) = &mut args[0].kind else {
                    panic!()
                };
                input.kind = ExprKind::Read(LocalId(u32::MAX));
            }
        }
        rejects(&bad);
    }
    let mut bad = lower("fn main() { f32_is_finite(f32(1)) }");
    let ExprKind::Float(op, _) = &mut body(&mut bad).kind else {
        panic!()
    };
    *op = FloatOp::Neg;
    rejects(&bad);
}

#[test]
fn ieee_equality_is_numeric_including_captured_float_values() {
    assert_eq!(Value::F32(-0.0), Value::F32(0.0));
    assert_ne!(Value::F32(f32::NAN), Value::F32(f32::NAN));
    let ast = crate::parser::parse("fn main() { let x = f32_div(f32(1), f32(10)); let f = fn(y) { f32_add(x, y) }; f(f32(1)) }").unwrap();
    assert_eq!(
        crate::interpreter::run_reference(&ast).unwrap().to_string(),
        "1.10000002"
    );
    let converted = crate::functions::lower(&ast).unwrap();
    let program = lower_main(&crate::check::analyze(&converted).unwrap()).unwrap();
    assert_eq!(evaluate(&program).unwrap().to_string(), "1.10000002");
}
