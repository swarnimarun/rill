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
fn int(n: i32) -> ExprNode {
    ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(n),
    }
}
fn body(p: &mut TypedMain) -> &mut ExprNode {
    p.functions[p.entry].body.value_expr_mut().unwrap()
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
#[test]
fn record_shape_names_and_ownership_are_independently_checked() {
    let original = lower("type P = { x: i32; y: i32 }\nfn main() { record P { x = 20; y = 22 } }");
    for case in 0..7 {
        let mut bad = original.clone();
        let record = &mut bad.adts[0];
        match case {
            0 => record.record_fields = None,
            1 => record.record_fields = Some(vec!["x".into(), "x".into()]),
            2 => record.record_fields = Some(vec!["x".into()]),
            3 => record.record_fields = Some(vec![String::new(), "y".into()]),
            4 => record.variants.push(record.variants[0].clone()),
            5 => record.owned = true,
            _ => record.variants[0].fields[0] = Type::OwnedI32,
        }
        rejects(&bad);
    }
    let mut bad = lower("type E = E\nfn main() { E }");
    bad.adts[0].record_fields = Some(vec![]);
    rejects(&bad);
    let mut bad = original;
    let ExprKind::Record { adt, fields } = &body(&mut bad).kind else {
        panic!()
    };
    let positional = ExprKind::Constructor {
        adt: *adt,
        variant: 0,
        fields: fields.iter().map(|(_, v)| v.clone()).collect(),
    };
    body(&mut bad).kind = positional;
    rejects(&bad);
    let mut bad = lower("type P = { x: i32 }\nfn main() { match record P { x = 42 } { _ => 42 } }");
    let ExprKind::CopyMatch { arms, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    arms[0].pattern = CopyPattern::Constructor {
        adt: 0,
        variant: 0,
        fields: vec![CopyPattern::Wildcard],
    };
    rejects(&bad);
}
#[test]
fn record_construction_requires_every_field_once_at_its_type() {
    let original =
        lower("type P = { x: i32; y: bool }\nfn main() { record P { y = true; x = 42 } }");
    for case in 0..5 {
        let mut bad = original.clone();
        let ExprKind::Record { adt, fields } = &mut body(&mut bad).kind else {
            panic!()
        };
        match case {
            0 => {
                fields.pop();
            }
            1 => fields[1].0 = fields[0].0,
            2 => fields[0].0 = 999,
            3 => fields[0].1 = int(1),
            _ => *adt = 999,
        }
        rejects(&bad);
    }
}
#[test]
fn every_projection_step_has_checked_identity_index_and_scope() {
    let original=lower("type I = { x: i32; y: bool }\ntype O = { inner: I; other: i32 }\nfn main() { let p = record O { inner = record I { x = 42; y = true }; other = 0 }; p.inner.x }");
    for case in 0..6 {
        let mut bad = original.clone();
        let node = tail(body(&mut bad));
        let ExprKind::FieldRead(place) = &mut node.kind else {
            panic!()
        };
        match case {
            0 => place.root = LocalId(u32::MAX),
            1 => place.path.clear(),
            2 => place.path = vec![place.path[0].clone(); 17],
            3 => place.path[0].record = place.path[1].record,
            4 => place.path[1].field = 999,
            _ => node.ty = Type::Bool,
        }
        rejects(&bad);
    }
}
#[test]
fn field_accesses_obey_mutability_and_whole_root_loan_intervals() {
    let mut bad =
        lower("type P = { x: i32 }\nfn main() { let mut p = record P { x = 1 }; p.x <- 2 }");
    let ExprKind::Let { mutable, .. } = &mut body(&mut bad).kind else {
        panic!()
    };
    *mutable = false;
    rejects(&bad);
    for exclusive in [false, true] {
        let function = if exclusive {
            "fn use(inout p: P, n) { p.x <- n }"
        } else {
            "fn use(read p: P, n) { p.x + n }"
        };
        let arg = if exclusive { "inout p" } else { "p" };
        let mut bad=lower(&format!("type P = {{ x: i32 }}\n{function}\nfn main() {{ let mut p = record P {{ x = 1 }}; use({arg}, 2) }}"));
        let ExprKind::Let { local, .. } = &body(&mut bad).kind else {
            panic!()
        };
        let place = FieldPlace {
            root: *local,
            path: vec![FieldStep {
                record: 0,
                field: 0,
            }],
        };
        let replacement = if exclusive {
            ExprNode {
                ty: Type::Int,
                kind: ExprKind::FieldRead(place),
            }
        } else {
            ExprNode {
                ty: Type::Int,
                kind: ExprKind::Let {
                    local: LocalId(u32::MAX),
                    mutable: false,
                    value: Box::new(ExprNode {
                        ty: Type::Unit,
                        kind: ExprKind::FieldAssign(place, Box::new(int(9))),
                    }),
                    body: Box::new(int(2)),
                },
            }
        };
        let ExprKind::Call(_, args) = &mut tail(body(&mut bad)).kind else {
            panic!()
        };
        args[1] = CallArg::Value(replacement);
        rejects(&bad);
    }
}
#[test]
fn record_fields_visit_owner_effects_and_cleanup_in_written_order() {
    let p=lower("type P = { x: i32; y: i32 }\nfn main() { let p = record P { y = { let owner = system_box_i32_abort(2); box_read(owner) }; x = { let owner = system_box_i32_abort(40); box_read(owner) } }; p.x + p.y }");
    for fail_after in [None, Some(0), Some(1)] {
        let mut state = EvalState {
            fail_after,
            ..EvalState::default()
        };
        let result = evaluate_function(&p, p.entry, vec![], 1, &mut state);
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
                assert_eq!((state.allocations, state.frees), (n, n));
            }
        }
    }
}

