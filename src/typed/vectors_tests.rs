use super::*;
fn lower(source: &str) -> TypedMain {
    let p = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&p).unwrap()).unwrap()
}
fn rejects(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(evaluate(program).is_err());
    assert!(emit_c(program).is_err());
}
fn body(program: &mut TypedMain) -> &mut ExprNode {
    &mut program.functions[program.entry].body
}
fn int(n: i32) -> ExprNode {
    ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(n),
    }
}
fn vector() -> ExprNode {
    ExprNode {
        ty: Type::U8x16,
        kind: ExprKind::Vector(
            VectorOp::Load,
            vec![
                ExprNode {
                    ty: Type::Bytes(16),
                    kind: ExprKind::BytesNew(16),
                },
                int(0),
            ],
        ),
    }
}

#[test]
fn vector_operations_have_closed_independent_signatures() {
    let cases = [
        "u8x16_load(bytes_new(16), 0)",
        "u8x16_lane(u8x16_load(bytes_new(16), 0), 0)",
        "u8x16_and(u8x16_load(bytes_new(16), 0), u8x16_load(bytes_new(16), 0))",
        "u8x16_or(u8x16_load(bytes_new(16), 0), u8x16_load(bytes_new(16), 0))",
        "u8x16_xor(u8x16_load(bytes_new(16), 0), u8x16_load(bytes_new(16), 0))",
        "u8x16_eq(u8x16_load(bytes_new(16), 0), u8x16_load(bytes_new(16), 0))",
        "u8x16_any(u8x16_load(bytes_new(16), 0))",
        "u8x16_all(u8x16_load(bytes_new(16), 0))",
    ];
    for source in cases {
        let original = lower(&format!("fn main() {{ {source} }}"));
        for case in 0..5 {
            let mut bad = original.clone();
            let node = body(&mut bad);
            let ExprKind::Vector(_, args) = &mut node.kind else {
                panic!()
            };
            match case {
                0 => args.clear(),
                1 => args.push(vector()),
                2 => {
                    args[0] = ExprNode {
                        ty: Type::Bool,
                        kind: ExprKind::Bool(true),
                    }
                }
                3 => {
                    *args.last_mut().unwrap() = ExprNode {
                        ty: Type::Unit,
                        kind: ExprKind::Unit,
                    }
                }
                _ => node.ty = Type::Unit,
            }
            rejects(&bad);
        }
    }
    let mut bad = lower("fn main() { u8x16_load(bytes_new(16), 0) }");
    let ExprKind::Vector(_, args) = &mut body(&mut bad).kind else {
        panic!()
    };
    args[0] = ExprNode {
        ty: Type::Bytes(257),
        kind: ExprKind::BytesNew(257),
    };
    rejects(&bad);
    // Capacity below sixteen is a valid typed operation that traps at runtime.
    let p = lower("fn main() { u8x16_load(bytes_new(0), 0) }");
    verify(&p).unwrap();
    assert_eq!(evaluate(&p).unwrap_err(), "runtime trap");
    emit_c(&p).unwrap();
}

