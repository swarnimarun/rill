use super::*;

fn limits(tree: usize, total: usize, depth: usize, work: usize) -> Solver {
    Solver {
        materialization: Budget {
            tree_limit: tree,
            depth_limit: depth,
            remaining: Cell::new(total),
            work: Cell::new(work),
        },
        ..Solver::default()
    }
}
fn nominal(name: &str, args: Vec<Type>) -> Type {
    Type::Nominal(name.to_owned(), args)
}
fn variable(ty: &Type) -> usize {
    let Type::Var(id) = ty else {
        panic!("variable expected")
    };
    *id
}
fn shared(solver: &mut Solver, layers: usize, name: &str) -> Type {
    let mut root = solver.fresh(1);
    for _ in 0..layers {
        let parent = solver.fresh(1);
        solver
            .unify(parent.clone(), nominal(name, vec![root.clone(), root]))
            .unwrap();
        root = parent;
    }
    root
}

#[test]
fn exact_tree_total_and_work_boundaries_are_inclusive() {
    let solver = limits(NODE_WEIGHT, NODE_WEIGHT * 2, 1, 4);
    assert_eq!(solver.copy_type(&Type::Int).unwrap(), Type::Int);
    assert_eq!(solver.copy_type(&Type::Bool).unwrap(), Type::Bool);
    assert_eq!(solver.materialization.remaining.get(), 0);
    assert_eq!(solver.materialization.work.get(), 0);
    assert_eq!(solver.copy_type(&Type::Unit).unwrap_err(), LIMIT_ERROR);
    let solver = limits(NODE_WEIGHT, NODE_WEIGHT - 1, 1, 100);
    assert_eq!(solver.copy_type(&Type::Int).unwrap_err(), LIMIT_ERROR);
    assert_eq!(solver.materialization.remaining.get(), NODE_WEIGHT - 1);
    let solver = limits(NODE_WEIGHT - 1, 1000, 1, 100);
    assert_eq!(solver.copy_type(&Type::Int).unwrap_err(), LIMIT_ERROR);
    let solver = limits(NODE_WEIGHT, NODE_WEIGHT, 1, 1);
    assert_eq!(solver.copy_type(&Type::Int).unwrap_err(), LIMIT_ERROR);
    assert_eq!(solver.materialization.remaining.get(), NODE_WEIGHT);
}

#[test]
fn sizes_and_budget_arithmetic_reject_overflow_without_wrapping() {
    assert!(Size::node(usize::MAX).is_err());
    let mut size = Size {
        weight: usize::MAX,
        depth: 1,
    };
    assert!(size
        .add_child(Size {
            weight: 1,
            depth: 1
        })
        .is_err());
    let mut size = Size {
        weight: 0,
        depth: 1,
    };
    assert!(size
        .add_child(Size {
            weight: 0,
            depth: usize::MAX
        })
        .is_err());
    let solver = limits(usize::MAX, 0, DEPTH_LIMIT, 0);
    assert_eq!(
        solver.materialization.reserve(usize::MAX).unwrap_err(),
        LIMIT_ERROR
    );
    assert_eq!(
        solver.materialization.work(usize::MAX).unwrap_err(),
        LIMIT_ERROR
    );
}

#[test]
fn shared_dag_weight_counts_every_expanded_occurrence_and_name_byte() {
    // Depth 5: 31 Type slots and 15 names, including UTF-8 byte multiplicity.
    let weight = 31 * NODE_WEIGHT + 15 * "Pé".len();
    let mut solver = limits(weight, weight * 2, DEPTH_LIMIT, 1000);
    let root = shared(&mut solver, 4, "Pé");
    let result = solver.resolve(&root).unwrap();
    assert_eq!(solver.materialization.remaining.get(), weight);
    solver.copy_type(&result).unwrap();
    assert_eq!(solver.materialization.remaining.get(), 0);
    assert_eq!(solver.resolve(&root).unwrap_err(), LIMIT_ERROR);
    let mut solver = limits(weight - 1, weight * 2, DEPTH_LIMIT, 1000);
    let root = shared(&mut solver, 4, "Pé");
    assert_eq!(solver.resolve(&root).unwrap_err(), LIMIT_ERROR);
    assert_eq!(solver.materialization.remaining.get(), weight * 2);
}

#[test]
fn enormous_dag_is_rejected_with_linear_preflight_and_no_output() {
    let mut solver = Solver::default();
    let root = shared(&mut solver, 120, "Pair");
    assert_eq!(solver.resolve(&root).unwrap_err(), LIMIT_ERROR);
    assert_eq!(solver.materialization.remaining.get(), TOTAL_LIMIT);
    assert!(WORK_LIMIT - solver.materialization.work.get() < 2000);
}

