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
        "rill-typed-instance-{}-{}",
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

#[test]
fn concrete_generic_calls_share_checked_typed_semantics() {
    for (source, expected) in [
        ("fn id(x) { x }\nfn main() { let a = id(40); if id(true) { a + id(2) } else { 0 } }", "42"),
        ("fn main() { let a = id(40); if id(true) { a + id(2) } else { 0 } }\nfn id(x) { x }", "42"),
        ("type Option<a> = None | Some(a)\nfn wrap(x) { Some(x) }\nfn main() { wrap(wrap(42)) }", "Some(Some(42))"),
        ("type Pair<a,b> = Pair(a,b)\nfn make(x, y) { Pair(x, y) }\nfn main() { let a = make(40, true); let b = make(false, 2); match a { Pair(n, _) => match b { Pair(_, m) => n + m } } }", "42"),
        ("fn id(x) { x }\nfn forward(x) { id(x) }\nfn main() { let a = forward(42); if forward(true) { a } else { 0 } }", "42"),
        ("fn choose(flag, a, b) { if flag { a } else { b } }\nfn main() { choose(true, 40, 0) + byte_len(choose(false, \"a\", \"é\")) }", "42"),
        ("fn recur(n, x) { if n == 0 { x } else { recur(n - 1, x) } }\nfn main() { let a = recur(2, 42); if recur(2, true) { a } else { 0 } }", "42"),
        ("fn first(n, x) { if n == 0 { x } else { second(n - 1, x) } }\nfn second(n, x) { if n == 0 { x } else { first(n - 1, x) } }\nfn main() { let a = first(7, 42); if second(6, true) { a } else { 0 } }", "42"),
        ("type Option<a> = None | Some(a)\nfn absent() { None }\nfn integer(x: Option<i32>) { match x { Some(n) => n | None => 40 } }\nfn boolean(x: Option<bool>) { match x { Some(b) => b | None => true } }\nfn main() { if boolean(absent()) { integer(absent()) + 2 } else { 0 } }", "42"),
        ("type Phantom<a> = Marker\nfn mark(x) { Marker }\nfn integer(x: Phantom<i32>) { match x { Marker => 40 } }\nfn boolean(x: Phantom<bool>) { match x { Marker => 2 } }\nfn main() { integer(mark(0)) + boolean(mark(0)) }", "42"),
        ("fn id(x) { x }\nfn main() { let mut b = bytes_new(2); let old = id(b); b <- bytes_set(b, 1, 42); bytes_get(old, 1) + bytes_get(id(b), 1) }", "42"),
        ("fn id(x) { x }\nfn main() { byte_len(id(\"hé\\0x\")) + id(37) }", "42"),
        ("fn id(x) { x }\nfn main() { let mut n = 0; let a = id({ n <- n + 1; n }); let b = id({ n <- n + 1; n }); a * 10 + b }", "12"),
        ("fn id(x) { x }\nfn unused(x) { x }\nfn main() { id(42) }", "42"),
        ("fn id(x) { x }\nfn main() { id(()) }", "()"),
        ("fn f(a, b, n) { if n == 0 { a } else { g(b, a, n - 1) } }\nfn g(a, b, n) { if n == 0 { b } else { f(b, a, n - 1) } }\nfn main() { let a = f(42, true, 9); let b = f(false, 13, 9); if b { 0 } else { a } }", "42"),
        ("type Option<a> = None | Some(a)\nfn unwrap(x, fallback) { match x { Some(n) => n | None => fallback } }\nfn main() { unwrap(Some(40), 0) + (if unwrap(Some(true), false) { 2 } else { 0 }) }", "42"),
        ("fn replace(x, y) { let mut value = x; value <- y; value }\nfn main() { let a = replace(0, 42); if replace(false, true) { a } else { 0 } }", "42"),
    ] { compare(source, Some(expected)); }
}

#[test]
fn specialization_preserves_traps_and_argument_evaluation_order() {
    for source in [
        "fn id(x) { x }\nfn main() { id(1 / 0) }",
        "fn first(x, y) { x }\nfn main() { first(42, 1 / 0) }",
        "fn recur(n, x) { if n == 0 { x } else { recur(n - 1, x) } }\nfn main() { recur(31, 42) }",
    ] {
        compare(source, None);
    }
    compare(
        "fn recur(n, x) { if n == 0 { x } else { recur(n - 1, x) } }\nfn main() { recur(30, 42) }",
        Some("42"),
    );
}

#[test]
fn unused_declarations_are_checked_but_not_instantiated() {
    compare("fn uncalled(read x) { x }\nfn main() { 42 }", Some("42"));
    compare(
        "type Huge<a> = Huge(a, a)\nfn uncalled(x) { Huge(x, x) }\nfn main() { 42 }",
        Some("42"),
    );
    let program = rill::parser::parse("fn unused() { true + 1 }\nfn main() { 42 }").unwrap();
    assert!(rill::interpreter::run(&program)
        .unwrap_err()
        .contains("E-TYPE"));
    assert!(rill::backend::emit(&program)
        .unwrap_err()
        .contains("E-TYPE"));
}

#[test]
fn local_values_specialize_but_owner_generics_keep_explicit_rejection() {
    compare("type Option<a> = None | Some(a)\nfn main() { let n = None; if true { Some(1) } else { n } }", Some("Some(1)"));
    let program = rill::parser::parse("fn id(x) { x }\nfn main() { let b = system_box_i32_abort(42); let moved = id(move b); box_read(moved) }").unwrap();
    assert!(rill::check::analyze(&program)
        .err()
        .unwrap()
        .contains("generic"));
}

#[test]
fn imported_generic_functions_preserve_module_nominal_identities() {
    let dir =
        std::env::temp_dir().join(format!("rill-typed-generic-modules-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for module in ["a", "b"] {
        fs::write(
            dir.join(format!("{module}.rill")),
            "export type O<a> = No | Yes(a)\nexport fn wrap(x) { Yes(x) }\nexport fn id(x) { x }",
        )
        .unwrap();
    }
    let source = "import a; import b;\nfn main() { let x = a.id(a.wrap(40)); let y = b.id(b.wrap(2)); let z = a.id(b.wrap(true)); match x { a.Yes(n) => match y { b.Yes(m) => if match z { b.Yes(v) => v | b.No => false } { n + m } else { 0 } | b.No => 0 } | a.No => 0 } }";
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    compare_program(&program, source, Some("42"));
    fs::remove_dir_all(dir).unwrap();
}
