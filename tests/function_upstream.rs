//! Callable conversion retains the bounded upstream checker and origin contracts.
use rill::{
    ast::{Expr, TypeExpr},
    check, functions, interpreter, modules, parser, typed,
};

fn agrees(source: &str, expected: &str) {
    let program = parser::parse(source).unwrap();
    check::check(&program).unwrap();
    let converted = functions::lower(&program).unwrap();
    assert!(converted
        .functions
        .iter()
        .all(|f| f.result_annotation.is_some()));
    assert_eq!(interpreter::run(&program).unwrap().to_string(), expected);
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        expected
    );
}

#[test]
fn closed_recursive_signatures_survive_callable_conversion() {
    agrees(
        "type Row = {value:i32}
        fn first(row) -> i32 { second(row) }
        fn second(row:Row) -> i32 { if row.value == 0 {42} else {first(record Row {value=0})} }
        fn main() -> i32 { let f=first; f(record Row {value=1}) }",
        "42",
    );
}

#[test]
fn callback_projections_resolve_before_component_generalization() {
    agrees(
        "type Row={value:i32}
        fn apply(f,x) {f(x)}
        fn main() {apply(fn(row){row.value},record Row {value=42})}",
        "42",
    );
}

#[test]
fn lambda_annotation_admission_precedes_recursive_type_consumers() {
    let mut program = parser::parse("fn main(){fn(x:i32){x}}").unwrap();
    let Expr::Lambda(params, _) = &mut program.functions[0].body else {
        panic!()
    };
    let mut annotation = TypeExpr {
        name: "unit".into(),
        args: vec![],
    };
    for _ in 0..128 {
        annotation = TypeExpr {
            name: "W".into(),
            args: vec![annotation],
        };
    }
    params[0].annotation = Some(annotation);
    assert!(check::analyze(&program)
        .err()
        .unwrap()
        .starts_with("E-TYPE-COMPLEXITY:"));
}

#[test]
fn callable_origins_reject_without_attaching_generated_source_paths() {
    let root = std::env::temp_dir().join(format!("rill-callable-origins-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("main.rill");
    std::fs::write(&path, "fn main(){let f=fn(x){x+1};f(41)}").unwrap();
    let sources = modules::compile::load_program_with_sources(&path).unwrap();
    assert!(typed::provenance::lower_main_with_function_origins(sources)
        .err()
        .unwrap()
        .starts_with("E-TYPED-UNSUPPORTED: callable"));
    let sources = modules::compile::load_program_with_sources(&path).unwrap();
    assert!(
        typed::provenance::lower_main_with_copy_node_origins(sources)
            .err()
            .unwrap()
            .starts_with("E-TYPED-UNSUPPORTED: callable")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_phantom_captures_keep_an_explicit_conversion_limit() {
    let program = parser::parse(
        "type Tag<a> = Tag fn tag(){Tag}
        fn main(){let value=tag();let f=fn(){value};f();42}",
    )
    .unwrap();
    check::check(&program).unwrap();
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        "42"
    );
    assert!(functions::lower(&program)
        .err()
        .unwrap()
        .starts_with("E-FUNCTION-LAYOUT:"));
    let pure =
        parser::parse("type Tag<a> = Tag fn tag(){Tag} fn main(){let value=tag();42}").unwrap();
    let checked = check::analyze(&pure).unwrap();
    typed::lower_main(&checked).unwrap();
}

#[test]
fn nonreturning_callable_programs_never_default_the_result_type() {
    let program =
        parser::parse("fn forever(){forever()} fn main(){let f=fn(x){x};forever();f(42)}").unwrap();
    check::check(&program).unwrap();
    assert!(functions::lower(&program)
        .err()
        .unwrap()
        .starts_with("E-FUNCTION-LAYOUT:"));
    let pure = parser::parse("fn forever(){forever()} fn main(){forever();42}").unwrap();
    let checked = check::analyze(&pure).unwrap();
    typed::lower_main(&checked).unwrap();
}
