use std::{fs, process::Command};

fn conformance(source: &str, expected: &str, case: &str) {
    let program = rill::parser::parse(source).unwrap_or_else(|e| panic!("{case}: {e}"));
    rill::check::analyze(&program).unwrap_or_else(|e| panic!("{case}: {e}"));
    assert_eq!(
        rill::interpreter::run(&program)
            .unwrap_or_else(|e| panic!("{case}: {e}"))
            .to_string(),
        expected,
        "{case}"
    );
    assert_eq!(
        rill::interpreter::run_reference(&program)
            .unwrap_or_else(|e| panic!("{case}: {e}"))
            .to_string(),
        expected,
        "reference: {case}"
    );
    let c = rill::backend::emit(&program).unwrap_or_else(|e| panic!("{case}: {e}"));
    for allocator in ["malloc", "calloc", "realloc", "rl_allocate"] {
        assert!(
            !c.contains(allocator),
            "{case}: unexpected allocation in closure C output"
        );
    }
    let directory =
        std::env::temp_dir().join(format!("rill-functions-{}-{case}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let source_file = directory.join("program.c");
    let executable = directory.join("program");
    fs::write(&source_file, c).unwrap();
    for optimization in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&source_file)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{expected}\n"),
            "{case}, {optimization}"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn higher_order_returned_closures_and_composition() {
    conformance(
        r#"
        fn add(x, y) { x + y }
        fn twice(f, x) { f(f(x)) }
        fn make_adder(x) { fn(y) { x + y } }
        fn main() {
            let increment = add(1);
            let add_ten = make_adder(10);
            let pipeline = increment >> add_ten;
            twice(pipeline, 10)
        }
    "#,
        "32",
        "composition",
    );
}

#[test]
fn generic_callback_can_return_partial_application() {
    conformance(
        r#"
        fn apply(f, x) { f(x) }
        fn add(x, y) { x + y }
        fn main() { let increment = apply(add, 1); increment(41) }
    "#,
        "42",
        "generic-partial",
    );
}

#[test]
fn named_functions_and_identity_lambdas_generalize() {
    conformance(
        r#"
        fn named(x) { x }
        fn main() {
            let id = fn(x) { x };
            let alias = named;
            if id(true) && alias(true) { id(20) + alias(22) } else { 0 }
        }
    "#,
        "42",
        "polymorphic-values",
    );
}

#[test]
fn nested_escaping_closures_keep_concrete_captures() {
    conformance(
        r#"
        fn outer(x) { fn(y) { fn(z) { x + y + z } } }
        fn main() { outer(10)(20)(12) }
    "#,
        "42",
        "nested",
    );
}

#[test]
fn application_and_pipeline_operator_forms() {
    conformance(
        r#"
        fn add(x, y) { x + y }
        fn main() {
            let plus = add 20;
            let twice = fn(x) { x * 2 };
            let backward = plus << twice;
            (1 |> backward) + (plus <| 0)
        }
    "#,
        "42",
        "operators",
    );
}

#[test]
fn partial_arguments_evaluate_once_at_construction() {
    conformance(
        r#"
        fn tick(inout n) { n <- n + 1; n }
        fn add(x, y) { x + y }
        fn main() {
            let mut n = 0;
            let f = add(tick(inout n));
            f(20) + f(20) + n
        }
    "#,
        "43",
        "once",
    );
}

#[test]
fn saturated_calls_and_returned_closures_preserve_effect_order() {
    conformance(
        r#"
        fn tick(inout n) { n <- n + 1; n }
        fn make(inout n) { n <- n * 10; fn(x) { x } }
        fn main() {
            let mut n = 1;
            let answer = make(inout n)(tick(inout n));
            answer + n
        }
    "#,
        "22",
        "staged-order",
    );
}

#[test]
fn lexical_function_shadowing_and_callable_records() {
    conformance(
        r#"
        type Holder<a> = { callback: a }
        fn apply(x) { x + 100 }
        fn main() {
            let apply = fn(x) { x + 1 };
            let holder = record Holder { callback = apply };
            holder.callback(41)
        }
    "#,
        "42",
        "record",
    );
}

#[test]
fn byte_buffer_capture_has_inline_layout() {
    conformance(
        r#"
        fn make() {
            let buffer = bytes_set(bytes_new(1), 0, 42);
            fn() { bytes_get(buffer, 0) }
        }
        fn main() { make()() }
    "#,
        "42",
        "bytes",
    );
}

#[test]
fn forward_pipeline_evaluates_input_before_function_expression() {
    conformance(
        r#"
        fn tick(inout n) { n <- n + 1; n }
        fn choose(inout n) {
            n <- n * 10;
            let snapshot = n;
            fn(x) { x + snapshot }
        }
        fn main() {
            let mut n = 1;
            let answer = tick(inout n) |> choose(inout n);
            answer + n
        }
    "#,
        "42",
        "pipeline-order",
    );
}

#[test]
fn callable_main_result_has_an_explicit_diagnostic() {
    let program = rill::parser::parse("fn main() { fn(x) { x + 1 } }").unwrap();
    let error = rill::interpreter::run(&program).unwrap_err();
    assert!(error.contains("E-FUNCTION-RESULT"), "{error}");
}

#[test]
fn captured_multi_parameter_lambda_can_be_partially_applied() {
    conformance(
        r#"
        fn make(a) { fn(b, c) { a + b + c } }
        fn main() { let partial = make(10)(20); partial(12) }
    "#,
        "42",
        "captured-partial",
    );
}

#[test]
fn generic_nested_closures_capture_callback_representations() {
    conformance(
        r#"
        fn add(x, y) { x + y }
        fn curry(f) { fn(x) { fn(y) { f(x, y) } } }
        fn main() { curry(add)(20)(22) }
    "#,
        "42",
        "generic-nested",
    );
}

#[test]
fn recursive_higher_order_instances_reuse_their_identity() {
    conformance(
        r#"
        fn repeat(f, n) { if n == 0 { 0 } else { f(repeat(f, n - 1)) } }
        fn main() { repeat(fn(x) { x + 6 }, 7) }
    "#,
        "42",
        "recursive",
    );
}

#[test]
fn unused_callable_signatures_need_no_arbitrary_type_default() {
    conformance(
        r#"
        fn ignore(f) { 42 }
        fn identity(x) { x }
        fn make() { fn(x) { x } }
        fn main() { make(); ignore(identity); ignore(fn(x) { x }) }
    "#,
        "42",
        "phantom-signature",
    );
}

#[test]
fn copying_a_closure_keeps_its_environment_snapshot() {
    conformance(
        r#"
        fn make(n) { fn(x) { x + n } }
        fn main() {
            let mut current = make(1);
            let snapshot = current;
            current <- make(2);
            snapshot(20) + current(19)
        }
    "#,
        "42",
        "environment-snapshot",
    );
}

#[test]
fn polymorphic_closure_captures_use_lexical_binding_identity() {
    conformance(
        r#"
        fn main() {
            let x = 1;
            let f = fn(y) { x };
            let x = 2;
            f(true) + f(3)
        }
    "#,
        "2",
        "polymorphic-shadow",
    );
}

#[test]
fn polymorphic_aliases_keep_parameter_and_pattern_captures() {
    conformance(
        r#"
        type Option<a> = None | Some(a)
        fn from_parameter(x) {
            let f = fn(y) { x };
            let alias = f;
            let x = 99;
            alias(true) + alias(3)
        }
        fn main() {
            match Some(10) {
                Some(x) => {
                    let f = fn(y) { x };
                    let alias = f;
                    let x = 99;
                    alias(true) + alias(3) + from_parameter(11)
                }
                | None => 0
            }
        }
    "#,
        "42",
        "polymorphic-alias-shadow",
    );
}

#[test]
fn polymorphic_record_capture_projection_survives_shadowing() {
    conformance(
        r#"
        type Point = { x: i32 }
        fn main() {
            let point = record Point { x = 21 };
            let f = fn(y) { point.x };
            let point = record Point { x = 99 };
            f(true) + f(0)
        }
    "#,
        "42",
        "record-capture-shadow",
    );
}

#[test]
fn partial_and_composed_generic_closures_keep_original_captures() {
    conformance(
        r#"
        fn main() {
            let x = 21;
            let f = fn(a, b) { x };
            let alias = f;
            let x = 99;
            let partial = alias(true);
            let identity = fn(value) { value };
            let composed = partial >> identity;
            composed(0) + alias(2)(false)
        }
    "#,
        "42",
        "generic-partial-shadow",
    );
}

#[test]
fn unused_weak_callable_initializer_retains_its_effects() {
    conformance(
        r#"
        fn make(inout n) { n <- n + 1; fn(x) { 0 } }
        fn main() {
            let mut n = 0;
            let unused = make(inout n);
            n
        }
    "#,
        "1",
        "unused-effectful-closure",
    );
}

#[test]
fn constructor_calls_keep_priority_over_same_named_locals() {
    conformance(
        r#"
        type Option<a> = None | Some(a)
        fn main() {
            let Some = fn(x) { x + 1 };
            match Some(41) { Some(x) => x | None => 0 }
        }
    "#,
        "41",
        "constructor-priority",
    );
}

#[test]
fn generalized_record_callable_fields_share_one_environment_layout() {
    conformance(
        r#"
        type Holder<a> = { callback: a }
        fn main() {
            let h = record Holder { callback = fn(x) { x } };
            if h.callback(true) { h.callback(42) } else { 0 }
        }
    "#,
        "42",
        "polymorphic-record-callable",
    );
}

#[test]
fn generalized_enum_callable_fields_keep_one_initializer() {
    conformance(
        r#"
        type Holder<a> = Wrap(a)
        fn main() {
            let h = Wrap(fn(x) { x });
            let f = match h { Wrap(g) => g };
            f(42)
        }
    "#,
        "42",
        "polymorphic-enum-callable",
    );
}

#[test]
fn polymorphic_aggregate_callbacks_return_distinct_concrete_captures() {
    conformance(
        r#"
        type Holder<a> = { callback: a }
        fn main() {
            let h = record Holder { callback = fn(x) { fn(y) { x } } };
            let boolean = h.callback(true);
            let integer = h.callback(42);
            if boolean(0) { integer(false) } else { 0 }
        }
    "#,
        "42",
        "polymorphic-return-layout",
    );
}

#[test]
fn captured_callable_signatures_do_not_change_physical_field_layout() {
    conformance(
        r#"
        fn identity(x) { x }
        fn wrap(f) { fn(x) { f(x) } }
        fn main() {
            let boolean = wrap(identity);
            let integer = wrap(identity);
            if boolean(true) { integer(42) } else { 0 }
        }
    "#,
        "42",
        "nested-callable-layout",
    );
}

#[test]
fn generalized_aggregate_closures_keep_once_evaluated_capture_snapshots() {
    conformance(
        r#"
        type Holder<a> = { callback: a }
        fn tick(inout n) { n <- n + 1; n }
        fn main() {
            let mut n = 20;
            let captured = tick(inout n);
            let h = record Holder { callback = fn(x) { captured } };
            let captured = 99;
            h.callback(true) + h.callback(0) + n
        }
    "#,
        "63",
        "aggregate-capture-once",
    );
}
