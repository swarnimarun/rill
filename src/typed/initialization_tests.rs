use super::*;

fn record(name: &str, fields: Vec<Type>) -> AdtType {
    AdtType {
        symbolic: None,
        origin: AdtOrigin::Source,
        name: name.into(),
        record_fields: Some((0..fields.len()).map(|i| format!("f{i}")).collect()),
        arguments: vec![],
        owned: fields.iter().any(|ty| ty.is_owner()),
        variants: vec![AdtVariant {
            name: name.into(),
            fields,
        }],
    }
}
fn nominal(id: usize, owned: bool) -> Type {
    Type::Adt { id, owned }
}
fn step(record: usize, field: usize) -> FieldStep {
    FieldStep { record, field }
}
fn layouts() -> Vec<AdtType> {
    vec![
        record("Empty", vec![]),
        record("Copy", vec![Type::Int, nominal(0, false)]),
        record("Inner", vec![Type::OwnedI32, Type::OwnedBytes]),
        AdtType {
            symbolic: None,
            origin: AdtOrigin::Source,
            name: "Enum".into(),
            record_fields: None,
            arguments: vec![],
            owned: true,
            variants: vec![AdtVariant {
                name: "Payload".into(),
                fields: vec![nominal(2, true)],
            }],
        },
        record(
            "Outer",
            vec![nominal(2, true), nominal(1, false), nominal(3, true)],
        ),
    ]
}

#[test]
fn initialization_is_type_shaped_and_enums_are_atomic() {
    let types = layouts();
    let state = Initialization::full(nominal(4, true), &types).unwrap();
    assert!(state.fully_initialized());
    assert!(matches!(
        state.selected(&[step(4, 0)]).unwrap(),
        Initialization::Record { record: 2, .. }
    ));
    assert_eq!(
        state.selected(&[step(4, 2)]).unwrap(),
        &Initialization::LiveAtom
    );
    assert!(state.selected(&[step(4, 2), step(3, 0)]).is_err());
    let empty = state.selected(&[step(4, 1), step(1, 1)]).unwrap();
    assert!(empty.fully_initialized());
    assert!(empty.materialized());
    assert_eq!(
        empty,
        &Initialization::Record {
            record: 0,
            fields: vec![]
        }
    );
}

#[test]
fn moves_preserve_siblings_and_retire_only_the_selected_place() {
    let types = layouts();
    let mut state = Initialization::full(nominal(4, true), &types).unwrap();
    state.take(&[step(4, 0), step(2, 0)]).unwrap();
    assert!(!state.fully_initialized());
    assert!(state
        .selected(&[step(4, 0), step(2, 1)])
        .unwrap()
        .fully_initialized());
    assert!(state
        .selected(&[step(4, 1), step(1, 0)])
        .unwrap()
        .fully_initialized());
    let before = state.clone();
    for path in [vec![], vec![step(4, 0)], vec![step(4, 0), step(2, 0)]] {
        assert!(state.take(&path).is_err());
        assert_eq!(state, before);
    }
    state.take(&[step(4, 0), step(2, 1)]).unwrap();
    assert_eq!(
        state.selected(&[step(4, 0)]).unwrap(),
        &Initialization::Record {
            record: 2,
            fields: vec![Initialization::Dead, Initialization::Dead],
        }
    );
    assert!(state.materialized());
}

#[test]
fn moved_parent_blocks_descendants_and_does_not_hide_its_state() {
    let types = layouts();
    let mut state = Initialization::full(nominal(4, true), &types).unwrap();
    state.take(&[step(4, 0)]).unwrap();
    assert!(state.selected(&[step(4, 0), step(2, 0)]).is_err());
    assert_eq!(
        state.selected(&[step(4, 0)]).unwrap(),
        &Initialization::Dead
    );
    assert!(state.selected(&[step(4, 1)]).unwrap().fully_initialized());
    let mut root = Initialization::full(nominal(4, true), &types).unwrap();
    root.take(&[]).unwrap();
    assert!(!root.materialized());
    assert!(root.selected(&[step(4, 1), step(1, 0)]).is_err());
    assert_eq!(
        root.cleanup_entry(LocalId(9), nominal(4, true), &types)
            .unwrap(),
        None
    );
}

