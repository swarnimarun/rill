use rill::{
    interpreter::{run_reference, Value},
    parser,
};

fn reference(source: &str) -> Result<Value, String> {
    run_reference(&parser::parse(source).unwrap())
}

#[test]
fn nested_lambdas_keep_lexical_values_under_shadowing() {
    let source = r#"
        type Option<a> = Some(a) | None
        fn make(x) { fn(y) { match Some(y) { Some(x) => x + 1 | None => x } } }
        fn main() {
            let x = 100;
            let make = fn(x) { fn(y) { x + y } };
            let add = make(20);
            add(22)
        }
    "#;
    assert_eq!(reference(source), Ok(Value::Int(42)));
    assert_eq!(reference("type Option<a> = Some(a) | None fn make(x) { fn(y) { match Some(y) { Some(x) => x + 1 | None => x } } } fn main(){make(100)(41)}"), Ok(Value::Int(42)));
}

#[test]
fn partial_closures_capture_the_prefix_once() {
    assert_eq!(
        reference(
            r#"
        fn tick(inout n) { n <- n + 1; n }
        fn make(a) { fn(b, c) { a + b + c } }
        fn main() {
            let mut n = 0;
            let partial = make(10)(tick(inout n));
            partial(20) + partial(9) + n
        }
    "#
        ),
        Ok(Value::Int(52))
    );
}

#[test]
fn expression_callee_and_each_saturated_body_run_before_later_arguments() {
    assert_eq!(
        reference(
            r#"
        fn tick(inout n) { n <- n + 1; n }
        fn choose(inout n) { n <- n * 10; fn(x) { x + 1 } }
        fn main() {
            let mut n = 1;
            let value = (choose(inout n))(tick(inout n));
            value + n
        }
    "#
        ),
        Ok(Value::Int(23))
    );
    assert_eq!(
        reference(
            r#"
        fn tick(inout n) { n <- n + 1; n }
        fn choose(inout n) { n <- n * 10; fn(x) { fn(y) { x + y } } }
        fn main() {
            let mut n = 1;
            let value = choose(inout n)(tick(inout n), tick(inout n));
            value + n
        }
    "#
        ),
        Ok(Value::Int(35))
    );
}

#[test]
fn pipelines_and_composition_evaluate_source_operands_once_in_order() {
    assert_eq!(
        reference(
            r#"
        fn tick(inout n) { n <- n + 1; n }
        fn choose(inout n) { n <- n * 10; let snapshot = n; fn(x) { x + snapshot } }
        fn main() { let mut n = 1; let result = choose(inout n) <| tick(inout n); result + n }
    "#
        ),
        Ok(Value::Int(32))
    );
    assert_eq!(
        reference(
            r#"
        fn choose(inout n) { n <- n * 10 + 1; let snapshot = n; fn(x) { x + snapshot } }
        fn main() {
            let mut n = 0;
            let composed = choose(inout n) << choose(inout n);
            composed(0) + composed(1) + n
        }
    "#
        ),
        Ok(Value::Int(36))
    );
}

#[test]
fn callable_record_fields_and_named_aliases_use_lexical_scope() {
    assert_eq!(
        reference(
            r#"
        type Holder<a> = { callback: a }
        fn add(x, y) { x + y }
        fn main() {
            let add = fn(x) { x + 2 };
            let holder = record Holder { callback = add };
            holder.callback(40)
        }
    "#
        ),
        Ok(Value::Int(42))
    );
}

#[test]
fn named_values_have_the_same_source_call_guard_as_direct_calls() {
    for call in ["down(n - 1)", "{ let again = down; again(n - 1) }"] {
        for (depth, expected) in [(30, Ok(Value::Int(42))), (31, Err("runtime trap".into()))] {
            let source = format!(
                "fn down(n){{if n == 0 {{42}} else {{{call}}}}} fn main(){{down({depth})}}"
            );
            assert_eq!(reference(&source), expected, "{source}");
        }
    }
}

#[test]
fn reference_rejects_callable_results_and_preserves_owner_boundary() {
    assert!(reference("fn main(){fn(x){x}}")
        .unwrap_err()
        .contains("E-FUNCTION-RESULT"));
    let error =
        reference("fn main(){let owner=system_box_i32_abort(42);box_read(owner)}").unwrap_err();
    assert!(error.contains("E-REFERENCE"), "{error}");
}