#[test]
fn replacements_are_raw_fresh_and_charged_for_every_alias_occurrence() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(1);
    solver.unify(a.clone(), b.clone()).unwrap();
    let root = solver.fresh(1);
    solver
        .unify(root.clone(), nominal("Pair", vec![a.clone(), b.clone()]))
        .unwrap();
    let replacement = nominal(
        "LongName",
        vec![Type::CPointer {
            pointee: "Opaque".into(),
            is_const: true,
        }],
    );
    let replacement_weight = NODE_WEIGHT * 2 + "LongName".len() + "Opaque".len();
    let weight = NODE_WEIGHT + "Pair".len() + 2 * replacement_weight;
    solver.materialization = Budget {
        tree_limit: weight,
        remaining: Cell::new(weight),
        ..Budget::default()
    };
    let replacements = HashMap::from([(variable(&a), replacement)]);
    let result = solver.substitute(&root, &replacements).unwrap();
    let Type::Nominal(_, args) = result else {
        panic!("nominal expected")
    };
    assert_eq!(args[0], args[1]);
    assert_eq!(solver.materialization.remaining.get(), 0);
    // Replacing an alias uses the canonical ID; the copied replacement itself
    // is neither recursively substituted nor resolved (even if already bound).
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(1);
    let c = solver.fresh(1);
    solver.unify(b.clone(), Type::Bool).unwrap();
    let replacements = HashMap::from([(variable(&a), b.clone()), (variable(&b), c)]);
    assert_eq!(solver.substitute(&a, &replacements).unwrap(), b);
}

#[test]
fn repeated_small_copies_exhaust_the_shared_total_not_a_per_call_allowance() {
    let ty = nominal("Pair", vec![Type::Int, Type::Bool]);
    let weight = 3 * NODE_WEIGHT + 4;
    let solver = limits(weight, 100 * weight, DEPTH_LIMIT, 10_000);
    for _ in 0..100 {
        assert_eq!(solver.copy_type(&ty).unwrap(), ty);
    }
    assert_eq!(solver.copy_type(&ty).unwrap_err(), LIMIT_ERROR);
}

#[test]
fn depth_boundary_and_failure_cleanup_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut solver = Solver::default();
            let root = solver.fresh(1);
            let mut tree = Type::Bool;
            for _ in 1..DEPTH_LIMIT {
                tree = nominal("One", vec![tree]);
            }
            solver.unify(root.clone(), tree).unwrap();
            // Accepted output uses ordinary public drop within the documented depth.
            drop(solver.resolve(&root).unwrap());
            let outer = solver.fresh(1);
            solver
                .unify(outer.clone(), nominal("One", vec![root]))
                .unwrap();
            assert_eq!(solver.resolve(&outer).unwrap_err(), LIMIT_ERROR);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn failed_mismatch_does_not_expand_or_format_types() {
    let mut solver = Solver::default();
    let root = shared(&mut solver, 120, "Pair");
    solver.materialization.work.set(0);
    assert_eq!(
        solver.unify(root, Type::Bool).unwrap_err(),
        "E-TYPE: incompatible types"
    );
    assert_eq!(solver.materialization.remaining.get(), TOTAL_LIMIT);
}

#[test]
fn malformed_binding_and_parent_cycles_consume_the_preflight_budget() {
    for parent_cycle in [true, false] {
        let mut solver = limits(1000, 1000, DEPTH_LIMIT, 20);
        let a = solver.fresh(1);
        let b = solver.fresh(1);
        if parent_cycle {
            solver.variables[variable(&a)].parent.set(variable(&b));
            solver.variables[variable(&b)].parent.set(variable(&a));
        } else {
            solver.variables[variable(&a)].binding = Some(solver.variables[variable(&a)].term);
        }
        assert_eq!(solver.resolve(&a).unwrap_err(), LIMIT_ERROR);
        assert_eq!(solver.materialization.work.get(), 0);
        assert_eq!(solver.materialization.remaining.get(), 1000);
    }
}

#[test]
fn memoized_subtrees_still_pay_depth_on_a_deeper_path() {
    for reverse in [false, true] {
        let mut solver = limits(10_000, 10_000, 4, 1000);
        let inner = shared(&mut solver, 2, "Pair");
        let nested = solver.fresh(1);
        solver
            .unify(nested.clone(), nominal("One", vec![inner.clone()]))
            .unwrap();
        let mut args = vec![inner, nested];
        if reverse {
            args.reverse();
        }
        assert_eq!(
            solver.resolve(&nominal("Pair", args)).unwrap_err(),
            LIMIT_ERROR
        );
        assert_eq!(solver.materialization.remaining.get(), 10_000);
    }
}

#[test]
fn replacement_lookup_follows_a_rank_changed_representative() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(1);
    let c = solver.fresh(1);
    solver.unify(b.clone(), c.clone()).unwrap();
    solver.unify(a.clone(), b.clone()).unwrap();
    assert_eq!(solver.representative(variable(&a)), variable(&b));
    let scheme = nominal("Pair", vec![a.clone(), c]);
    let substitutions = HashMap::from([(variable(&a), Type::Bool), (variable(&b), Type::Int)]);
    assert_eq!(
        solver.substitute(&scheme, &substitutions).unwrap(),
        nominal("Pair", vec![Type::Int, Type::Int])
    );
}
