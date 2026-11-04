//! Closed braced-function results constrain existing types, never layouts.
use rill::{ast::TypeExpr, backend, check, interpreter, parser, typed};

fn evaluate(source: &str, expected: &str) {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let lowered = typed::lower_main(&checked).unwrap();
    assert_eq!(typed::evaluate(&lowered).unwrap().to_string(), expected);
    assert_eq!(interpreter::run(&program).unwrap().to_string(), expected);
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        expected
    );
    let c = backend::emit(&program).unwrap();
    assert!(!c.contains("rl_value"));
    assert!(!c.contains("RL_CAP"));
    backend::emit_reference(&program).unwrap();
}

#[test]
fn closed_results_make_observed_and_mutable_values_concrete() {
    evaluate("type O<a> = None | Some(a)\nfn absent() -> O<i32> { None }\nfn main() -> i32 { let mut value = absent(); value <- Some(42); match value { None => 0 | Some(x) => x } }", "42");
    evaluate("type O<a> = None | Some(a)\nfn absent() -> O<i32> { None }\nfn main() -> O<i32> { absent() }", "None");
    evaluate(
        "fn text() -> str { \"héllo\" }\nfn main() -> i32 { byte_len(text()) }",
        "6",
    );
    evaluate("fn main() -> unit { 42; }", "()");
}

#[test]
fn all_recursive_result_constraints_precede_every_body() {
    let f = "fn f(n) -> i32 { let p = g(n); p.x }";
    let g = "fn g(n) -> P { if n == 0 { record P { x = 42 } } else { record P { x = f(n - 1) } } }";
    for declarations in [format!("{f}\n{g}"), format!("{g}\n{f}")] {
        evaluate(
            &format!("type P = {{ x: i32 }}\n{declarations}\nfn main() -> i32 {{ f(2) }}"),
            "42",
        );
    }
}

#[test]
fn closed_results_preserve_parameter_generalization_and_constrain_identity() {
    let program = parser::parse("fn count(x) -> i32 {42}\nfn identity(x) -> i32 {x}\nfn main() { count(true) + identity(count(1)) }").unwrap();
    let checked = check::analyze(&program).unwrap();
    assert_eq!(checked.functions["count"].quantified.len(), 1);
    assert!(checked.functions["identity"].quantified.is_empty());
    assert_eq!(checked.functions["identity"].params, vec![check::Type::Int]);
    evaluate(
        "fn count(x) -> i32 {42}\nfn main() { count(true) + count(1) }",
        "84",
    );
}

#[test]
fn invalid_closed_annotations_and_unused_body_mismatches_reject() {
    for (source, code) in [
        ("fn main() -> bool {42}", "E-TYPE"),
        ("fn main() -> i32 {42;}", "E-TYPE"),
        (
            "type O<a> = None | Some(a)\nfn bad() -> O<bool> {Some(1)}\nfn main(){42}",
            "E-TYPE",
        ),
        (
            "type O<a> = None | Some(a)\nfn main() -> O<i32,bool> {None}",
            "E-ARITY",
        ),
        ("fn identity(x) -> a {x}\nfn main(){42}", "E-TYPE"),
        ("fn main() -> _ {42}", "E-TYPE"),
    ] {
        let program = parser::parse(source).unwrap();
        assert!(
            check::check(&program).unwrap_err().starts_with(code),
            "{source}"
        );
    }
    for source in [
        "let main -> i32 = 42",
        "fn main() -> {42}",
        "fn main() -> i32 -> i32 {42}",
        "fn main<a>() -> a {42}",
        "fn main() -> forall a {42}",
        "fn main() -> i32 ! {} {42}",
        "fn main(){let x:i32=42;x}",
    ] {
        assert!(parser::parse(source).is_err(), "{source}");
        assert!(rill::syntax::SyntaxTree::parse(source)
            .parsed_module()
            .is_none());
    }
}

#[test]
fn parser_and_programmatic_annotation_depths_are_guarded_separately() {
    let source = format!(
        "fn main() -> {}i32{} {{ 42 }}",
        "O<".repeat(10_000),
        ">".repeat(10_000)
    );
    assert!(parser::parse(&source)
        .unwrap_err()
        .contains("E-PARSE-DEPTH"));
    let mut program = parser::parse("fn main(){42}").unwrap();
    let mut ty = TypeExpr {
        name: "i32".into(),
        args: vec![],
    };
    for _ in 0..10_000 {
        ty = TypeExpr {
            name: "O".into(),
            args: vec![ty],
        };
    }
    program.functions[0].result_annotation = Some(ty);
    for error in [
        check::check(&program).unwrap_err(),
        interpreter::run(&program).unwrap_err(),
        backend::emit(&program).unwrap_err(),
        backend::emit_reference(&program).unwrap_err(),
    ] {
        assert!(error.starts_with("E-TYPE-COMPLEXITY"), "{error}");
    }
    // The compiler borrows the caller's AST; the caller also avoids recursive
    // destruction of this deliberately malformed, arbitrarily deep test tree.
    let mut pending = vec![program.functions[0].result_annotation.take().unwrap()];
    while let Some(mut ty) = pending.pop() {
        pending.append(&mut ty.args);
    }
}

