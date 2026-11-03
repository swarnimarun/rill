use super::*;

fn lower(source: &str) -> TypedMain {
    let p = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&p).unwrap()).unwrap()
}
fn rejects(p: &TypedMain) {
    assert!(verify(p).is_err());
    assert!(evaluate(p).is_err());
    assert!(emit_c(p).is_err());
}
fn tail(expr: &mut ExprNode) -> &mut ExprNode {
    if matches!(expr.kind, ExprKind::Let { .. }) {
        let ExprKind::Let { body, .. } = &mut expr.kind else {
            unreachable!()
        };
        tail(body)
    } else {
        expr
    }
}
fn arguments(p: &mut TypedMain) -> &mut Vec<CallArg> {
    let ExprKind::Call(_, args) = &mut tail(&mut p.functions[p.entry].body).kind else {
        panic!()
    };
    args
}
fn place(arg: &mut CallArg) -> &mut FieldPlace {
    match arg {
        CallArg::ReadField(p) | CallArg::InoutField(p) => p,
        _ => panic!(),
    }
}
fn int(n: i32) -> ExprNode {
    ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(n),
    }
}
fn then(value: ExprNode, body: ExprNode) -> ExprNode {
    ExprNode {
        ty: body.ty,
        kind: ExprKind::Let {
            local: LocalId(u32::MAX),
            mutable: false,
            value: Box::new(value),
            body: Box::new(body),
        },
    }
}
fn write(place: FieldPlace, body: ExprNode) -> ExprNode {
    then(
        ExprNode {
            ty: Type::Unit,
            kind: ExprKind::FieldAssign(place, Box::new(int(9))),
        },
        body,
    )
}

#[test]
fn field_operands_recheck_paths_modes_identity_and_authority() {
    let original = lower("type P = { x: i32; y: i32; flag: bool }\nfn set(inout x, read y) { x <- y }\nfn main() { let mut p = record P { x = 1; y = 42; flag = true }; set(inout p.x, p.y) }");
    for case in 0..11 {
        let mut bad = original.clone();
        match case {
            0 => place(&mut arguments(&mut bad)[0]).root = LocalId(u32::MAX),
            1 => place(&mut arguments(&mut bad)[0]).path.clear(),
            2 => {
                let p = place(&mut arguments(&mut bad)[0]);
                p.path = vec![p.path[0].clone(); 17];
            }
            3 => place(&mut arguments(&mut bad)[0]).path[0].record = usize::MAX,
            4 => place(&mut arguments(&mut bad)[0]).path[0].field = usize::MAX,
            5 => {
                let p = place(&mut arguments(&mut bad)[0]).clone();
                arguments(&mut bad)[0] = CallArg::ReadField(p);
            }
            6 => {
                let p = place(&mut arguments(&mut bad)[1]).clone();
                arguments(&mut bad)[1] = CallArg::InoutField(p);
            }
            7 => {
                let ExprKind::Let { mutable, .. } = &mut bad.functions[bad.entry].body.kind else {
                    panic!()
                };
                *mutable = false;
            }
            8 => bad.functions[0].params[0].1 = Type::Bool,
            9 => place(&mut arguments(&mut bad)[0]).path[0].field = 2,
            _ => bad.adts[0].record_fields = None,
        }
        rejects(&bad);
    }
    let mut bad=lower("type P = { x: i32 }\nfn get(read x: i32) { x }\nfn main() { let owner = system_box_i32_abort(42); let p = record P { x = 0 }; get(p.x) }");
    let ExprKind::OwnerLet { local, body, .. } = &mut bad.functions[bad.entry].body.kind else {
        panic!()
    };
    let owner = *local;
    let ExprKind::Call(_, args) = &mut tail(body).kind else {
        panic!()
    };
    place(&mut args[0]).root = owner;
    rejects(&bad);
}

