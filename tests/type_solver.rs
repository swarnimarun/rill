use rill::{check, parser};
use std::collections::{BTreeMap, HashMap};

fn check_source(source: &str) -> Result<(), String> {
    check::check(&parser::parse(source)?)
}

#[test]
fn outer_connected_variables_and_mutable_aliases_remain_monomorphic() {
    for source in [
        "type Option<a> = None | Some(a)\nfn bad(x) { let alias = x; let one = if true { alias } else { Some(1) }; if true { x } else { Some(true) } }\nfn main() { 0 }",
        "type Option<a> = None | Some(a)\nfn main() { let mut slot = None; let alias = slot; slot <- Some(1); if true { alias } else { Some(true) } }",
        "type Option<a> = None | Some(a)\nfn main() { let mut slot = None; let nested = Some(slot); slot <- Some(1); if true { nested } else { Some(Some(true)) } }",
    ] { let error = check_source(source).unwrap_err(); assert!(error.starts_with("E-TYPE:"), "{error}"); }
}

#[test]
fn independent_instantiation_preserves_repeated_occurrences() {
    check_source("type Pair<a,b> = Pair(a,b)\nfn duplicate(x) { Pair(x,x) }\nfn main() { let a = duplicate(1); duplicate(true) }").unwrap();
    let error = check_source("type Pair<a,b> = Pair(a,b)\nfn duplicate(x) { Pair(x,x) }\nfn main() { if true { duplicate(1) } else { Pair(1,true) } }").unwrap_err();
    assert!(error.starts_with("E-TYPE:"), "{error}");
}

#[test]
fn source_infinite_types_and_nominal_mismatches_are_rejected() {
    for source in [
        "type Wrap<a> = Wrap(a)\nfn bad(x) { if true { x } else { Wrap(x) } }\nfn main() { 0 }",
        "type Wrap<a> = Wrap(a)\nfn f(x) { g(Wrap(x)) }\nfn g(x) { f(x) }\nfn main() { 0 }",
        "type A<a> = A(a)\ntype B<a> = B(a)\nfn main() { if true { A(1) } else { B(1) } }",
    ] {
        let error = check_source(source).unwrap_err();
        assert!(error.starts_with("E-TYPE:"), "{error}");
    }
    let error = check_source("type A<a> = A(a)\nfn bad(x: A<i32,bool>) { x }\nfn main() { 0 }")
        .unwrap_err();
    assert!(error.starts_with("E-ARITY:"), "{error}");
}

fn canonical(ty: &check::Type, ids: &mut HashMap<usize, usize>) -> check::Type {
    match ty {
        check::Type::Var(id) => {
            let next = ids.len();
            check::Type::Var(*ids.entry(*id).or_insert(next))
        }
        check::Type::Nominal(name, args) => check::Type::Nominal(
            name.clone(),
            args.iter().map(|ty| canonical(ty, ids)).collect(),
        ),
        atom => atom.clone(),
    }
}
fn signatures(source: &str) -> BTreeMap<String, (usize, Vec<check::Type>, check::Type)> {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    checked
        .functions
        .into_iter()
        .map(|(name, signature)| {
            assert!(signature
                .quantified
                .windows(2)
                .all(|pair| pair[0] < pair[1]));
            let mut ids = HashMap::new();
            let params = signature
                .params
                .iter()
                .map(|ty| canonical(ty, &mut ids))
                .collect();
            let result = canonical(&signature.result, &mut ids);
            (name, (signature.quantified.len(), params, result))
        })
        .collect()
}

#[test]
fn declaration_permutations_preserve_schemes_modulo_alpha_renaming() {
    let declarations = [
        "fn identity(x) { x }",
        "fn pair(x,y) { Pair(identity(x),identity(y)) }",
        "fn recursive(x) { other(x) }",
        "fn other(x) { if true { recursive(x) } else { identity(x) } }",
        "fn main() { let a = pair(1,true); pair(false,2) }",
    ];
    let prefix = "type Pair<a,b> = Pair(a,b)\n";
    let expected = signatures(&format!("{prefix}{}", declarations.join("\n")));
    fn permutations(order: &mut [usize], start: usize, visit: &mut impl FnMut(&[usize])) {
        if start == order.len() {
            visit(order);
            return;
        }
        for next in start..order.len() {
            order.swap(start, next);
            permutations(order, start + 1, visit);
            order.swap(start, next);
        }
    }
    permutations(&mut [0, 1, 2, 3, 4], 0, &mut |order| {
        let source = format!(
            "{prefix}{}",
            order
                .iter()
                .map(|i| declarations[*i])
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(signatures(&source), expected, "order={order:?}");
    });
}

#[test]
fn wide_shallow_constraints_do_not_create_variable_link_recursion() {
    let count = 4_000;
    let parameters = (0..=count)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let fields = (0..count)
        .map(|i| format!("a{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let arguments = (0..count)
        .map(|i| format!("if true {{ x{i} }} else {{ x{} }}", i + 1))
        .collect::<Vec<_>>()
        .join(",");
    check_source(&format!("type Tuple<{fields}> = Tuple({fields})\nfn chain({parameters}) {{ Tuple({arguments}) }}\nfn main() {{ 42 }}")).unwrap();
}

#[test]
fn local_scheme_instantiations_remain_independent() {
    check_source("type Option<a> = None | Some(a)\nfn main() { let none = None; let a = if true { Some(1) } else { none }; if true { Some(true) } else { none } }").unwrap();
}

#[test]
fn many_independent_schemes_do_not_change_the_ambient_environment() {
    let mut source = (0..2_000)
        .map(|i| format!("fn identity{i}(x) {{ x }}\n"))
        .collect::<String>();
    source.push_str("fn main() { identity0(42) }");
    check_source(&source).unwrap();
}

#[test]
fn pending_raw_pointer_obligations_are_not_generalized() {
    check_source("extern fn consume(x: ptr<Opaque>) -> unit;\nfn null() { c_null() }\nfn main() { let p = null(); consume(p) }").unwrap();
    let error = check_source("extern fn first(x: ptr<One>) -> unit;\nextern fn second(x: ptr<Two>) -> unit;\nfn null() { c_null() }\nfn main() { first(null()); second(null()) }").unwrap_err();
    assert!(error.starts_with("E-TYPE:"), "{error}");
}

#[test]
fn wide_independent_parameters_keep_sorted_schemes() {
    let count = 20_000;
    let params = (0..count)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(",");
    // The local let also exercises filtering an earlier wide generic scheme
    // from the local environment, not only SCC-boundary collection.
    let source = format!("fn wide({params}) {{ 0 }}\nfn main() {{ let ignored = 0; 42 }}");
    let program = parser::parse(&source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let signature = &checked.functions["wide"];
    assert_eq!(signature.quantified.len(), count);
    assert!(signature
        .quantified
        .windows(2)
        .all(|pair| pair[0] < pair[1]));
    assert!(signature.params.iter().all(|ty| match ty {
        check::Type::Var(id) => signature.quantified.binary_search(id).is_ok(),
        _ => false,
    }));
}