#[test]
fn annotations_do_not_generalize_mutable_aliases_or_remove_owner_restrictions() {
    let source = "type O<a> = None | Some(a)\nfn main() -> i32 { let mut slot = None; let alias = slot; slot <- Some(1); let bad = if true {alias} else {Some(true)}; 42 }";
    assert!(check::check(&parser::parse(source).unwrap())
        .unwrap_err()
        .starts_with("E-TYPE"));
    let source = "fn hidden() -> SystemBoxI32 { hidden() }\nfn main() -> i32 {42}";
    assert!(check::check(&parser::parse(source).unwrap())
        .unwrap_err()
        .contains("E-OWNED"));
}

#[path = "support/result_annotation_compatibility.rs"]
mod compatibility;
#[test]
fn unannotated_fingerprint_has_only_the_documented_projection_migration() {
    assert_eq!(
        compatibility::fingerprints(),
        include_str!("support/result_annotation_schemes.txt")
    );
}

#[test]
fn raw_c_pointee_owner_spelling_retains_the_parameter_boundary() {
    for name in ["SystemBytes", "SystemBoxI32"] {
        for pointer in ["ptr", "const_ptr"] {
            for source in [
                format!("fn inherited(x:{pointer}<{name}>) {{x}} fn main(){{42}}"),
                format!("fn inherited() -> {pointer}<{name}> {{c_null()}} fn main(){{42}}"),
            ] {
                let error = check::check(&parser::parse(&source).unwrap()).unwrap_err();
                assert!(error.starts_with("E-OWNED"), "{source}: {error}");
            }
        }
    }
}

#[test]
fn scheme_comparison_distinguishes_quantifier_membership_and_unused_variables() {
    let signature = |quantified| check::FunctionType {
        quantified,
        params: vec![check::Type::Var(7)],
        result: check::Type::Var(11),
    };
    let left = compatibility::canonical_signature(&signature(vec![7]));
    let right = compatibility::canonical_signature(&signature(vec![11]));
    assert_ne!(left, right);
    assert_eq!(left.0, vec![0]);
    assert_eq!(right.0, vec![1]);
    assert_eq!(
        compatibility::canonical_signature(&signature(vec![19])).0,
        vec![2]
    );
    assert_eq!(
        compatibility::canonical_signature(&signature(vec![])).0,
        Vec::<usize>::new()
    );
    let renamed = check::FunctionType {
        quantified: vec![101],
        params: vec![check::Type::Var(101)],
        result: check::Type::Var(99),
    };
    assert_eq!(left, compatibility::canonical_signature(&renamed));
}

#[test]
fn closed_annotations_do_not_relax_existing_type_and_layout_failures() {
    for (source, code) in [
        ("fn identity(x)->i32{x} fn main(){identity(true)}", "E-TYPE"),
        (
            "fn f(x)->i32{if true{f(true)}else{f(1)}} fn main(){42}",
            "E-TYPE",
        ),
        (
            "fn f()->i32{if true{42}else{false}} fn main(){42}",
            "E-TYPE",
        ),
        (
            "type O<a> = None|Some(a) fn f()->i32{match Some(1){None=>42}} fn main(){42}",
            "E-MATCH",
        ),
        (
            "fn f(n)->i32{g(n)} fn g(n)->bool{f(n)} fn main(){42}",
            "E-TYPE",
        ),
        ("fn f()->cstr{c\"no\"} fn main(){42}", "E-TYPE"),
        (
            "fn f()->u8x16<i32>{u8x16_load(bytes_new(16),0)} fn main(){42}",
            "E-TYPE",
        ),
    ] {
        let program = parser::parse(source).unwrap();
        let error = check::check(&program).unwrap_err();
        assert!(error.starts_with(code), "{source}: {error}");
    }
    // These unannotated storage requests still lack a real concrete type.
    for source in [
        "type O<a> = None|Some(a) fn absent(){None} fn main(){absent()}",
        "type O<a> = None|Some(a) fn main(){let mut out=None;42}",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        assert!(typed::lower_main(&checked).is_err(), "{source}");
    }
}
