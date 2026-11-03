use super::*;

const EXHAUSTED: &str = "test cursor budget exhausted";

type VariableState = (usize, u32, usize, usize, Option<usize>);
fn snapshot(solver: &Solver) -> (Vec<VariableState>, usize, usize) {
    (
        solver
            .variables
            .iter()
            .map(|v| (v.parent.get(), v.rank, v.level, v.term, v.binding))
            .collect(),
        solver.terms.len(),
        solver.interned.len(),
    )
}
fn spend(remaining: &mut usize) -> Result<(), String> {
    *remaining = remaining.checked_sub(1).ok_or(EXHAUSTED)?;
    Ok(())
}
fn variable(ty: &Type) -> usize {
    let Type::Var(id) = ty else {
        panic!("expected variable")
    };
    *id
}

#[test]
fn every_parent_binding_and_head_lookup_has_an_exact_charge() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(2);
    let c = solver.fresh(3);
    solver.unify(b.clone(), c).unwrap();
    solver.unify(a.clone(), b.clone()).unwrap();
    // A higher-ranked, later root replaces a's original identity.
    assert_eq!(solver.variables[variable(&a)].parent.get(), variable(&b));
    let before = snapshot(&solver);
    for limit in 0..4 {
        let mut remaining = limit;
        assert!(matches!(
            solver.type_head(TypeCursor::Public(&a), || spend(&mut remaining)),
            Err(error) if error == EXHAUSTED
        ));
        assert_eq!(remaining, 0);
        assert_eq!(snapshot(&solver), before);
    }
    let mut remaining = 4; // public head + two parents + root binding
    assert!(matches!(
        solver.type_head(TypeCursor::Public(&a), || spend(&mut remaining)),
        Ok(TypeHead::Variable(id)) if id == variable(&b)
    ));
    assert_eq!(remaining, 0);
    assert_eq!(snapshot(&solver), before);

    solver.unify(b, Type::Bool).unwrap();
    let before = snapshot(&solver);
    for cursor in [
        TypeCursor::Public(&a),
        TypeCursor::Private(solver.variables[variable(&a)].term),
    ] {
        for limit in 0..5 {
            let mut remaining = limit;
            assert!(
                matches!(solver.type_head(cursor, || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
            );
            assert_eq!(remaining, 0);
            assert_eq!(snapshot(&solver), before);
        }
        let mut remaining = 5; // public/private variable head + two parents + binding + atom
        assert!(matches!(
            solver.type_head(cursor, || spend(&mut remaining)),
            Ok(TypeHead::Atom(Type::Bool))
        ));
        assert_eq!(remaining, 0);
        assert_eq!(snapshot(&solver), before);
    }
}

#[test]
fn budget_failure_happens_before_even_an_invalid_lookup() {
    let solver = Solver::default();
    let invalid = Type::Var(usize::MAX);
    for (cursor, limit) in [
        (TypeCursor::Private(usize::MAX), 0),
        (TypeCursor::Public(&invalid), 0),
        (TypeCursor::Public(&invalid), 1),
    ] {
        let mut remaining = limit;
        assert!(
            matches!(solver.type_head(cursor, || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
        );
        assert_eq!(remaining, 0);
    }
    let nominal = Type::Nominal("Uninspected".into(), vec![invalid; 100_000]);
    let mut remaining = 1;
    let TypeHead::Nominal(name, children) = solver
        .type_head(TypeCursor::Public(&nominal), || spend(&mut remaining))
        .unwrap()
    else {
        panic!("expected nominal")
    };
    assert_eq!(name, "Uninspected");
    assert_eq!(children.len(), 100_000);
    assert_eq!(remaining, 0);
    assert!(children.get(100_000).is_none());
    let TypeChildren::Public(children) = children else {
        panic!("expected borrowed public children")
    };
    let Type::Nominal(_, original) = &nominal else {
        unreachable!()
    };
    assert!(std::ptr::eq(children.as_ptr(), original.as_ptr()));
}

#[test]
fn mixed_public_and_private_children_stay_lazy_and_ordered() {
    let mut solver = Solver::default();
    let open = solver.fresh(2);
    let alias = solver.fresh(2);
    solver.unify(open.clone(), alias.clone()).unwrap();
    let bound = solver.fresh(2);
    solver
        .unify(
            bound.clone(),
            Type::Nominal("Inner".into(), vec![alias, Type::Bool]),
        )
        .unwrap();
    let outer = Type::Nominal("Outer".into(), vec![bound, Type::Unit]);
    let before = snapshot(&solver);
    let mut remaining = 1;
    let TypeHead::Nominal("Outer", children) = solver
        .type_head(TypeCursor::Public(&outer), || spend(&mut remaining))
        .unwrap()
    else {
        panic!("outer head")
    };
    assert_eq!(remaining, 0);
    assert!(matches!(children, TypeChildren::Public(_)));
    assert_eq!(children.len(), 2);
    let mut remaining = 4;
    let TypeHead::Nominal("Inner", inner) = solver
        .type_head(children.get(0).unwrap(), || spend(&mut remaining))
        .unwrap()
    else {
        panic!("inner head")
    };
    assert_eq!(remaining, 0);
    assert!(matches!(inner, TypeChildren::Private(_)));
    assert_eq!(inner.len(), 2);
    let mut remaining = 3; // interned root variable, parent, binding
    assert!(
        matches!(solver.type_head(inner.get(0).unwrap(), || spend(&mut remaining)), Ok(TypeHead::Variable(id)) if id == variable(&open))
    );
    assert_eq!(remaining, 0);
    let mut remaining = 1;
    assert!(matches!(
        solver.type_head(inner.get(1).unwrap(), || spend(&mut remaining)),
        Ok(TypeHead::Atom(Type::Bool))
    ));
    assert_eq!(remaining, 0);
    let mut remaining = 1;
    assert!(matches!(
        solver.type_head(children.get(1).unwrap(), || spend(&mut remaining)),
        Ok(TypeHead::Atom(Type::Unit))
    ));
    assert_eq!(remaining, 0);
    assert!(inner.get(2).is_none());
    assert_eq!(snapshot(&solver), before);
}

#[test]
fn adverse_long_alias_walk_stops_at_the_budget_without_compression() {
    let mut solver = Solver::default();
    let count = 200_000;
    for _ in 0..count {
        solver.fresh(0);
    }
    // Corrupt the ranked shape only to reproduce the former hidden-hop attack.
    for id in 0..count - 1 {
        solver.variables[id].parent.set(id + 1);
    }
    let before = snapshot(&solver);
    let root = Type::Var(0);
    let mut remaining = 100_000;
    assert!(
        matches!(solver.type_head(TypeCursor::Public(&root), || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
    );
    assert_eq!(remaining, 0);
    assert_eq!(snapshot(&solver), before);
}

#[test]
fn repeated_rank_deep_aliases_pay_for_every_parent_on_every_visit() {
    let mut solver = Solver::default();
    let count = 1 << 12;
    let vars: Vec<_> = (0..count).map(|_| solver.fresh(0)).collect();
    let mut width = 1;
    while width < count {
        for start in (0..count).step_by(width * 2) {
            solver
                .unify(vars[start].clone(), vars[start + width].clone())
                .unwrap();
        }
        width *= 2;
    }
    solver.unify(vars[0].clone(), Type::Bool).unwrap();
    let before = snapshot(&solver);
    let cursor = TypeCursor::Public(&vars[count - 1]);
    let mut remaining = 16_000; // 1 head + 13 parents + 1 binding + 1 atom, repeated 1000 times
    for _ in 0..1_000 {
        assert!(matches!(
            solver.type_head(cursor, || spend(&mut remaining)),
            Ok(TypeHead::Atom(Type::Bool))
        ));
    }
    assert_eq!(remaining, 0);
    assert!(
        matches!(solver.type_head(cursor, || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
    );
    assert_eq!(snapshot(&solver), before);
}

#[test]
fn malformed_parent_and_binding_cycles_terminate_without_mutation() {
    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let b = solver.fresh(2);
    solver.variables[variable(&a)].parent.set(variable(&b));
    solver.variables[variable(&b)].parent.set(variable(&a));
    let before = snapshot(&solver);
    let mut remaining = 32;
    assert!(
        matches!(solver.type_head(TypeCursor::Public(&a), || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
    );
    assert_eq!(remaining, 0);
    assert_eq!(snapshot(&solver), before);

    let mut solver = Solver::default();
    let a = solver.fresh(1);
    let root = variable(&a);
    solver.variables[root].binding = Some(solver.variables[root].term);
    let before = snapshot(&solver);
    let mut remaining = 32;
    assert!(
        matches!(solver.type_head(TypeCursor::Public(&a), || spend(&mut remaining)), Err(error) if error == EXHAUSTED)
    );
    assert_eq!(remaining, 0);
    assert_eq!(snapshot(&solver), before);
}