#[test]
fn prefix_overlap_and_root_intervals_are_independent_of_lowering() {
    for source in [
        "type P = { x: i32; y: i32 }\nfn f(inout a, read b) { a <- b }\nfn main() { let mut p = record P { x = 1; y = 42 }; f(inout p.x, p.y) }",
        "type P = { x: i32; y: i32 }\nfn f(read a, inout b) { b <- a }\nfn main() { let mut p = record P { x = 1; y = 42 }; f(p.x, inout p.y) }",
        "type P = { x: i32; y: i32 }\nfn f(inout a, inout b) { a <- b }\nfn main() { let mut p = record P { x = 1; y = 42 }; f(inout p.x, inout p.y) }",
        "type I = { x: i32 }\ntype O = { a: I; b: I }\nfn f(inout a: I, read b) { a.x <- b }\nfn main() { let mut p = record O { a = record I { x = 1 }; b = record I { x = 42 } }; f(inout p.a, p.b.x) }",
        "type I = { x: i32 }\ntype O = { a: I; b: I }\nfn f(read a, inout b: I) { b.x <- a }\nfn main() { let mut p = record O { a = record I { x = 1 }; b = record I { x = 42 } }; f(p.a.x, inout p.b) }",
    ] {
        let mut bad=lower(source);
        place(&mut arguments(&mut bad)[1]).path[0].field=0;
        rejects(&bad);
    }
    for (mode, first, second) in [("read", "p", "inout q.x"), ("inout", "inout p", "q.x")] {
        let other = if mode == "read" { "inout" } else { "read" };
        let mut bad=lower(&format!("type P = {{ x: i32 }}\nfn f({mode} a: P, {other} b: i32) {{ () }}\nfn main() {{ let mut p = record P {{ x = 1 }}; let mut q = record P {{ x = 2 }}; f({first}, {second}) }}"));
        let args = arguments(&mut bad);
        let id = match args[0] {
            CallArg::ReadCopy(id) | CallArg::InoutCopy(id) => id,
            _ => panic!(),
        };
        place(&mut args[1]).root = id;
        rejects(&bad);
    }
    // A later whole-root loan must also see an earlier projected interval.
    for (mode, first, second) in [("read", "inout p.x", "q"), ("inout", "p.x", "inout q")] {
        let other = if mode == "read" { "inout" } else { "read" };
        let mut bad=lower(&format!("type P = {{ x: i32 }}\nfn f({other} a: i32, {mode} b: P) {{ () }}\nfn main() {{ let mut p = record P {{ x = 1 }}; let mut q = record P {{ x = 2 }}; f({first}, {second}) }}"));
        let id = place(&mut arguments(&mut bad)[0]).root;
        arguments(&mut bad)[1] = if mode == "read" {
            CallArg::ReadCopy(id)
        } else {
            CallArg::InoutCopy(id)
        };
        rejects(&bad);
    }
}

#[test]
fn later_argument_accesses_and_nested_release_keep_enclosing_intervals() {
    let original=lower("type P = { x: i32; y: i32 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut p = record P { x = 1; y = 2 }; set(inout p.x, 42) }");
    for case in 0..4 {
        let mut bad = original.clone();
        let p = place(&mut arguments(&mut bad)[0]).clone();
        let root_ty = Type::Adt {
            id: p.path[0].record,
            owned: false,
        };
        let expr = match case {
            0 => ExprNode {
                ty: Type::Int,
                kind: ExprKind::FieldRead(p),
            },
            1 => write(p, int(42)),
            2 => then(
                ExprNode {
                    ty: root_ty,
                    kind: ExprKind::Read(p.root),
                },
                int(42),
            ),
            _ => then(
                ExprNode {
                    ty: Type::Unit,
                    kind: ExprKind::Assign(
                        p.root,
                        Box::new(ExprNode {
                            ty: root_ty,
                            kind: ExprKind::Record {
                                adt: p.path[0].record,
                                fields: vec![(0, int(1)), (1, int(2))],
                            },
                        }),
                    ),
                },
                int(42),
            ),
        };
        arguments(&mut bad)[1] = CallArg::Value(expr);
        rejects(&bad);
    }
    for inner in ["get(p.x)", "get(p.y)", "root(p)"] {
        let mut bad=lower(&format!("type P = {{ x: i32; y: i32 }}\nfn get(read n) {{ n + 0 }}\nfn root(read p: P) {{ p.x }}\nfn sum(read n, value) {{ n + value }}\nfn main() {{ let mut p = record P {{ x = 20; y = 22 }}; sum(p.x, {{ let n = {inner}; n }}) }}"));
        let p = place(&mut arguments(&mut bad)[0]).clone();
        let CallArg::Value(value) = &mut arguments(&mut bad)[1] else {
            panic!()
        };
        let ExprKind::Let { body, .. } = &mut value.kind else {
            panic!()
        };
        **body = write(p, (**body).clone());
        rejects(&bad);
    }
}

#[test]
fn read_record_parameters_cannot_write_or_exclusively_forward_fields() {
    let mut bad=lower("type P = { x: i32 }\nfn get(read p: P) { p.x }\nfn main() { let p = record P { x = 42 }; get(p) }");
    let ExprKind::FieldRead(p) = &bad.functions[0].body.kind else {
        panic!()
    };
    bad.functions[0].body = write(p.clone(), int(42));
    rejects(&bad);
    let mut bad=lower("type P = { x: i32 }\nfn bump(inout x) { x <- x + 1 }\nfn forward(inout p: P) { bump(inout p.x) }\nfn main() { let mut p = record P { x = 41 }; forward(inout p) }");
    bad.functions[1].params[0].2 = ParamMode::Read;
    let CallArg::InoutCopy(id) = arguments(&mut bad)[0] else {
        panic!()
    };
    arguments(&mut bad)[0] = CallArg::ReadCopy(id);
    rejects(&bad);
}

