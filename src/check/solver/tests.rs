use super::*;

#[test]
fn callable_children_unify_substitute_and_reject_infinite_types() {
    use crate::ast::ParamMode;
    let mut solver = Solver::default();
    let environment = solver.fresh(1);
    let value = solver.fresh(1);
    let callable = crate::check::callables::callable_type(
        environment.clone(),
        vec![value.clone()],
        vec![ParamMode::Value],
        value.clone(),
    );
    let mut free = HashSet::new();
    solver.free_type(&callable, &mut free);
    assert_eq!(free.len(), 2);
    let concrete = crate::check::callables::callable_type(
        Type::Nominal("$fn:test".into(), vec![]),
        vec![Type::Int],
        vec![ParamMode::Value],
        Type::Int,
    );
    solver.unify(callable.clone(), concrete.clone()).unwrap();
    assert_eq!(solver.resolve(&callable).unwrap(), concrete);

    let mut solver = Solver::default();
    let Type::Var(env) = solver.fresh(1) else {
        unreachable!()
    };
    let Type::Var(argument) = solver.fresh(1) else {
        unreachable!()
    };
    let callable = crate::check::callables::callable_type(
        Type::Var(env),
        vec![Type::Var(argument)],
        vec![ParamMode::Value],
        Type::Var(argument),
    );
    let substituted = solver
        .substitute(
            &callable,
            &HashMap::from([(env, Type::Unit), (argument, Type::Bool)]),
        )
        .unwrap();
    assert_eq!(
        substituted,
        crate::check::callables::callable_type(
            Type::Unit,
            vec![Type::Bool],
            vec![ParamMode::Value],
            Type::Bool
        )
    );
    assert!(solver
        .unify(Type::Var(argument), callable)
        .unwrap_err()
        .contains("infinite type"));
    let read = crate::check::callables::callable_type(
        Type::Unit,
        vec![Type::Int],
        vec![ParamMode::Read],
        Type::Int,
    );
    let value = crate::check::callables::callable_type(
        Type::Unit,
        vec![Type::Int],
        vec![ParamMode::Value],
        Type::Int,
    );
    assert!(solver
        .unify(read, value)
        .unwrap_err()
        .contains("incompatible"));
}

fn nominal(name: &str, args: Vec<Type>) -> Type {
    Type::Nominal(name.to_owned(), args)
}
fn id(ty: &Type) -> usize {
    let Type::Var(id) = ty else {
        panic!("expected variable")
    };
    *id
}

#[test]
fn long_shallow_unions_do_not_recurse() {
    let mut solver = Solver::default();
    let vars: Vec<_> = (0..100_000).map(|_| solver.fresh(1)).collect();
    for pair in vars.windows(2) {
        solver.unify(pair[0].clone(), pair[1].clone()).unwrap();
    }
    solver
        .unify(vars.last().unwrap().clone(), Type::Int)
        .unwrap();
    for ty in &vars {
        assert_eq!(solver.resolve(ty).unwrap(), Type::Int);
    }
    assert!(solver.variables.iter().all(|var| var.rank <= 1));
}

#[test]
fn existing_long_parent_path_is_fully_compressed() {
    let mut solver = Solver::default();
    for _ in 0..100_000 {
        solver.fresh(1);
    }
    // Bypass ranked union only to exercise compression on an adverse chain.
    for id in 0..99_999 {
        solver.variables[id].parent.set(id + 1);
    }
    assert_eq!(solver.representative(0), 99_999);
    assert!(solver
        .variables
        .iter()
        .all(|var| var.parent.get() == 99_999));
}

#[test]
fn occurs_checks_reject_direct_and_indirect_infinite_types() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(2);
    assert!(solver
        .unify(a.clone(), nominal("Box", vec![a.clone()]))
        .unwrap_err()
        .contains("infinite"));
    solver
        .unify(a.clone(), nominal("Box", vec![b.clone()]))
        .unwrap();
    assert!(solver
        .unify(b, nominal("Box", vec![a]))
        .unwrap_err()
        .contains("infinite"));
}