#[test]
fn cleanup_is_exact_and_retains_empty_owned_record_shells() {
    let types = layouts();
    let mut state = Initialization::full(nominal(4, true), &types).unwrap();
    state.take(&[step(4, 0), step(2, 0)]).unwrap();
    assert_eq!(
        state.cleanup(nominal(4, true), &types).unwrap(),
        CleanupTree::Record {
            record: 4,
            fields: vec![
                CleanupTree::Record {
                    record: 2,
                    fields: vec![CleanupTree::SkipMoved, CleanupTree::DropWholeOwner]
                },
                CleanupTree::SkipCopy,
                CleanupTree::DropWholeOwner
            ],
        }
    );
    let mut shell = Initialization::full(nominal(2, true), &types).unwrap();
    shell.take(&[step(2, 0)]).unwrap();
    shell.take(&[step(2, 1)]).unwrap();
    assert_eq!(
        shell
            .cleanup_entry(LocalId(0), nominal(2, true), &types)
            .unwrap(),
        Some(Cleanup {
            local: LocalId(0),
            tree: CleanupTree::Record {
                record: 2,
                fields: vec![CleanupTree::SkipMoved, CleanupTree::SkipMoved]
            },
        })
    );
}

#[test]
fn cleanup_rejects_forged_record_shapes_and_copy_holes() {
    let types = layouts();
    for state in [
        Initialization::LiveAtom,
        Initialization::Record {
            record: 4,
            fields: vec![],
        },
        Initialization::Record {
            record: 2,
            fields: vec![Initialization::LiveAtom; 3],
        },
        Initialization::Record {
            record: 4,
            fields: vec![Initialization::LiveAtom; 3],
        },
    ] {
        assert!(state.cleanup(nominal(4, true), &types).is_err());
    }
    assert!(Initialization::Dead.cleanup(Type::Int, &types).is_err());
    let mut state = Initialization::full(nominal(4, true), &types).unwrap();
    state.take(&[step(4, 1), step(1, 0)]).unwrap(); // transition caller must reject Copy moves
    assert!(state.cleanup(nominal(4, true), &types).is_err());
    assert!(Initialization::Record {
        record: 0,
        fields: vec![]
    }
    .cleanup(Type::Int, &types)
    .is_err());
}

#[test]
fn place_identity_rejects_nominal_type_and_field_forgery() {
    let types = layouts();
    assert_eq!(
        places::projected_type(nominal(4, true), &[step(4, 0), step(2, 1)], &types).unwrap(),
        Type::OwnedBytes
    );
    assert_eq!(
        places::projected_type(nominal(4, true), &[step(4, 1), step(1, 0)], &types).unwrap(),
        Type::Int
    );
    for (ty, path) in [
        (nominal(4, false), vec![step(4, 0)]),
        (nominal(4, true), vec![step(2, 0)]),
        (nominal(4, true), vec![step(4, 9)]),
        (nominal(3, true), vec![step(3, 0)]),
        (Type::OwnedI32, vec![step(4, 0)]),
        (nominal(999, true), vec![]),
    ] {
        assert!(places::projected_type(ty, &path, &types).is_err());
    }
}

#[test]
fn normalized_places_keep_root_identity_and_prefix_overlap() {
    let root = Place::root(LocalId(1));
    let left = Place {
        root: root.root,
        path: vec![step(4, 0)],
    };
    let inner = Place {
        root: root.root,
        path: vec![step(4, 0), step(2, 1)],
    };
    let right = Place {
        root: root.root,
        path: vec![step(4, 1)],
    };
    assert!(root.overlaps(&inner) && inner.overlaps(&root));
    assert!(left.overlaps(&inner) && inner.overlaps(&left));
    assert!(!left.overlaps(&right));
    assert!(!Place::root(LocalId(2)).overlaps(&inner));
}

