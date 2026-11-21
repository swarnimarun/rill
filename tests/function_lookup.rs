use rill::{
    ast::{Expr, Function},
    functions, parser,
};

#[test]
fn callable_detection_preserves_direct_partial_unknown_and_duplicate_arity_cases() {
    for (source, expected) in [
        ("fn f(x) { x } fn main() { f(42) }", false),
        ("fn f(x, y) { x + y } fn main() { f(42) }", true),
        ("fn f(x) { x } fn main() { f }", true),
        ("fn main() { unknown(42) }", false),
        ("fn f(x) { x } fn f(y) { y } fn main() { f(42) }", false),
        ("fn f(x) { x } fn f() { 42 } fn main() { f(42) }", true),
        ("fn f() { 42 } fn f(x) { x } fn main() { f(42) }", true),
        (
            "fn f() { 42 } fn f(x) { x } fn f(x) { x } fn main() { f(42) }",
            true,
        ),
    ] {
        let program = parser::parse(source).unwrap();
        assert_eq!(functions::needs_lowering(&program), expected, "{source}");
        if program.functions.iter().filter(|f| f.name == "f").count() > 1 {
            assert!(rill::backend::emit(&program)
                .unwrap_err()
                .contains("E-NAME"));
        }
    }
}

#[test]
fn large_unused_library_stays_checked_while_only_main_is_executed() {
    let mut program = parser::parse("fn main() { 42 }").unwrap();
    for index in 0..4096 {
        program.functions.push(Function {
            name: format!("function{index:05}"),
            params: vec![],
            result_annotation: None,
            body: Expr::Call("function04095".into(), vec![]),
        });
    }
    program.functions.last_mut().unwrap().body = Expr::Int(7);
    assert!(!functions::needs_lowering(&program));
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
    rill::backend::emit(&program).unwrap();
    program.functions.last_mut().unwrap().body = Expr::Call("missing".into(), vec![]);
    assert!(rill::backend::emit(&program)
        .unwrap_err()
        .contains("E-NAME"));
}
