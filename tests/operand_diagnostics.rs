use rill::{backend, check, interpreter, parser};

#[test]
fn fixed_operand_constraints_report_direction_and_solved_type() {
    for (body, message) in [
        ("40 + false", "right operand of '+' expects i32, found bool"),
        (
            "true * false",
            "left operand of '*' expects i32, found bool",
        ),
        (
            "let n = 7; n || false",
            "left operand of '||' expects bool, found i32",
        ),
        ("1 < \"x\"", "right operand of '<' expects i32, found str"),
        (
            "let v = Some(true); v % 1",
            "left operand of '%' expects i32, found Option",
        ),
        ("-false", "operand of '-' expects i32, found bool"),
        (
            "let f = fn(x) { x }; true && f",
            "right operand of '&&' expects bool, found a function",
        ),
    ] {
        let program = parser::parse(&format!(
            "type Option<a> = None | Some(a)\nfn main() {{ {body} }}"
        ))
        .unwrap();
        let expected = format!("E-TYPE: {message}");
        assert_eq!(check::check(&program).unwrap_err(), expected);
        assert_eq!(interpreter::run(&program).unwrap_err(), expected);
        assert_eq!(backend::emit(&program).unwrap_err(), expected);
    }
}

#[test]
fn preserves_nested_inference_error_and_unconstrained_parameter_inference() {
    let program = parser::parse("fn main() { true + (missing * 2) }").unwrap();
    assert!(check::check(&program).unwrap_err().starts_with("E-NAME:"));
    let program = parser::parse("fn plus(x, y) { x + y } fn main() { plus(10, 32) }").unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
}
