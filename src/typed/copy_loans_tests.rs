use super::*;
fn checked(source: &str) -> TypedMain {
    let p = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&p).unwrap()).unwrap()
}
fn arguments(expr: &mut ExprNode) -> &mut Vec<CallArg> {
    match &mut expr.kind {
        ExprKind::Let { body, .. } => arguments(body),
        ExprKind::Call(_, args) => args,
        _ => panic!("expected let-chain tail call"),
    }
}
fn root(program: &TypedMain) -> LocalId {
    let ExprKind::Let { local, .. } = &program.functions[program.entry]
        .body
        .value_expr()
        .unwrap()
        .kind
    else {
        panic!()
    };
    *local
}
fn int(n: i32) -> ExprNode {
    ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(n),
    }
}
fn write_then(id: LocalId, body: ExprNode) -> ExprNode {
    ExprNode {
        ty: body.ty,
        kind: ExprKind::Let {
            local: LocalId(u32::MAX),
            mutable: false,
            value: Box::new(ExprNode {
                ty: Type::Unit,
                kind: ExprKind::Assign(id, Box::new(int(9))),
            }),
            body: Box::new(body),
        },
    }
}
fn rejects(p: &TypedMain, message: &str) {
    assert!(verify(p).unwrap_err().contains(message));
    assert!(evaluate(p).is_err());
    assert!(emit_c(p).is_err());
}
#[test]
fn independent_copy_loan_verifier_checks_identity_modes_and_types() {
    let original =
        checked("fn sum(read a, read b) { a + b }\nfn main() { let mut x = 21; sum(x, x) }");
    for case in 0..8 {
        let mut bad = original.clone();
        let id = root(&bad);
        let entry = bad.entry;
        match case {
            0 => {
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[0] =
                    CallArg::ReadCopy(LocalId(u32::MAX))
            }
            1 => {
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] =
                    CallArg::ReadTemporary(ExprNode {
                        ty: Type::Bool,
                        kind: ExprKind::Bool(true),
                    })
            }
            2 => {
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] =
                    CallArg::InoutCopy(id)
            }
            3 => bad.functions[0].params[0].2 = ParamMode::Value,
            4 => {
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[0] =
                    CallArg::Value(int(21))
            }
            5 => {
                let ExprKind::Let { value, .. } =
                    &mut bad.functions[entry].body.value_expr_mut().unwrap().kind
                else {
                    panic!()
                };
                **value = ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Bool(true),
                };
            }
            6 => {
                bad.functions[0].params[1].2 = ParamMode::Inout;
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] =
                    CallArg::InoutCopy(id);
            }
            _ => {
                bad.functions[0].params[0].2 = ParamMode::Inout;
                arguments(bad.functions[entry].body.value_expr_mut().unwrap())[0] =
                    CallArg::InoutCopy(id);
            }
        }
        assert!(verify(&bad).is_err(), "case {case}");
        assert!(evaluate(&bad).is_err());
        assert!(emit_c(&bad).is_err());
    }
}
#[test]
fn copy_storage_aliases_only_explicit_loans() {
    use crate::interpreter::Value;
    let mut locals = CopyLocals::default();
    locals.insert(LocalId(0), Value::Bytes(vec![1, 2]));
    locals.bind_loan(LocalId(1), locals.loan(&LocalId(0)).unwrap());
    locals.insert(LocalId(2), locals.read(&LocalId(0)).unwrap());
    locals.write(&LocalId(1), Value::Bytes(vec![3, 4])).unwrap();
    assert_eq!(locals.read(&LocalId(0)).unwrap(), Value::Bytes(vec![3, 4]));
    assert_eq!(locals.read(&LocalId(2)).unwrap(), Value::Bytes(vec![1, 2]));
    locals.remove(&LocalId(1));
    locals.remove(&LocalId(0));
    assert_eq!(locals.read(&LocalId(2)).unwrap(), Value::Bytes(vec![1, 2]));
    assert!(locals.loan(&LocalId(0)).is_err());
}
#[test]
fn later_reads_writes_and_exclusive_aliases_are_rejected() {
    let original =
        checked("fn set(inout a, b) { a <- b }\nfn main() { let mut x = 1; set(inout x, 2) }");
    let mut bad = original.clone();
    let id = root(&bad);
    let entry = bad.entry;
    arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] = CallArg::Value(ExprNode {
        ty: Type::Int,
        kind: ExprKind::Read(id),
    });
    rejects(&bad, "invalid ordinary read");
    let mut bad = original;
    let entry = bad.entry;
    let ExprKind::Let { mutable, .. } =
        &mut bad.functions[entry].body.value_expr_mut().unwrap().kind
    else {
        panic!()
    };
    *mutable = false;
    rejects(&bad, "conflicting Copy loan");
    let mut bad=checked("fn both(inout a, inout b) { a <- b }\nfn main() { let mut x = 1; let mut y = 2; both(inout x, inout y) }");
    let id = root(&bad);
    let entry = bad.entry;
    arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] = CallArg::InoutCopy(id);
    rejects(&bad, "invalid Copy root loan");
    let mut bad = checked("fn add(read a, b) { a + b }\nfn main() { let mut x = 1; add(x, 2) }");
    let id = root(&bad);
    let entry = bad.entry;
    arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1] =
        CallArg::Value(write_then(id, int(2)));
    rejects(&bad, "invalid assignment");
}
#[test]
fn nested_call_release_does_not_erase_enclosing_loan() {
    let mut bad=checked("fn get(read a) { a + 0 }\nfn add(read a, b) { a + b }\nfn main() { let mut x = 1; add(x, { let z = get(x); z }) }");
    let id = root(&bad);
    let entry = bad.entry;
    let CallArg::Value(value) =
        &mut arguments(bad.functions[entry].body.value_expr_mut().unwrap())[1]
    else {
        panic!()
    };
    let ExprKind::Let { body, .. } = &mut value.kind else {
        panic!()
    };
    **body = write_then(id, (**body).clone());
    rejects(&bad, "invalid assignment");
}
#[test]
fn shared_parameter_cannot_be_written_or_exclusively_reborrowed() {
    let mut bad = checked("fn observe(read x) { x + 0 }\nfn main() { observe(42) }");
    let id = bad.functions[0].params[0].0;
    bad.functions[0].body = (write_then(id, int(42))).into();
    rejects(&bad, "invalid assignment");
    let mut bad=checked("fn bump(inout x) { x <- x + 1 }\nfn forward(inout x) { bump(inout x) }\nfn main() { let mut n = 1; forward(inout n) }");
    bad.functions[1].params[0].2 = ParamMode::Read;
    rejects(&bad, "conflicting Copy loan");
}
#[test]
fn read_temporary_visits_owner_effects_and_hidden_calls() {
    let mut program=checked("fn observe(read x: i32) { x }\nfn main() { observe({ let owner = system_box_i32_abort(42); box_read(owner) }) }");
    let mut state = EvalState::default();
    assert!(matches!(
        evaluate_function(&program, program.entry, vec![], 1, &mut state),
        Ok(EvalArg::Value(crate::interpreter::Value::Int(42)))
    ));
    assert_eq!((state.allocations, state.frees), (1, 1));
    assert!(emit_c(&program).unwrap().contains("rl_box_i32_abort"));
    let entry = program.entry;
    arguments(program.functions[entry].body.value_expr_mut().unwrap())[0] =
        CallArg::ReadTemporary(ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(entry, vec![]),
        });
    let body = program.functions[entry].body.value_expr().unwrap().clone();
    program.functions[entry].body = (ExprNode {
        ty: Type::Int,
        kind: ExprKind::OwnerLet {
            local: LocalId(u32::MAX),
            mutable: false,
            init: OwnerInit::Alloc(Box::new(int(1))),
            body: Box::new(body),
            cleanup: Some(Cleanup {
                local: LocalId(u32::MAX),
                tree: CleanupTree::DropWholeOwner,
            }),
        },
    })
    .into();
    rejects(&program, "recursive");
}
#[test]
fn allocation_metadata_and_failure_are_rechecked_inside_read_temporary() {
    let mut source=crate::parser::parse("type Result<a,b> = Ok(a) | Err(b)\ntype AllocError = InvalidLength | SizeOverflow | OutOfMemory\nfn observe(read x: i32) { x }\nfn main() { observe({ let result = system_box_i32(42); match move result { Ok(owner) => box_read(owner) | Err(_) => 0 } }) }").unwrap();
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
        let count = usize::from(fail_at_attempt.is_none());
        assert_eq!((state.allocations, state.frees), (count, count));
    }
    let entry = program.entry;
    let CallArg::ReadTemporary(value) =
        &mut arguments(program.functions[entry].body.value_expr_mut().unwrap())[0]
    else {
        panic!()
    };
    let ExprKind::OwnerLet {
        init: OwnerInit::Expr(value),
        ..
    } = &mut value.kind
    else {
        panic!()
    };
    let ExprKind::TryAlloc { layout, .. } = &mut value.kind else {
        panic!()
    };
    layout.ok_variant = usize::MAX;
    assert!(verify(&program).is_err());
    assert!(evaluate(&program).is_err());
    assert!(emit_c(&program).is_err());
}