#[test]
fn vector_results_do_not_enable_scalar_patterns_or_operations() {
    let mut bad = lower("fn main() { match u8x16_load(bytes_new(16), 0) { _ => 42 } }");
    let ExprKind::CopyMatch { arms, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    arms[0].pattern = CopyPattern::Int(0);
    rejects(&bad);
    let mut bad = lower("fn main() { 20 + 22 }");
    let ExprKind::Binary(_, a, _) = &mut body(&mut bad).kind else {
        panic!()
    };
    **a = vector();
    rejects(&bad);
    let mut bad =
        lower("type R = { v: u8x16 }\nfn main() { record R { v = u8x16_load(bytes_new(16), 0) } }");
    let ExprKind::Record { fields, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    fields[0].1 = int(0);
    rejects(&bad);
}

#[test]
fn nested_vector_operands_keep_active_field_loan_checks() {
    let mut bad=lower("type R = { x: i32; v: u8x16 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut r = record R { x = 0; v = u8x16_load(bytes_new(16), 0) }; set(inout r.x, u8x16_lane(r.v, 0)) }");
    let ExprKind::Let { body, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::InoutField(place) = &args[0] else {
        panic!()
    };
    let place = place.clone();
    let CallArg::Value(value) = &mut args[1] else {
        panic!()
    };
    let ExprKind::Vector(_, operands) = &mut value.kind else {
        panic!()
    };
    operands[1] = ExprNode {
        ty: Type::Int,
        kind: ExprKind::FieldRead(place),
    };
    assert!(verify(&bad).unwrap_err().contains("conflicting"));
    rejects(&bad);
}

#[test]
fn vector_operands_visit_owner_effects_cleanup_and_calls() {
    let program=lower("fn main() { let v = u8x16_xor(u8x16_load({ let a = system_box_i32_abort(40); bytes_set(bytes_new(16), 0, box_read(a)) }, 0), u8x16_load({ let b = system_box_i32_abort(2); bytes_set(bytes_new(16), 0, box_read(b)) }, 0)); u8x16_lane(v, 0) }");
    for fail_after in [None, Some(0), Some(1)] {
        let mut state = EvalState {
            fail_after,
            ..EvalState::default()
        };
        let result = evaluate_function(&program, program.entry, vec![], 1, &mut state);
        match fail_after {
            None => {
                assert!(matches!(
                    result,
                    Ok(EvalArg::Value(crate::interpreter::Value::Int(42)))
                ));
                assert_eq!(state.released, [40, 2]);
            }
            Some(n) => {
                assert_eq!(result.unwrap_err(), "runtime trap");
                assert_eq!((state.allocations, state.frees), (n, n));
            }
        }
    }
    assert!(emit_c(&program)
        .unwrap()
        .contains("static int32_t *rl_box_i32_abort"));
    let mut bad=lower("fn main() { let owner = system_box_i32_abort(42); u8x16_lane(u8x16_load(bytes_new(16), 0), 0) }");
    let entry = bad.entry;
    let ExprKind::OwnerLet { body, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    let ExprKind::Vector(_, args) = &mut body.kind else {
        panic!()
    };
    args[1] = ExprNode {
        ty: Type::Int,
        kind: ExprKind::Call(entry, vec![]),
    };
    assert!(verify(&bad).unwrap_err().contains("recursive"));
    rejects(&bad);
}

#[test]
fn fallible_allocation_metadata_inside_vector_operands_is_rechecked() {
    let mut source=crate::parser::parse("type Result<a,b> = Ok(a) | Err(b)\ntype AllocError = InvalidLength | SizeOverflow | OutOfMemory\nfn main() { u8x16_lane(u8x16_load({ let result = system_box_i32(42); match move result { Ok(owner) => bytes_set(bytes_new(16), 0, box_read(owner)) | Err(_) => bytes_new(16) } }, 0), 0) }").unwrap();
    source.allocation_types = Some(crate::ast::AllocationTypes {
        result_type: "Result".into(),
        ok_constructor: "Ok".into(),
        err_constructor: "Err".into(),
        error_type: "AllocError".into(),
        invalid_length_constructor: "InvalidLength".into(),
        size_overflow_constructor: "SizeOverflow".into(),
        out_of_memory_constructor: "OutOfMemory".into(),
    });
    let mut program = lower_main(&crate::check::analyze(&source).unwrap()).unwrap();
    for fail_at_attempt in [None, Some(1)] {
        let mut state = EvalState {
            fail_at_attempt,
            ..EvalState::default()
        };
        let value = evaluate_function(&program, program.entry, vec![], 1, &mut state).unwrap();
        let expected = if fail_at_attempt.is_none() { 42 } else { 0 };
        assert!(matches!(value,EvalArg::Value(crate::interpreter::Value::Int(n)) if n==expected));
        let n = usize::from(fail_at_attempt.is_none());
        assert_eq!((state.allocations, state.frees), (n, n));
    }
    let ExprKind::Vector(_, args) = &mut body(&mut program).kind else {
        panic!()
    };
    let ExprKind::Vector(_, args) = &mut args[0].kind else {
        panic!()
    };
    let ExprKind::OwnerLet {
        init: OwnerInit::Expr(value),
        ..
    } = &mut args[0].kind
    else {
        panic!()
    };
    let ExprKind::TryAlloc { layout, .. } = &mut value.kind else {
        panic!()
    };
    layout.ok_variant = usize::MAX;
    rejects(&program);
}