#[test]
fn nominal_identity_arity_and_primitive_metadata_are_preserved() {
    for (left, right) in [
        (
            nominal("One", vec![Type::Int]),
            nominal("Other", vec![Type::Int]),
        ),
        (nominal("One", vec![Type::Int]), nominal("One", vec![])),
        (Type::Bytes(1), Type::Bytes(2)),
        (
            Type::CPointer {
                pointee: "A".into(),
                is_const: false,
            },
            Type::CPointer {
                pointee: "B".into(),
                is_const: false,
            },
        ),
        (
            Type::CPointer {
                pointee: "A".into(),
                is_const: false,
            },
            Type::CPointer {
                pointee: "A".into(),
                is_const: true,
            },
        ),
    ] {
        assert!(Solver::default().unify(left, right).is_err());
    }
}

#[test]
fn binding_and_union_lower_all_reachable_free_variables() {
    let mut solver = Solver::default();
    let outer = solver.fresh(0);
    let inner = solver.fresh(3);
    let alias = solver.fresh(4);
    let nested = solver.fresh(5);
    solver.unify(inner.clone(), alias.clone()).unwrap();
    solver
        .unify(nested.clone(), nominal("Box", vec![alias.clone()]))
        .unwrap();
    solver
        .unify(outer, nominal("Pair", vec![inner.clone(), nested.clone()]))
        .unwrap();
    assert_eq!(solver.level(id(&inner)), 0);
    assert_eq!(solver.level(id(&alias)), 0);
    let mut free = HashSet::new();
    solver.free_type(&nested, &mut free);
    assert_eq!(free.len(), 1);
    assert!(free.iter().all(|id| solver.level(*id) == 0));
}

#[test]
fn rank_switch_keeps_instantiations_fresh_and_repeated_occurrences_equal() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(1);
    let c = solver.fresh(1);
    solver.unify(b.clone(), c).unwrap();
    solver.unify(a.clone(), b).unwrap();
    assert_ne!(solver.resolve(&a).unwrap(), a);
    let scheme = nominal("Pair", vec![a.clone(), a]);
    let mut free = HashSet::new();
    solver.free_type(&scheme, &mut free);
    let quantified = *free.iter().next().unwrap();
    let first = solver.fresh(2);
    let second = solver.fresh(2);
    let first = solver
        .substitute(&scheme, &HashMap::from([(quantified, first)]))
        .unwrap();
    let second = solver
        .substitute(&scheme, &HashMap::from([(quantified, second)]))
        .unwrap();
    solver
        .unify(first.clone(), nominal("Pair", vec![Type::Int, Type::Int]))
        .unwrap();
    solver
        .unify(
            second.clone(),
            nominal("Pair", vec![Type::Bool, Type::Bool]),
        )
        .unwrap();
    assert_ne!(
        solver.resolve(&first).unwrap(),
        solver.resolve(&second).unwrap()
    );
    assert!(solver
        .unify(second, nominal("Pair", vec![Type::Bool, Type::Int]))
        .is_err());
}

#[test]
fn shared_dags_unify_and_lower_without_exponential_expansion() {
    let mut solver = Solver::default();
    let leaf = solver.fresh(2);
    let other_leaf = solver.fresh(3);
    let mut tree = leaf.clone();
    let mut other_tree = other_leaf.clone();
    for _ in 0..120 {
        let parent = solver.fresh(2);
        let other_parent = solver.fresh(3);
        solver
            .unify(parent.clone(), nominal("Pair", vec![tree.clone(), tree]))
            .unwrap();
        solver
            .unify(
                other_parent.clone(),
                nominal("Pair", vec![other_tree.clone(), other_tree]),
            )
            .unwrap();
        tree = parent;
        other_tree = other_parent;
    }
    solver.unify(tree.clone(), other_tree).unwrap();
    assert_eq!(
        solver.resolve(&leaf).unwrap(),
        solver.resolve(&other_leaf).unwrap()
    );
    let outer = solver.fresh(0);
    solver.unify(outer, tree.clone()).unwrap();
    assert_eq!(solver.level(id(&leaf)), 0);
    assert!(solver.unify(leaf, tree).unwrap_err().contains("infinite"));
}