#[test]
fn projected_runtime_handles_preserve_original_storage_and_copy_snapshots() {
    use crate::interpreter::Value;
    let mut locals = CopyLocals::default();
    let inner = Value::Record(
        "I".into(),
        vec![("x".into(), Value::Int(1)), ("y".into(), Value::Int(2))],
    );
    locals.insert(
        LocalId(0),
        Value::Record(
            "O".into(),
            vec![("inner".into(), inner), ("other".into(), Value::Int(40))],
        ),
    );
    let inner_place = FieldPlace {
        root: LocalId(0),
        path: vec![FieldStep {
            record: 0,
            field: 0,
        }],
    };
    locals.bind_loan(LocalId(1), locals.loan_field(&inner_place).unwrap());
    let x_place = FieldPlace {
        root: LocalId(1),
        path: vec![FieldStep {
            record: 1,
            field: 0,
        }],
    };
    locals.bind_loan(LocalId(2), locals.loan_field(&x_place).unwrap());
    locals.insert(LocalId(3), locals.read(&LocalId(1)).unwrap());
    locals.write(&LocalId(2), Value::Int(42)).unwrap();
    assert_eq!(locals.read_field(&x_place).unwrap(), Value::Int(42));
    assert_eq!(
        locals
            .read_field(&FieldPlace {
                root: LocalId(3),
                path: x_place.path.clone()
            })
            .unwrap(),
        Value::Int(1)
    );
    // Replacing the original prefix does not detach a projected handle.
    locals
        .write(
            &LocalId(1),
            Value::Record(
                "I".into(),
                vec![("x".into(), Value::Int(20)), ("y".into(), Value::Int(22))],
            ),
        )
        .unwrap();
    assert_eq!(locals.read(&LocalId(2)).unwrap(), Value::Int(20));
    assert_eq!(
        locals
            .read_field(&FieldPlace {
                root: LocalId(0),
                path: vec![FieldStep {
                    record: 0,
                    field: 1
                }]
            })
            .unwrap(),
        Value::Int(40)
    );
    locals.remove(&LocalId(2));
    locals.remove(&LocalId(1));
    assert_eq!(locals.strong_count(LocalId(0)), 1);
}

#[test]
fn field_call_handles_release_on_success_argument_failure_and_depth_trap() {
    use crate::interpreter::Value;
    for (source,expected) in [
        ("type P = { x: i32 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut p = record P { x = 0 }; set(inout p.x, 42) }",true),
        ("type P = { x: i32 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut p = record P { x = 0 }; set(inout p.x, 1 / 0) }",false),
        ("type P = { x: i32 }\nfn set(inout x, n) { x <- n / 0 }\nfn main() { let mut p = record P { x = 0 }; set(inout p.x, 42) }",false),
    ] {
        let mut program=lower(source);
        let ExprKind::Let {local,..}=&program.functions[program.entry].body.kind else {panic!()};
        let root=*local;
        let call=tail(&mut program.functions[program.entry].body).clone();
        for depth in [1,32] {
            let mut locals=CopyLocals::default();
            locals.insert(root,Value::Record("P".into(),vec![("x".into(),Value::Int(0))]));
            let result=evaluate_expr(&call,&mut locals,&program,depth,&mut EvalState::default(),&mut HashMap::new());
            assert_eq!(result.is_ok(),expected&&depth==1);
            assert_eq!(locals.strong_count(root),1);
            locals.write(&root,Value::Record("P".into(),vec![("x".into(),Value::Int(9))])).unwrap();
        }
    }
}

#[test]
fn owner_effects_in_later_arguments_preserve_field_loans_and_cleanup() {
    let program=lower("type P = { x: i32; y: i32 }\nfn set(inout x: i32, n: i32) { x <- n }\nfn main() { let outer = system_box_i32_abort(40); let mut p = record P { x = 0; y = 0 }; set(inout p.x, { let inner = system_box_i32_abort(2); p.y <- box_read(inner); box_read(outer) + p.y }); p.x }");
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
                assert_eq!(state.released, [2, 40]);
            }
            Some(n) => {
                assert_eq!(result.unwrap_err(), "runtime trap");
                assert_eq!((state.allocations, state.frees), (n, 0));
            }
        }
    }
}

#[test]
fn projection_overlap_is_symmetric_and_matches_prefix_rules() {
    let mut paths = vec![vec![]];
    for depth in 1..=3 {
        for mut value in 0..3usize.pow(depth) {
            let mut path = Vec::new();
            for record in 0..depth as usize {
                path.push(FieldStep {
                    record,
                    field: value % 3,
                });
                value /= 3;
            }
            paths.push(path);
        }
    }
    for a in &paths {
        for b in &paths {
            let overlap = a.starts_with(b) || b.starts_with(a);
            for exclusive in [false, true] {
                for write in [false, true] {
                    let local = Local {
                        id: LocalId(0),
                        ty: Type::Adt {
                            id: 0,
                            owned: false,
                        },
                        mutable: true,
                        initialization: Initialization::LiveAtom,
                        loaned: false,
                        shared: 0,
                        field_loans: vec![field_loans::ProjectionLoan {
                            path: a.clone(),
                            exclusive,
                        }],
                        mode: ParamMode::Value,
                    };
                    assert_eq!(
                        field_loans::conflicts(&local, b, write),
                        overlap && (exclusive || write)
                    );
                    let mut reversed = local.clone();
                    reversed.field_loans[0].path = b.clone();
                    assert_eq!(
                        field_loans::conflicts(&reversed, a, write),
                        overlap && (exclusive || write)
                    );
                }
            }
        }
    }
}