#[test]
fn forwarding_has_no_second_source_path_length_limit() {
    let mut types = vec![record("Leaf", vec![Type::OwnedI32])];
    for id in 1..24 {
        types.push(record(&format!("R{id}"), vec![nominal(id - 1, true)]));
    }
    let path: Vec<_> = (0..24).rev().map(|id| step(id, 0)).collect();
    assert_eq!(
        places::projected_type(nominal(23, true), &path, &types).unwrap(),
        Type::OwnedI32
    );
    let mut state = Initialization::full(nominal(23, true), &types).unwrap();
    state.take(&path).unwrap();
    assert!(!state.fully_initialized());
    state.cleanup(nominal(23, true), &types).unwrap();
}

#[test]
fn malformed_and_excessive_layouts_fail_without_unbounded_expansion() {
    let mut types = layouts();
    types[4].variants[0].fields[0] = nominal(4, true);
    assert!(Initialization::full(nominal(4, true), &types).is_err());
    let mut types = vec![record("Leaf", vec![Type::OwnedI32])];
    for id in 1..16 {
        types.push(record(&format!("R{id}"), vec![nominal(id - 1, true); 2]));
    }
    assert!(Initialization::full(nominal(15, true), &types)
        .unwrap_err()
        .contains("too large"));
    let mut types = vec![record("Leaf", vec![Type::OwnedI32])];
    for id in 1..130 {
        types.push(record(&format!("R{id}"), vec![nominal(id - 1, true)]));
    }
    assert!(Initialization::full(nominal(129, true), &types)
        .unwrap_err()
        .contains("nesting"));
}

fn sample(source: &str) -> TypedMain {
    let p = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&p).unwrap()).unwrap()
}
fn rejects(p: &TypedMain) {
    assert!(verify(p).is_err());
    assert!(evaluate(p).is_err());
    assert!(emit_c(p).is_err());
}

#[test]
fn consumers_reject_forged_cleanup_trees_independently() {
    let original = sample("fn main() { let b = system_box_i32_abort(42); box_read(b) }");
    for tree in [
        CleanupTree::SkipCopy,
        CleanupTree::SkipMoved,
        CleanupTree::Record {
            record: 0,
            fields: vec![CleanupTree::DropWholeOwner],
        },
        CleanupTree::Record {
            record: 0,
            fields: vec![CleanupTree::DropWholeOwner, CleanupTree::DropWholeOwner],
        },
    ] {
        let mut bad = original.clone();
        let ExprKind::OwnerLet { cleanup, .. } =
            &mut bad.functions[bad.entry].body.value_expr_mut().unwrap().kind
        else {
            panic!()
        };
        cleanup.as_mut().unwrap().tree = tree;
        rejects(&bad);
    }
}

#[test]
fn consumers_reject_duplicate_reordered_and_borrowed_parameter_cleanup() {
    let original = sample("fn consume(a: SystemBoxI32, b: SystemBoxI32) { box_read(a) + box_read(b) } fn main() { let a = system_box_i32_abort(20); let b = system_box_i32_abort(22); consume(move a, move b) }");
    let index = original
        .functions
        .iter()
        .position(|f| f.params.len() == 2)
        .unwrap();
    let mut bad = original.clone();
    bad.functions[index].param_cleanup.reverse();
    rejects(&bad);
    let mut bad = original.clone();
    let duplicate = bad.functions[index].param_cleanup[0].clone();
    bad.functions[index].param_cleanup.push(duplicate);
    rejects(&bad);
    let mut bad = original;
    bad.functions[index].params[0].2 = ParamMode::Read;
    rejects(&bad);
}

#[test]
fn equal_root_liveness_does_not_hide_different_subtree_or_loan_states() {
    let types = layouts();
    let initialization = Initialization::full(nominal(4, true), &types).unwrap();
    let local = Local {
        id: LocalId(0),
        ty: nominal(4, true),
        mutable: true,
        initialization,
        loaned: false,
        shared: 0,
        field_loans: vec![],
        mode: ParamMode::Value,
    };
    let mut a = HashMap::from([(local.id, local.clone())]);
    let mut b = a.clone();
    a.get_mut(&local.id)
        .unwrap()
        .initialization
        .take(&[step(4, 0)])
        .unwrap();
    b.get_mut(&local.id)
        .unwrap()
        .initialization
        .take(&[step(4, 2)])
        .unwrap();
    assert!(!same_owner_states(&a, &b));
    let mut b = a.clone();
    b.get_mut(&local.id)
        .unwrap()
        .field_loans
        .push(field_loans::ProjectionLoan {
            path: vec![step(4, 1)],
            exclusive: false,
        });
    assert!(!same_owner_states(&a, &b));
    assert!(!Lower::same_states(
        &HashMap::from([("r".into(), a[&local.id].clone())]),
        &HashMap::from([("r".into(), b[&local.id].clone())])
    ));
}