#[test]
fn deferred_scc_projection_metadata_is_independently_rechecked() {
    let original = lower(
        "type P<a> = {value:a;flag:bool}\n\
         type Q<a> = {value:a;flag:bool}\n\
         fn first(x){let p=later(x);p.value}\n\
         fn later(x:P<i32>){if true{x}else{record P{value=first(x);flag=true}}}\n\
         fn main(){let other=record Q{value=0;flag=false};first(record P{value=42;flag=true})}",
    );
    verify(&original).unwrap();
    assert_eq!(evaluate(&original).unwrap().to_string(), "42");
    let function = original
        .functions
        .iter()
        .position(|function| {
            matches!(&function.body.value_expr().unwrap().kind,
                ExprKind::Let { body, .. } if matches!(body.kind, ExprKind::FieldRead(_)))
        })
        .unwrap();
    let same_shape = original
        .adts
        .iter()
        .position(|adt| adt.name == "Q")
        .unwrap();
    let record = original
        .adts
        .iter()
        .position(|adt| adt.name == "P")
        .unwrap();
    assert_eq!(
        original.adts[record].record_fields,
        original.adts[same_shape].record_fields
    );
    assert_eq!(
        original.adts[record].variants[0].fields,
        original.adts[same_shape].variants[0].fields
    );
    for case in 0..7 {
        let mut bad = original.clone();
        let node = tail(bad.functions[function].body.value_expr_mut().unwrap());
        let ExprKind::FieldRead(place) = &mut node.kind else {
            panic!("deferred result projection");
        };
        match case {
            0 => place.root = LocalId(u32::MAX),
            1 => place.path[0].record = same_shape,
            2 => place.path[0].field = usize::MAX,
            3 => place.path.clear(),
            4 => node.ty = Type::Bool,
            5 => {
                // Names and fields disagree on the arity of the resolved schema.
                bad.adts[record].variants[0].fields.pop();
            }
            _ => bad.adts[record].record_fields = None,
        }
        rejects(&bad);
    }
}

#[test]
fn deferred_projection_local_uses_retain_binding_identity_under_shadowing() {
    let parsed = crate::parser::parse(
        "type P={value:i32}\n\
         fn first(x){let p=later(x);let old=p.value;let p=record P{value=2};old+p.value}\n\
         fn later(x:P){if true{x}else{record P{value=first(x)}}}\n\
         fn main(){first(record P{value=40})}",
    )
    .unwrap();
    let checked = crate::check::analyze(&parsed).unwrap();
    let Expr::Let(_, _, body) = &parsed.functions[0].body else {
        panic!()
    };
    let Expr::Let(_, old, body) = body.as_ref() else {
        panic!()
    };
    let Expr::Let(_, _, body) = body.as_ref() else {
        panic!()
    };
    let Expr::Binary(_, _, new) = body.as_ref() else {
        panic!()
    };
    let old_id = checked.local_use(old).expect("deferred occurrence binding");
    let new_id = checked.local_use(new).expect("shadowed occurrence binding");
    assert_ne!(old_id, new_id);
    for occurrence in [old.as_ref(), new.as_ref()] {
        let fields = checked.field_info(occurrence).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].record.as_str(), fields[0].index, fields[0].arity),
            ("P", 0, 1)
        );
    }
    let lowered = lower_main(&checked).unwrap();
    verify(&lowered).unwrap();
    assert_eq!(evaluate(&lowered).unwrap().to_string(), "42");
}
