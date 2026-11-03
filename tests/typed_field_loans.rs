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
    let checked = rill::check::analyze(program).unwrap_or_else(|error| panic!("{source}: {error}"));
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
        "rill-typed-field-loan-{}-{}",
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
fn projected_loans_preserve_order_disjointness_and_snapshots() {
    for (source,expected) in [
        ("type P = { x: i32; y: i32 }\nfn swap(inout a, inout b) { let old = a; a <- b; b <- old }\nfn main() { let mut p = record P { x = 1; y = 2 }; swap(inout p.x, inout p.y); p.x * 10 + p.y }","21"),
        ("type P = { x: i32; y: i32 }\nfn set(read a, inout b) { b <- a + 1 }\nfn main() { let mut p = record P { x = 20; y = 0 }; set(p.x, inout p.y); p.x + p.y + 1 }","42"),
        ("type P = { x: i32 }\nfn sum(read a, read b) { a + b }\nfn main() { let p = record P { x = 21 }; sum((p.x), p.x) }","42"),
        ("type P = { x: i32; y: i32 }\nfn sum(read a, b) { a + b }\nfn main() { let mut p = record P { x = 20; y = 0 }; sum(p.x, { p.y <- 22; p.y }) }","42"),
        ("type P = { x: i32; y: i32 }\nfn set(inout a, b) { a <- b }\nfn main() { let mut p = record P { x = 0; y = 21 }; set(inout p.x, p.y); p.x + p.y }","42"),
        ("type P = { x: i32 }\nfn change(read a, inout b) { b <- b + 1; a * 10 + b }\nfn main() { let mut p = record P { x = 1 }; change({ p.x + 0 }, inout p.x) }","12"),
        ("type P = { x: i32 }\nfn set(old, inout x) { x <- old + 1 }\nfn main() { let mut p = record P { x = 41 }; set(p.x, inout p.x); p.x }","42"),
        ("type I = { x: i32; y: i32 }\ntype O = { inner: I; other: i32 }\nfn swap(inout a, inout b) { let old = a; a <- b; b <- old }\nfn forward(inout p: O) { swap(inout p.inner.x, inout p.inner.y) }\nfn main() { let mut p = record O { inner = record I { x = 20; y = 22 }; other = 1 }; forward(inout p); p.inner.x + p.inner.y }","42"),
        ("type P = { x: i32; y: i32 }\nfn get(read p: P) { p.x }\nfn sum(read a, b) { a + b }\nfn main() { let p = record P { x = 21; y = 0 }; sum(p.x, get(p)) }","42"),
        ("type P = { x: i32; y: i32 }\nfn get(read x) { x + 0 }\nfn sum(read a, b) { a + b }\nfn main() { let p = record P { x = 21; y = 0 }; sum(p.x, { let n = get(p.x); n }) }","42"),
        ("type P = { x: i32 }\nfn get(read x) { x + 0 }\nfn sum(read a, b) { a + b }\nfn main() { let p = record P { x = 20 }; sum(p.x, { let p = record P { x = 22 }; get(p.x) }) }","42"),
        ("type P = { x: i32 }\nfn bump(inout x) { x <- x + 1 }\nfn main() { let mut p = record P { x = 0 }; while p.x < 42 { bump(inout p.x) }; p.x }","42"),
        ("type P = { x: i32 }\nfn get(read x) { x + 0 }\nfn main() { let p = record P { x = 42 }; match p { p => get(p.x) } }","42"),
        ("type E = A(i32) | B(i32, i32)\ntype P = { before: E; x: i32 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut p = record P { before = A(1); x = 0 }; set(inout p.x, { p.before <- B(20, 20); 2 }); match p.before { A(n) => n | B(a, b) => a + b + p.x } }","42"),
        ("type R<a> = { value: a }\nfn replace(inout x, value) { x <- value }\nfn main() { let mut a = record R { value = 0 }; let mut b = record R { value = false }; replace(inout a.value, 42); replace(inout b.value, true); if b.value { a.value } else { 0 } }","42"),
        ("type R<a> = { bytes: a }\nfn fill(inout b) { let base = if true { b } else { bytes_new(2) }; b <- bytes_set(base, 1, 42) }\nfn copy(read b) { b }\nfn main() { let mut r = record R { bytes = bytes_new(2) }; let old = copy(r.bytes); fill(inout r.bytes); bytes_get(old, 1) + bytes_get(r.bytes, 1) }","42"),
        ("type R = { text: str }\nfn len(read s) { byte_len(s) }\nfn main() { let r = record R { text = \"é\\0\" }; len(r.text) + 39 }","42"),
        ("type E = {}\ntype R = { empty: E; unit: unit }\nfn id(read x) { x }\nfn main() { let r = record R { empty = record E {}; unit = () }; let a = id(r.empty); id(r.unit) }","()"),
        ("type I = { x: i32; y: i32 }\ntype O = { a: I; b: I }\nfn replace(inout a: I, read b: I) { a <- b }\nfn main() { let mut p = record O { a = record I { x = 1; y = 2 }; b = record I { x = 20; y = 22 } }; replace(inout p.a, p.b); p.a.x + p.a.y }","42"),
        ("type P = { x: i32 }\nfn recur(n, inout x) { x <- x + 1; if n == 0 { x } else { recur(n - 1, inout x) } }\nfn main() { let mut p = record P { x = 11 }; recur(30, inout p.x) }","42"),
        ("type P = { x: i32 }\nfn snapshot(value: P, inout x) { x <- 22; value.x }\nfn main() { let mut p = record P { x = 20 }; let old = snapshot(p, inout p.x); old + p.x }","42"),
    ]{compare(source,Some(expected));}
}
#[test]
fn field_loan_argument_and_call_traps_match() {
    for source in [
        "type P = { x: i32; y: i32 }\nfn set(inout x, n) { x <- n }\nfn main() { let mut p = record P { x = 1; y = 2 }; set(inout p.x, 1 / 0) }",
        "type P = { x: i32 }\nfn recur(n, inout x) { x <- x + 1; if n == 0 { x } else { recur(n - 1, inout x) } }\nfn main() { let mut p = record P { x = 11 }; recur(31, inout p.x) }",
    ]{compare(source,None);}
}
#[test]
fn composed_forwarding_paths_do_not_add_a_new_source_depth_limit() {
    let mut source = String::from("type R0 = { value: i32 }\n");
    let mut value = String::from("record R0 { value = 41 }");
    for i in 1..20 {
        source.push_str(&format!("type R{i} = {{ next: R{} }}\n", i - 1));
        value = format!("record R{i} {{ next = {value} }}");
    }
    source.push_str(&format!("fn bump(inout x) {{ x <- x + 1 }}\nfn inner(inout p: R9) {{ bump(inout p{}.value) }}\nfn outer(inout p: R19) {{ inner(inout p{}) }}\nfn result(read p: R9) {{ p{}.value }}\nfn main() {{ let mut p = {value}; outer(inout p); result(p{}) }}", ".next".repeat(9),".next".repeat(10),".next".repeat(9),".next".repeat(10)));
    compare(&source, Some("42"));
}
#[test]
fn copy_field_loans_coexist_with_restricted_owners() {
    for source in [
        "type P = { x: i32 }\nfn get(read x) { x + 0 }\nfn main() { let owner = system_box_i32_abort(40); let p = record P { x = 2 }; box_read(owner) + get(p.x) }",
        "type P = { x: i32 }\nfn consume(inout x: i32, owner: SystemBoxI32) { x <- x + box_read(owner) }\nfn main() { let owner = system_box_i32_abort(40); let mut p = record P { x = 2 }; consume(inout p.x, move owner); p.x }",
        "type P = { x: i32 }\nfn pass(read x: i32, owner: SystemBoxI32) { let n = x + 0; move owner }\nfn main() { let owner = system_box_i32_abort(42); let p = record P { x = 1 }; let moved = pass(p.x, move owner); box_read(moved) }",
        "type P = { x: i32; y: i32 }\nfn set(inout x: i32, y: i32) { x <- y }\nfn main() { let mut p = record P { x = 0; y = 0 }; set(inout p.x, { let owner = system_box_i32_abort(42); p.y <- box_read(owner); p.y }); p.x }",
    ]{compare(source,Some("42"));}
}

