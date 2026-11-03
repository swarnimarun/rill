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
    let reference = if checked.has_owned() {
        rill::interpreter::run(program)
    } else {
        rill::interpreter::run_reference(program)
    };
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
    assert!(!c.contains("restrict"), "{source}");
    let dir = std::env::temp_dir().join(format!(
        "rill-typed-copy-loan-{}-{}",
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
fn root_loans_and_read_temporaries_share_typed_semantics() {
    for (source, expected) in [
        ("fn get(read x) { x + 0 }\nfn main() { let n = 42; get(n) }", "42"),
        ("fn sum(read a, read b) { a + b }\nfn main() { let n = 21; sum(n, n) }", "42"),
        ("fn bump(inout x) { x <- x + 1 }\nfn main() { let mut n = 41; bump(inout n); n }", "42"),
        ("fn bump(inout x) { x <- x + 1 }\nfn forward(inout x) { bump(inout x) }\nfn main() { let mut n = 41; forward(inout n); n }", "42"),
        ("fn get(read x) { x + 0 }\nfn forward(inout x) { let n = get(x); x <- n + 1 }\nfn main() { let mut n = 41; forward(inout n); n }", "42"),
        ("fn get(read x) { x + 0 }\nfn forward(read x) { get(x) }\nfn main() { forward(42) }", "42"),
        ("fn set(before, inout x) { x <- before + 1 }\nfn main() { let mut n = 41; set(n, inout n); n }", "42"),
        ("fn sum(read a, b) { a + b }\nfn main() { let n = 21; sum(n, n) }", "42"),
        ("fn combine(read a, inout b) { b <- b + 1; a * 10 + b }\nfn main() { let mut n = 1; combine({ n + 0 }, inout n) }", "12"),
        ("fn sum(read a, read b) { a + b }\nfn main() { let mut n = 0; sum({ n <- n + 1; n }, { n <- n + 1; n }) }", "3"),
        ("fn id(read x) { x }\nfn main() { if id(true) { id(42) } else { 0 } }", "42"),
        ("fn replace(inout x, y) { x <- y }\nfn main() { let mut a = 0; let mut b = false; replace(inout a, 42); replace(inout b, true); if b { a } else { 0 } }", "42"),
        ("type O<a> = No | Yes(a)\nfn get(read x) { x }\nfn main() { let mut x = Yes(42); let saved = get(x); x <- No; match saved { Yes(n) => n | No => 0 } }", "42"),
        ("fn get(read x) { x }\nfn main() { let mut b = bytes_new(2); let old = get(b); b <- bytes_set(b, 1, 42); bytes_get(old, 1) + bytes_get(b, 1) }", "42"),
        ("fn get(read x) { x }\nfn main() { byte_len(get(\"é\\0\")) + 39 }", "42"),
        ("fn id(read x) { x }\nfn main() { id(()) }", "()"),
        ("type E = A | B\nfn get(read x) { x }\nfn main() { get(A) }", "A"),
        ("fn bump(inout x) { x <- x + 1 }\nfn main() { let mut n = 0; while n < 42 { bump(inout n) }; n }", "42"),
        ("fn get(read x) { x + 0 }\nfn sum(read a, b) { a + b }\nfn main() { let n = 21; sum(n, { let n = 20; get(n) + 1 }) }", "42"),
        ("fn get(read x) { x + 0 }\nfn sum(read a, b) { a + b }\nfn main() { let n = 21; sum(n, get(n)) }", "42"),
        ("fn recur(n, read x) { if n == 0 { x } else { recur(n - 1, x) } }\nfn main() { recur(30, 42) }", "42"),
    ] { compare(source, Some(expected)); }
}

#[test]
fn copy_loan_argument_and_call_depth_traps_agree() {
    for source in [
        "fn get(read x) { x + 0 }\nfn main() { get(1 / 0) }",
        "fn first(read x, y) { x }\nfn main() { first(42, 1 / 0) }",
        "fn recur(n, read x) { if n == 0 { x } else { recur(n - 1, x) } }\nfn main() { recur(31, 42) }",
    ] { compare(source, None); }
}

#[test]
fn copy_loans_coexist_with_owner_transfers_and_cleanup() {
    for source in [
        "fn get(read x: i32) { x }\nfn main() { get({ let owner = system_box_i32_abort(42); box_read(owner) }) }",
        "fn add(read x: i32, owner: SystemBoxI32) { x + box_read(owner) }\nfn main() { let owner = system_box_i32_abort(40); let n = 2; add(n, move owner) }",
        "fn bump(inout x: i32, read owner: SystemBoxI32) { x <- x + box_read(owner) }\nfn main() { let owner = system_box_i32_abort(40); let mut n = 2; bump(inout n, owner); n }",
        "type E = N | V(i32)\nfn get(read value: E) { match value { N => 0 | V(n) => n } }\nfn main() { let owner = system_box_i32_abort(40); get(V(2)) + box_read(owner) }",
        "fn pass(read x: i32, owner: SystemBoxI32) { let n = x + 0; move owner }\nfn main() { let owner = system_box_i32_abort(42); let n = 1; let moved = pass(n, move owner); box_read(moved) }",
    ] { compare(source, Some("42")); }
}

#[test]
fn conflicting_root_loans_are_rejected_before_execution() {
    for source in [
        "fn f(inout a, inout b) { a <- b }\nfn main() { let mut n = 1; f(inout n, inout n) }",
        "fn f(read a, inout b) { b <- a }\nfn main() { let mut n = 1; f(n, inout n) }",
        "fn f(inout a, read b) { a <- b }\nfn main() { let mut n = 1; f(inout n, n) }",
        "fn f(inout a, b) { a <- b }\nfn main() { let mut n = 1; f(inout n, n) }",
        "fn f(read a, b) { a + b }\nfn main() { let mut n = 1; f(n, { n <- 2; n }) }",
        "fn bump(inout a) { a <- a + 1 }\nfn main() { let n = 1; bump(inout n) }",
        "fn f(read a) { a <- 2 }\nfn main() { f(1) }",
        "fn bump(inout a) { a <- a + 1 }\nfn f(read a) { bump(inout a) }\nfn main() { f(1) }",
    ] {
        let p = rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze(&p).is_err(), "{source}");
        assert!(rill::interpreter::run(&p).is_err());
        assert!(rill::backend::emit(&p).is_err());
    }
}
