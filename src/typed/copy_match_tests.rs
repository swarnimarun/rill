use super::*;

fn lower(source: &str) -> TypedMain {
    let program = crate::parser::parse(source).unwrap();
    let checked = crate::check::analyze(&program).unwrap();
    lower_main(&checked).unwrap()
}
fn sample() -> TypedMain {
    lower("type Option<a> = None | Some(a)\nfn main() { match Some(Some(42)) { Some(Some(x)) => x | Some(None) => 1 | None => 0 } }")
}
fn arms(program: &mut TypedMain) -> &mut Vec<CopyArm> {
    let ExprKind::CopyMatch { arms, .. } = &mut program.functions[program.entry].body.kind else {
        panic!()
    };
    arms
}
fn rejects(program: &TypedMain, text: &str) {
    assert!(verify(program).unwrap_err().contains(text));
    assert!(emit_c(program).is_err());
    assert!(evaluate(program).is_err());
}

#[test]
fn verifier_checks_every_nested_pattern_type_tag_and_arity() {
    let original = sample();
    for replacement in [
        CopyPattern::Int(42),
        CopyPattern::Bool(true),
        CopyPattern::Unit,
        CopyPattern::Constructor {
            adt: 999,
            variant: 0,
            fields: vec![],
        },
        CopyPattern::Constructor {
            adt: 1,
            variant: 999,
            fields: vec![],
        },
        CopyPattern::Constructor {
            adt: 1,
            variant: 1,
            fields: vec![],
        },
        CopyPattern::Constructor {
            adt: 1,
            variant: 1,
            fields: vec![CopyPattern::Constructor {
                adt: 0,
                variant: 1,
                fields: vec![CopyPattern::Bool(true)],
            }],
        },
    ] {
        let mut forged = original.clone();
        arms(&mut forged)[0].pattern = replacement;
        rejects(&forged, "E-TYPED-MATCH");
    }
}

#[test]
fn verifier_rechecks_nested_and_primitive_exhaustiveness() {
    let mut forged = sample();
    arms(&mut forged).remove(1);
    rejects(&forged, "nonexhaustive");
    let mut forged = lower("fn main() { match true { true => 42 | false => 0 } }");
    arms(&mut forged).pop();
    rejects(&forged, "nonexhaustive");
    let mut forged = lower("fn main() { match 42 { 42 => 1 | _ => 0 } }");
    arms(&mut forged).pop();
    rejects(&forged, "nonexhaustive");
    arms(&mut forged).clear();
    rejects(&forged, "nonexhaustive");
}

#[test]
fn verifier_checks_pattern_identity_scope_and_arm_results() {
    let original =
        lower("type Pair = Pair(i32, i32)\nfn main() { match Pair(1, 2) { Pair(x, y) => x + y } }");
    let mut forged = original.clone();
    let CopyPattern::Constructor { fields, .. } = &mut arms(&mut forged)[0].pattern else {
        panic!()
    };
    fields[1] = fields[0].clone();
    rejects(&forged, "duplicate");
    let mut forged = sample();
    arms(&mut forged)[1].body = arms(&mut forged)[0].body.clone();
    rejects(&forged, "outside local scope");
    let mut forged = sample();
    arms(&mut forged)[0].body = ExprNode {
        ty: Type::Bool,
        kind: ExprKind::Bool(true),
    };
    rejects(&forged, "inconsistent arm result");
}