#[test]
fn source_rejects_prefix_root_and_later_argument_conflicts() {
    for body in [
        "both(inout p.a.x, inout p.a.x)",
        "observe(p.a.x, inout p.a.x)",
        "sum(p.a.x, { p.a.x <- 9; 42 })",
        "set(inout p.a.x, p.a.x)",
        "set(inout p.a.x, { let copy = p; copy.a.x })",
        "set(inout p.a.x, { p.a <- record I { x = 2; y = 3 }; 42 })",
        "set(inout p.a.x, { p <- record O { a = record I { x = 1; y = 2 }; b = record I { x = 3; y = 4 } }; 42 })",
        "sub(inout p.a, p.a.x)",
        "reverse(p.a.x, inout p.a)",
        "whole(inout p, p.a.x)",
        "last(p.a.x, inout p)",
        "sum(p.a.x, { let n = get(p.a.x); p.a.x <- n; n })",
        "set(inout p.a.x, get(p.a.x))",
        "set(inout p.a.x, { let n = get(p.b.x); p.a.x })",
    ] {
        let source=format!("type I = {{ x: i32; y: i32 }}\ntype O = {{ a: I; b: I }}\nfn both(inout a: i32, inout b: i32) {{ () }}\nfn observe(read a: i32, inout b: i32) {{ () }}\nfn set(inout a: i32, n: i32) {{ a <- n }}\nfn sub(inout a: I, n: i32) {{ a.x <- n }}\nfn reverse(read n: i32, inout a: I) {{ a.x <- n }}\nfn whole(inout a: O, n: i32) {{ a.a.x <- n }}\nfn last(read n: i32, inout a: O) {{ a.a.x <- n }}\nfn sum(read a: i32, n: i32) {{ a + n }}\nfn get(read a: i32) {{ a }}\nfn main() {{ let mut p = record O {{ a = record I {{ x = 1; y = 2 }}; b = record I {{ x = 3; y = 4 }} }}; {body} }}");
        let program=rill::parser::parse(&source).unwrap();
        assert!(rill::check::analyze(&program).is_err(),"accepted: {source}");
        assert!(rill::interpreter::run(&program).is_err());
        assert!(rill::backend::emit(&program).is_err());
    }
    for source in [
        "type P = { x: i32 }\nfn bump(inout n) { n <- n + 1 }\nfn main() { let p = record P { x = 41 }; bump(inout p.x) }",
        "type P = { x: i32 }\nfn bump(inout n) { n <- n + 1 }\nfn bad(read p: P) { bump(inout p.x) }\nfn main() { let p = record P { x = 41 }; bad(p) }",
    ] {
        let p=rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze(&p).is_err());
        assert!(rill::interpreter::run(&p).is_err());
        assert!(rill::backend::emit(&p).is_err());
    }
}

#[test]
fn individual_field_loan_operands_keep_the_sixteen_step_limit() {
    for depth in [16, 17] {
        let mut source = String::from("type R0 = { value: i32 }\n");
        let mut value = String::from("record R0 { value = 41 }");
        for i in 1..depth {
            source.push_str(&format!("type R{i} = {{ next: R{} }}\n", i - 1));
            value = format!("record R{i} {{ next = {value} }}");
        }
        let path = format!("{}.value", ".next".repeat(depth - 1));
        source.push_str(&format!("fn bump(inout x) {{ x <- x + 1 }}\nfn get(read x) {{ x + 0 }}\nfn main() {{ let mut p = {value}; bump(inout p{path}); get(p{path}) }}"));
        if depth == 16 {
            compare(&source, Some("42"));
        } else {
            assert!(rill::parser::parse(&source)
                .unwrap_err()
                .contains("field path exceeds 16"));
            assert!(rill::compile(&source).is_err());
        }
    }
}
