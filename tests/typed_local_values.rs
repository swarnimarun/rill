use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: Option<&str>) {
    let program = rill::parser::parse(source).unwrap();
    compare_program(&program, source, expected);
}

fn compare_program(program: &rill::ast::Program, source: &str, expected: Option<&str>) {
    let checked = rill::check::analyze(program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    let reference = rill::interpreter::run_reference(program);
    match expected {
        Some(expected) => assert_eq!(
            reference.as_ref().unwrap().to_string(),
            expected,
            "{source}"
        ),
        None => assert!(reference.is_err(), "{source}"),
    }
    assert_eq!(rill::typed::evaluate(&typed), reference, "{source}");
    assert_eq!(rill::interpreter::run(program), reference, "{source}");
    let c = rill::typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(program).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    let dir = std::env::temp_dir().join(format!(
        "rill-typed-local-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let source_path = dir.join("main.c");
    let binary = dir.join("main");
    fs::write(&source_path, c).unwrap();
    for opt in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", "-pedantic-errors", opt])
            .arg(&source_path)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        match expected {
            Some(expected) => {
                assert!(
                    output.status.success(),
                    "{source}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    output.stdout,
                    format!("{expected}\n").as_bytes(),
                    "{source}"
                );
                assert!(output.stderr.is_empty());
            }
            None => {
                assert!(!output.status.success(), "{source}");
                assert_eq!(output.stderr, b"rill: runtime trap\n", "{source}");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

const OPTION: &str = "type Option<a> = None | Some(a)\nfn integer(x: Option<i32>) { match x { None => 40 | Some(n) => n } }\nfn boolean(x: Option<bool>) { match x { None => true | Some(b) => b } }\n";

#[test]
fn immutable_constructors_and_aliases_materialize_distinct_use_types() {
    for bindings in [
        "let n = None;",
        "let first = None; let second = first; let n = second;",
        "let n = None();",
        "let None = 7; let n = None();",
    ] {
        compare(&format!("{OPTION}fn main() {{ {bindings} if boolean(n) {{ integer(n) + 2 }} else {{ 0 }} }}"), Some("42"));
    }
    compare("type O<a> = No | Yes(a)\nfn ints(x: O<O<i32>>) { match x { Yes(No) => 40 | _ => 0 } }\nfn bools(x: O<O<bool>>) { match x { Yes(No) => 2 | _ => 0 } }\nfn main() { let x = No; let n = Yes(x); let alias = n; ints(alias) + bools(alias) }", Some("42"));
}

#[test]
fn whole_record_and_field_uses_are_independent_and_consistent() {
    let prefix = format!("{OPTION}type R<a> = {{ value: Option<a>; fixed: i32 }}\nfn whole(r: R<bool>) {{ if boolean(r.value) {{ r.fixed }} else {{ 0 }} }}\n");
    for body in [
        "let r = record R { value = None; fixed = 2 }; integer(r.value) + whole(r)",
        "let r = record R { fixed = 2; value = None }; let alias = r; let a = alias.value; let b = alias.value; if boolean(b) { integer(a) + whole(alias) } else { 0 }",
        "let r = record R { fixed = 2; value = None }; let unused = r; if boolean(r.value) { integer(r.value) + r.fixed } else { 0 }",
    ] { compare(&format!("{prefix}fn main() {{ {body} }}"), Some("42")); }
    compare(&format!("{OPTION}type Inner<a> = {{ value: Option<a>; fixed: i32 }}\ntype Outer<a,b> = {{ inner: Inner<a>; phantom: Option<b> }}\nfn main() {{ let r = record Outer {{ phantom = None; inner = record Inner {{ fixed = 2; value = None }} }}; if boolean(r.inner.value) {{ integer(r.inner.value) + r.inner.fixed }} else {{ 0 }} }}"), Some("42"));
}

#[test]
fn snapshots_happen_at_the_original_binding_and_never_repeat_computation() {
    compare("type Mark<a> = Mark(i32)\nfn ints(x: Mark<i32>) { match x { Mark(n) => n } }\nfn bools(x: Mark<bool>) { match x { Mark(n) => n } }\nfn main() { let mut count = 0; let snapshot = { count <- count + 1; count }; let p = Mark(snapshot); let alias = p; count <- 20; ints(alias) + bools(p) + count * 2 }", Some("42"));
    compare(&format!("{OPTION}type M<a,b> = M(str,b,Option<a>)\nfn get(b) {{ let known = if true {{ b }} else {{ bytes_new(2) }}; bytes_get(known,0) }}\nfn ints(x) {{ match x {{ M(s,b,t) => byte_len(s) + get(b) + integer(t) * 0 }} }}\nfn bools(x) {{ match x {{ M(s,b,t) => if boolean(t) {{ byte_len(s) + get(b) }} else {{ 0 }} }} }}\nfn main() {{ let mut storage = bytes_new(2); storage <- bytes_set(storage,0,19); let snapshot = storage; let p = M(\"é\", snapshot, None); storage <- bytes_set(storage,0,255); ints(p) + bools(p) }}"), Some("42"));
    compare("type Cell = { n: i32 }\ntype R<a> = { fixed: Cell; marker: a }\ntype O<a> = None | Some(a)\nfn main() { let mut cell = record Cell { n = 42 }; let snapshot = cell; let p = record R { fixed = snapshot; marker = None }; cell.n <- 0; p.fixed.n }", Some("42"));
}

#[test]
fn unused_and_phantom_shapes_do_not_invent_runtime_type_arguments() {
    for body in ["let x = None; 42", "let x = Some(None); let y = x; 42"] {
        compare(
            &format!("type O<a> = None | Some(a)\nfn main() {{ {body} }}"),
            Some("42"),
        );
    }
    compare("type P<a> = P\ntype R<a,b> = { fixed: i32; marker: P<a>; unused: P<b> }\nfn main() { let x = record R { unused = P; fixed = 42; marker = P }; x.fixed }", Some("42"));
    compare("type Mark<a> = Mark(i32)\nfn main() { let mut n = 0; let snapshot = { n <- n + 1; n }; let unused = Mark(snapshot); n + 41 }", Some("42"));
}

#[test]
fn generic_function_instances_and_shadowing_keep_capture_identities() {
    compare("type O<a> = None | Some(a)\ntype P<a,b> = P(a,O<b>)\nfn first(x: P<i32,i32>) { match x { P(n,_) => n } }\nfn second(x: P<i32,bool>) { match x { P(n,_) => n } }\nfn f(x) { let p = P(x,None); first(p) + second(p) }\nfn main() { f(21) }", Some("42"));
    compare("type M<a,b> = M(a)\nfn keep(x: M<i32,bool>) { match x { M(n) => n } }\nfn flag(x: M<bool,i32>) { match x { M(n) => n } }\nfn f(x) { let p = M(x); p }\nfn main() { if flag(f(true)) { keep(f(42)) } else { 0 } }", Some("42"));
    compare(&format!("{OPTION}fn main() {{ let n = None; let old = n; let result = {{ let n = Some(2); integer(old) + integer(n) }}; if boolean(old) {{ result }} else {{ 0 }} }}"), Some("42"));
    compare(&format!("{OPTION}fn main() {{ let n = None; let old = n; match Some(2) {{ Some(n) => if boolean(old) {{ integer(old) + n }} else {{ 0 }} | None => 0 }} }}"), Some("42"));
}

#[test]
fn generalized_read_arguments_are_temporary_values_with_source_loan_checks() {
    compare("type O<a> = None | Some(a)\nfn ints(read x: O<i32>) { match x { None => 40 | Some(n) => n } }\nfn bools(read x: O<bool>) { match x { None => 2 | Some(_) => 0 } }\nfn main() { let n = None; ints(n) + bools(n) }", Some("42"));
    compare("type O<a> = None | Some(a)\ntype R<a> = { value: O<a> }\nfn ints(read x: O<i32>) { match x { None => 40 | Some(n) => n } }\nfn bools(read x: O<bool>) { 2 }\nfn main() { let n = record R { value = None }; ints(n.value) + bools(n.value) }", Some("42"));
    compare("type M<a> = M(i32)\nfn take(read a: M<i32>, read b: M<bool>) { 42 }\nfn main() { let n = M(7); take(n,n) }", Some("42"));
    for source in [
        "type M<a> = M(i32)\nfn ints(x: M<i32>) { 40 }\nfn bools(x: M<bool>) { 2 }\nfn keep(read x: M<i32>, y: i32) { y }\nfn main() { let x = M(1); keep(x, { let alias = x; ints(alias) + bools(alias) }) }",
        "type M<a> = M(i32)\nfn keep(read x: M<i32>, inout y: M<i32>) { 42 }\nfn main() { let x = M(1); keep(x, inout x) }",
        "type O<a> = None | Some(a)\nfn keep(read x: O<i32>, y: i32) { y }\nfn main() { let mut n = None; keep(n, { n <- Some(1); 42 }) }",
    ] {
        let p = rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze(&p).is_err(), "{source}");
        assert!(rill::interpreter::run(&p).is_err());
        assert!(rill::backend::emit(&p).is_err());
    }
}

#[test]
fn weak_effectful_and_owner_values_are_never_generalized_by_elaboration() {
    for initializer in [
        "absent()",
        "if true { None } else { None }",
        "match true { _ => None }",
    ] {
        let source = format!("{OPTION}fn absent() {{ None }}\nfn main() {{ let n = {initializer}; let alias = n; if boolean(alias) {{ integer(n) }} else {{ 0 }} }}");
        let p = rill::parser::parse(&source).unwrap();
        assert!(rill::check::analyze(&p).is_err(), "{source}");
    }
    compare(&format!("{OPTION}fn absent(inout count: i32) {{ count <- count + 1; None }}\nfn main() {{ let mut count = 0; let n = absent(inout count); integer(n) + count + 1 }}"), Some("42"));
    for source in [
        "type O<a> = None | Some(a)\nfn take(x: O<SystemBoxI32>) { 42 }\nfn main() { let n = None; take(n) }",
        "type M<a> = M(SystemBoxI32)\nfn main() { let x = system_box_i32_abort(1); let y = M(move x); let z = y; 42 }",
        "type P<a> = P\nfn owner(x: P<SystemBoxI32>) { 42 }\nfn main() { let p = P; owner(p) }",
    ] {
        let p = rill::parser::parse(source).unwrap();
        assert!(rill::backend::emit(&p).is_err(), "{source}");
        assert!(rill::interpreter::run(&p).is_err(), "{source}");
    }
}

#[test]
fn trapping_computations_stay_at_their_original_source_positions() {
    compare(
        "type P<a> = P(i32)\nfn main() { let n = 1 / 0; let unused = P(n); 42 }",
        None,
    );
    compare(
        "type P<a> = P(i32)\nfn main() { let n = 42; let unused = P(n); 1 / 0 }",
        None,
    );
    compare("type P<a> = P(i32)\nfn take(x: P<i32>, y: i32) { 42 }\nfn main() { let n = P(1); take(n, 1 / 0) }", None);
}

#[test]
fn imported_shapes_retain_resolved_nominal_identity() {
    let dir = std::env::temp_dir().join(format!("rill-local-shape-module-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for name in ["a", "b"] {
        fs::write(dir.join(format!("{name}.rill")), "export type O<a> = None | Some(a)\nexport type R<a> = { value: O<a> }\nexport fn integer(x: R<i32>) { 20 }\nexport fn boolean(x: R<bool>) { 1 }").unwrap();
    }
    let source = "import a; import b;\nfn main() { let a = record a.R { value = a.None }; let b = record b.R { value = b.None }; let alias = a; a.integer(alias) + a.boolean(a) + b.integer(b) + b.boolean(b) }";
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    compare_program(&program, source, Some("42"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn captured_read_and_inout_snapshots_do_not_reborrow_the_original_storage() {
    for mode in ["read", "inout"] {
        let arg = if mode == "inout" { "inout x" } else { "x" };
        compare(&format!("type M<a> = M(i32)\nfn ints(x: M<i32>) {{ match x {{ M(n) => n }} }}\nfn bools(x: M<bool>) {{ match x {{ M(n) => n }} }}\nfn f({mode} x: i32) {{ let snapshot = x; let p = M(snapshot); ints(p) + bools(p) }}\nfn main() {{ let mut x = 21; f({arg}) }}"), Some("42"));
    }
    compare("type M<a> = M(i32)\nfn ints(x: M<i32>) { 40 }\nfn bools(x: M<bool>) { 2 }\nfn keep(inout x: i32,y: i32) { y }\nfn main() { let mut x = 1; keep(inout x, { let x = 2; let p = M(x); ints(p) + bools(p) }) }", Some("42"));
}

#[test]
fn generated_nested_shapes_agree_with_the_independent_source_oracle() {
    // Different concrete instantiations share source payloads and alternate
    // nominal enum/record layers. No expected output is produced by lowering.
    for depth in 0..6 {
        let mut source = "type O<a> = None | Some(a)\ntype Pair<a,b> = Pair(a,b)\n".to_string();
        let mut ty = "O<a>".to_string();
        let mut value = "None".to_string();
        for layer in 0..depth {
            let name = format!("Layer{layer}");
            if layer % 2 == 0 {
                source.push_str(&format!("type {name}<a> = {name}(i32,{ty})\n"));
                value = format!("{name}({layer},{value})");
            } else {
                source.push_str(&format!("type {name}<a> = {{ inner: {ty}; fixed: i32 }}\n"));
                value = format!("record {name} {{ fixed = {layer}; inner = {value} }}");
            }
            ty = format!("{name}<a>");
        }
        source.push_str(&format!("fn ints(x: {}) {{ x }}\nfn bools(x: {}) {{ x }}\nfn main() {{ let p = {value}; let alias = p; Pair(ints(alias),bools(p)) }}", ty.replace("<a>", "<i32>"), ty.replace("<a>", "<bool>")));
        let program = rill::parser::parse(&source).unwrap();
        let expected = rill::interpreter::run_reference(&program)
            .unwrap()
            .to_string();
        compare_program(&program, &source, Some(&expected));
    }
}

#[test]
fn concrete_capture_limit_is_exact_and_applies_even_to_unused_shapes() {
    for count in [256, 257] {
        let types = vec!["i32"; count].join(",");
        let values = vec!["1"; count].join(",");
        let source = format!(
            "type Many<a> = Many({types})\nfn main() {{ let unused = Many({values}); 42 }}"
        );
        let program = rill::parser::parse(&source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        if count == 256 {
            let ir = rill::typed::lower_main(&checked).unwrap();
            assert_eq!(rill::typed::evaluate(&ir).unwrap().to_string(), "42");
            rill::typed::emit_c(&ir).unwrap();
        } else {
            let error = rill::typed::lower_main(&checked).unwrap_err();
            assert!(error.starts_with("E-TYPED-LOCAL-COMPLEXITY:"), "{error}");
            assert_eq!(rill::interpreter::run(&program).unwrap_err(), error);
            assert_eq!(rill::backend::emit(&program).unwrap_err(), error);
        }
    }
}

#[test]
fn equal_capture_layouts_do_not_alias_shadowed_storage() {
    compare("type M<a> = M(i32)\nfn ints(x: M<i32>) { match x { M(n) => n } }\nfn bools(x: M<bool>) { match x { M(n) => n } }\nfn main() { let p = M(20); let old = p; let p = M(1); ints(old) + bools(old) + ints(p) + bools(p) }", Some("42"));
}

#[test]
fn sixteen_source_field_steps_can_project_from_a_concrete_captured_record() {
    let mut source = "type O<a> = None | Some(a)\ntype R0 = { value: i32 }\n".to_string();
    let mut value = "record R0 { value = 42 }".to_string();
    for index in 1..=14 {
        source.push_str(&format!("type R{index} = {{ value: R{} }}\n", index - 1));
        value = format!("record R{index} {{ value = {value} }}");
    }
    source.push_str(&format!("type Root<a> = {{ saved: R14; marker: O<a> }}\nfn main() {{ let snapshot = {value}; let p = record Root {{ marker = None; saved = snapshot }}; p.saved{} }}", ".value".repeat(15)));
    compare(&source, Some("42"));
}

#[test]
fn unresolved_non_generalized_uses_keep_an_explicit_representation_boundary() {
    for source in [
        "type O<a> = None | Some(a)\nfn main() { let n = None; match n { None => 42 | Some(_) => 0 } }",
        "type O<a> = None | Some(a)\ntype R<a> = { value: O<a> }\nfn main() { let r = record R { value = None }; let weak = r.value; 42 }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        assert!(rill::typed::lower_main(&checked).unwrap_err().starts_with("E-TYPED-UNSUPPORTED:"));
        assert_eq!(rill::interpreter::run_reference(&program).unwrap().to_string(), "42");
    }
}

#[test]
fn vector_captures_in_generalized_markers_survive_source_mutation() {
    compare(
        include_str!("fixtures/local-polymorphic-vector.rill"),
        Some("42"),
    );
}

#[test]
fn vector_captures_in_generalized_records_survive_source_mutation() {
    compare(
        include_str!("fixtures/local-polymorphic-vector-record.rill"),
        Some("42"),
    );
}
