use rill::{check, parser};

fn check_source(source: &str) -> Result<(), String> {
    check::check(&parser::parse(source)?)
}

#[test]
fn dependencies_are_generalized_before_their_callers() {
    let source = "let main = let a = id(1) in id(true)\nlet id x = x";
    check_source(source).unwrap();
}

#[test]
fn recursive_group_is_monomorphic() {
    let source = "let f x = g(x)\nlet g x = if true then f(1) else f(false)\nlet main = 0";
    assert!(check_source(source).unwrap_err().contains("E-TYPE"));
}

#[test]
fn disconnected_functions_are_still_checked() {
    let source = "let main = 0\nlet unused = missing()";
    assert!(check_source(source).unwrap_err().contains("E-NAME"));
}

#[test]
fn nested_dependencies_in_constructor_arguments_are_checked() {
    let source =
        "type Wrap<a> = Wrap(a)\nlet main = let a = Wrap(id(1)) in Wrap(id(true))\nlet id x = x";
    check_source(source).unwrap();
}