#[test]
fn deep_private_operations_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut solver = Solver::default();
            let outer = solver.fresh(0);
            let leaf = solver.fresh(2);
            let mut tree = leaf.clone();
            for _ in 0..50_000 {
                tree = nominal("One", vec![tree]);
            }
            solver.unify(outer.clone(), tree).unwrap();
            let mut free = HashSet::new();
            solver.free_type(&outer, &mut free);
            assert_eq!(free.len(), 1);
            assert_eq!(solver.level(id(&leaf)), 0);
            assert!(solver
                .unify(leaf.clone(), outer.clone())
                .unwrap_err()
                .contains("infinite"));
            // Private walks remain stack-safe at 50k depth. Public trees now
            // deliberately fail before construction, so cleanup cannot recurse.
            assert_eq!(
                solver.resolve(&outer).unwrap_err(),
                materialize::LIMIT_ERROR
            );
            assert_eq!(
                solver
                    .substitute(&outer, &HashMap::from([(id(&leaf), Type::Bool)]))
                    .unwrap_err(),
                materialize::LIMIT_ERROR
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

// Independent recursive substitution-list oracle for small generated terms.
// No union-find, levels, arena or production worklists are reused.
#[derive(Default)]
struct Oracle(HashMap<usize, Type>);
impl Oracle {
    fn resolve(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => self
                .0
                .get(id)
                .map(|ty| self.resolve(ty))
                .unwrap_or(ty.clone()),
            Type::Nominal(name, args) => {
                nominal(name, args.iter().map(|ty| self.resolve(ty)).collect())
            }
            atom => atom.clone(),
        }
    }
    fn occurs(id: usize, ty: &Type) -> bool {
        match ty {
            Type::Var(other) => id == *other,
            Type::Nominal(_, args) => args.iter().any(|ty| Self::occurs(id, ty)),
            _ => false,
        }
    }
    fn unify(&mut self, left: &Type, right: &Type) -> bool {
        let left = self.resolve(left);
        let right = self.resolve(right);
        if left == right {
            return true;
        }
        match (left, right) {
            (Type::Var(id), ty) | (ty, Type::Var(id)) => {
                if Self::occurs(id, &ty) {
                    return false;
                }
                self.0.insert(id, ty);
                true
            }
            (Type::Nominal(a, aa), Type::Nominal(b, bb)) if a == b && aa.len() == bb.len() => {
                aa.iter().zip(bb).all(|(a, b)| self.unify(a, &b))
            }
            _ => false,
        }
    }
}

fn canonical(ty: Type, ids: &mut HashMap<usize, usize>) -> Type {
    match ty {
        Type::Var(id) => {
            let next = ids.len();
            Type::Var(*ids.entry(id).or_insert(next))
        }
        Type::Nominal(name, args) => nominal(
            &name,
            args.into_iter().map(|ty| canonical(ty, ids)).collect(),
        ),
        atom => atom,
    }
}
fn generated(seed: &mut u64, depth: usize) -> Type {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let choice = (*seed >> 32) % if depth == 0 { 8 } else { 12 };
    match choice {
        0..=4 => Type::Var(choice as usize),
        5 => Type::Int,
        6 => Type::Bool,
        7 => Type::Unit,
        8 => nominal("One", vec![generated(seed, depth - 1)]),
        9 => nominal("Other", vec![generated(seed, depth - 1)]),
        _ => nominal(
            "Pair",
            vec![generated(seed, depth - 1), generated(seed, depth - 1)],
        ),
    }
}

#[test]
fn generated_constraints_match_oracle_and_substitution_is_idempotent() {
    for seed in 0..2_000 {
        let mut rng = seed;
        let mut solver = Solver::default();
        for _ in 0..5 {
            solver.fresh(1);
        }
        let mut oracle = Oracle::default();
        let mut equations = Vec::new();
        for step in 0..12 {
            let left = generated(&mut rng, 3);
            let right = generated(&mut rng, 3);
            let expected = oracle.unify(&left, &right);
            let actual = solver.unify(left.clone(), right.clone()).is_ok();
            assert_eq!(
                actual, expected,
                "seed={seed}, step={step}: {left:?} = {right:?}"
            );
            if !actual {
                break;
            }
            equations.push((left, right));
            for (left, right) in &equations {
                assert_eq!(
                    solver.resolve(left).unwrap(),
                    solver.resolve(right).unwrap()
                );
            }
            let mut actual_ids = HashMap::new();
            let mut expected_ids = HashMap::new();
            for id in 0..5 {
                let actual = solver.resolve(&Type::Var(id)).unwrap();
                assert_eq!(solver.resolve(&actual).unwrap(), actual);
                assert_eq!(solver.substitute(&actual, &HashMap::new()).unwrap(), actual);
                assert_eq!(
                    canonical(actual, &mut actual_ids),
                    canonical(oracle.resolve(&Type::Var(id)), &mut expected_ids),
                    "seed={seed}, step={step}, variable={id}"
                );
            }
        }
    }
}