#[test]
fn copy_matches_transfer_outer_owners_once_and_retain_cleanup_order() {
    for condition in ["true", "false"] {
        let source = format!("type Pair = Pair(bool, i32)\nfn main() {{ let first = system_box_i32_abort(1); let second = system_box_i32_abort(42); let kept = match Pair({condition}, 7) {{ Pair(true, _) => move second | Pair(false, n) => move second }}; box_read(kept) }}");
        let typed = lower(&source);
        let mut state = EvalState::default();
        let result = evaluate_function(&typed, typed.entry, vec![], 1, &mut state).unwrap();
        assert!(matches!(
            result,
            EvalArg::Value(crate::interpreter::Value::Int(42))
        ));
        assert_eq!(
            state.allocation_trace,
            vec![
                (0, SystemAllocationKind::BoxI32),
                (1, SystemAllocationKind::BoxI32)
            ]
        );
        assert_eq!(
            state.release_trace,
            vec![
                (1, SystemAllocationKind::BoxI32),
                (0, SystemAllocationKind::BoxI32)
            ]
        );
        assert!(state.storage.is_empty());
    }
}

#[test]
fn verifier_rejects_copy_match_consumption_and_inconsistent_owner_joins() {
    let mut typed = lower("fn main() { let b = system_box_i32_abort(42); let value = match true { true => move b | false => move b }; box_read(value) }");
    let ExprKind::OwnerLet { local, body, .. } = &mut typed.functions[typed.entry].body.kind else {
        panic!()
    };
    let ExprKind::OwnerLet {
        init: OwnerInit::Expr(value),
        ..
    } = &mut body.kind
    else {
        panic!()
    };
    let ExprKind::CopyMatch {
        value: scrutinee, ..
    } = &mut value.kind
    else {
        panic!()
    };
    **scrutinee = ExprNode {
        ty: Type::OwnedI32,
        kind: ExprKind::OwnerMove(*local),
    };
    rejects(&typed, "cannot copy an owner");

    let mut typed = lower("fn take(b) { box_read(b) }\nfn main() { let b = system_box_i32_abort(42); match true { true => take(move b) | false => take(move b) } }");
    let ExprKind::OwnerLet { body, .. } = &mut typed.functions[typed.entry].body.kind else {
        panic!()
    };
    let ExprKind::CopyMatch { arms, .. } = &mut body.kind else {
        panic!()
    };
    arms[1].body = ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(0),
    };
    rejects(&typed, "incompatible arm ownership");
}

#[test]
fn copy_match_call_traversal_retains_owner_recursion_rejection() {
    let mut typed = lower("fn helper() { 42 }\nfn main() { let b = system_box_i32_abort(0); match true { true => helper() | false => 0 } }");
    let entry = typed.entry;
    let ExprKind::OwnerLet { body, .. } = &mut typed.functions[entry].body.kind else {
        panic!()
    };
    let ExprKind::CopyMatch { arms, .. } = &mut body.kind else {
        panic!()
    };
    arms[0].body.kind = ExprKind::Call(entry, vec![]);
    rejects(&typed, "recursive owner");
}

#[test]
fn pattern_matrix_matches_every_boolean_product_pattern_subset() {
    let base =
        lower("type Pair = Pair(bool, bool)\nfn main() { match Pair(true, false) { _ => 42 } }");
    let ExprKind::CopyMatch { value, .. } = &base.functions[base.entry].body.kind else {
        panic!()
    };
    let Type::Adt { id, .. } = value.ty else {
        panic!()
    };
    let fields = [
        CopyPattern::Wildcard,
        CopyPattern::Bool(false),
        CopyPattern::Bool(true),
    ];
    // Each of the nine patterns chooses wildcard/false/true independently for
    // both fields. Compare all 512 pattern subsets with four concrete values.
    for mask in 0..512u32 {
        let mut program = base.clone();
        let mut patterns = Vec::new();
        let mut covered = 0u8;
        for row in 0..9 {
            if mask & (1 << row) == 0 {
                continue;
            }
            let (a, b) = (row / 3, row % 3);
            patterns.push(CopyArm {
                pattern: CopyPattern::Constructor {
                    adt: id,
                    variant: 0,
                    fields: vec![fields[a].clone(), fields[b].clone()],
                },
                body: ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Int(row as i32),
                },
            });
            for va in 0..2 {
                for vb in 0..2 {
                    if (a == 0 || a == va + 1) && (b == 0 || b == vb + 1) {
                        covered |= 1 << (va * 2 + vb);
                    }
                }
            }
        }
        *arms(&mut program) = patterns;
        assert_eq!(
            verify(&program).is_ok(),
            covered == 15,
            "mask={mask:09b}, covered={covered:04b}"
        );
    }
}