#[test]
fn place_move_checks_copy_authority_and_all_overlapping_intervals() {
    let types = layouts();
    let original = Local {
        id: LocalId(0),
        ty: nominal(4, true),
        mutable: false,
        initialization: Initialization::full(nominal(4, true), &types).unwrap(),
        loaned: false,
        shared: 0,
        field_loans: vec![],
        mode: ParamMode::Value,
    };
    let mut local = original.clone();
    assert!(places::take(&mut local, &[step(4, 1), step(1, 0)], &types).is_err());
    assert_eq!(local.initialization, original.initialization);
    for mode in [ParamMode::Read, ParamMode::Inout] {
        let mut local = original.clone();
        local.mode = mode;
        assert!(places::take(&mut local, &[step(4, 0), step(2, 0)], &types).is_err());
        assert_eq!(local.initialization, original.initialization);
    }
    let mut local = original.clone();
    local.field_loans.push(field_loans::ProjectionLoan {
        path: vec![step(4, 0), step(2, 0)],
        exclusive: false,
    });
    assert!(places::take(&mut local, &[], &types).is_err());
    assert!(places::take(&mut local, &[step(4, 0)], &types).is_err());
    assert!(places::take(&mut local, &[step(4, 0), step(2, 0)], &types).is_err());
    assert_eq!(
        places::take(&mut local, &[step(4, 0), step(2, 1)], &types).unwrap(),
        Type::OwnedBytes
    );
    assert!(local
        .initialization
        .selected(&[step(4, 0), step(2, 0)])
        .unwrap()
        .fully_initialized());
}

#[test]
fn exhaustive_small_move_sequences_match_an_independent_prefix_oracle() {
    let types = layouts();
    let targets = [
        vec![],
        vec![step(4, 0)],
        vec![step(4, 0), step(2, 0)],
        vec![step(4, 0), step(2, 1)],
        vec![step(4, 2)],
    ];
    // Numeric paths here deliberately do not use the implementation's tree
    // traversal, prefix-overlap routine or cleanup derivation as the oracle.
    let indices = [vec![], vec![0], vec![0, 0], vec![0, 1], vec![2]];
    for encoded in 0..625usize {
        let mut value = encoded;
        let mut state = Initialization::full(nominal(4, true), &types).unwrap();
        let mut dead: Vec<Vec<usize>> = vec![];
        for _ in 0..4 {
            let choice = value % 5;
            value /= 5;
            let path = &indices[choice];
            let available = !dead
                .iter()
                .any(|old| old.starts_with(path) || path.starts_with(old));
            let before = state.clone();
            assert_eq!(state.take(&targets[choice]).is_ok(), available);
            if available {
                dead.push(path.clone());
            } else {
                assert_eq!(state, before);
            }
            let moved = |p: &[usize]| dead.iter().any(|old| p.starts_with(old));
            let atom = |p: &[usize]| {
                if moved(p) {
                    CleanupTree::SkipMoved
                } else {
                    CleanupTree::DropWholeOwner
                }
            };
            let expected = if moved(&[]) {
                CleanupTree::SkipMoved
            } else {
                CleanupTree::Record {
                    record: 4,
                    fields: vec![
                        if moved(&[0]) {
                            CleanupTree::SkipMoved
                        } else {
                            CleanupTree::Record {
                                record: 2,
                                fields: vec![atom(&[0, 0]), atom(&[0, 1])],
                            }
                        },
                        CleanupTree::SkipCopy,
                        atom(&[2]),
                    ],
                }
            };
            assert_eq!(state.cleanup(nominal(4, true), &types).unwrap(), expected);
            assert_eq!(state.fully_initialized(), dead.is_empty());
            assert_eq!(state.materialized(), !moved(&[]));
        }
    }
}