#[test]
fn wide_pattern_matrix_has_an_explicit_bounded_work_policy() {
    let count = 4096;
    let ty = Type::Adt {
        id: 0,
        owned: false,
    };
    let mut program = TypedMain {
        allocation_contract: None,
        adts: vec![AdtType {
            origin: super::AdtOrigin::Source,
            record_fields: None,
            name: "Wide".into(),
            arguments: vec![],
            owned: false,
            variants: vec![AdtVariant {
                name: "Wide".into(),
                fields: vec![Type::Unit; count],
            }],
        }],
        entry: 0,
        functions: vec![TypedFunction {
            params: vec![],
            result: Type::Int,
            param_cleanup: vec![],
            body: ExprNode {
                ty: Type::Int,
                kind: ExprKind::CopyMatch {
                    value: Box::new(ExprNode {
                        ty,
                        kind: ExprKind::Constructor {
                            adt: 0,
                            variant: 0,
                            fields: vec![
                                ExprNode {
                                    ty: Type::Unit,
                                    kind: ExprKind::Unit
                                };
                                count
                            ],
                        },
                    }),
                    arms: vec![CopyArm {
                        pattern: CopyPattern::Constructor {
                            adt: 0,
                            variant: 0,
                            fields: vec![CopyPattern::Unit; count],
                        },
                        body: ExprNode {
                            ty: Type::Int,
                            kind: ExprKind::Int(42),
                        },
                    }],
                },
            },
        }],
    };
    rejects(&program, "pattern analysis budget exceeded");
    let CopyPattern::Constructor { fields, .. } = &mut arms(&mut program)[0].pattern else {
        panic!()
    };
    fields.fill(CopyPattern::Wildcard);
    // One fully irrefutable row does not enumerate the product of its fields.
    verify(&program).unwrap();
}

#[test]
fn source_and_independent_typed_coverage_match_nested_truth_tables() {
    let rows = [
        ("R(N, _, ())", 0b000011u8),
        ("R(S(false), _, ())", 0b001100),
        ("R(S(true), _, ())", 0b110000),
        ("R(S(bound), true, ())", 0b101000),
        ("R(S(_), false, ())", 0b010100),
        ("R(_, true, ())", 0b101010),
        ("R(_, false, ())", 0b010101),
        ("R(N, false, ())", 0b000001),
        ("whole", 0b111111),
    ];
    let source = format!("type O<a> = N | S(a)\ntype R = R(O<bool>, bool, unit)\nfn main() {{ match R(N, false, ()) {{ {} }} }}", rows.iter().map(|(p,_)| format!("{p} => 42")).collect::<Vec<_>>().join(" | "));
    let source_base = crate::parser::parse(&source).unwrap();
    let typed_base = lower(&source);
    for mask in 0..(1 << rows.len()) {
        let mut source_program = source_base.clone();
        let mut typed_program = typed_base.clone();
        let mut coverage = 0;
        let crate::ast::Expr::Match(_, source_arms) = &mut source_program.functions[0].body else {
            panic!()
        };
        let mut i = 0;
        source_arms.retain(|_| {
            let keep = mask & (1 << i) != 0;
            if keep {
                coverage |= rows[i].1;
            }
            i += 1;
            keep
        });
        let mut i = 0;
        arms(&mut typed_program).retain(|_| {
            i += 1;
            mask & (1 << (i - 1)) != 0
        });
        let expected = coverage == 0b111111;
        assert_eq!(
            crate::check::check(&source_program).is_ok(),
            expected,
            "source subset {mask}"
        );
        assert_eq!(
            verify(&typed_program).is_ok(),
            expected,
            "typed subset {mask}"
        );
    }
}
