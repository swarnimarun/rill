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
        "rill-typed-record-{}-{}",
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
fn concrete_copy_records_share_typed_layouts_and_semantics() {
    for (source, expected) in [
        ("type P = { x: i32 }\nfn get(read p: P) { p.x }\nfn main() { get(record P { x = 42 }) }", "42"),
        ("type Marker<a> = {}\nfn make() { record Marker {} }\nfn integer(read m: Marker<i32>) { 40 }\nfn boolean(read m: Marker<bool>) { 2 }\nfn main() { integer(make()) + boolean(make()) }", "42"),
        ("type Empty = {}\nfn main() { record Empty {} }", "Empty {}"),
        ("type Point = { x: i32; y: i32 }\nfn main() { record Point { y = 22; x = 20 } }", "Point { x = 20; y = 22 }"),
        ("type Point = { x: i32; y: i32 }\nfn main() { let mut n = 0; let p = record Point { y = { n <- n + 1; n }; x = { n <- n + 1; n } }; p.x * 10 + p.y }", "21"),
        ("type Point = { x: i32; y: i32 }\nfn main() { let mut p = record Point { x = 1; y = 2 }; p.x <- { p <- record Point { x = 7; y = 8 }; 9 }; p.x * 10 + p.y }", "98"),
        ("type I = { x: i32; y: i32 }\ntype O = { inner: I; other: i32 }\nfn main() { let mut p = record O { inner = record I { x = 1; y = 2 }; other = 3 }; p.inner.x <- { p.inner <- record I { x = 7; y = 8 }; 9 }; p.inner.x * 100 + p.inner.y * 10 + p.other }", "983"),
        ("type Point = { x: i32 }\nfn bump(inout p: Point) { p.x <- p.x + 1 }\nfn get(read p: Point) { p.x }\nfn main() { let mut p = record Point { x = 41 }; bump(inout p); get(p) }", "42"),
        ("type Point = { x: i32 }\nfn get(read p: Point) { p }\nfn main() { let mut p = record Point { x = 1 }; let old = get(p); p.x <- 2; old.x * 10 + p.x }", "12"),
        ("type Box<a> = { value: a }\nfn wrap(x) { record Box { value = x } }\nfn main() { let a = wrap(42); let b = wrap(true); if b.value { a.value } else { 0 } }", "42"),
        ("type Box<a> = { value: a }\nfn through(x) { let b = record Box { value = x }; b.value }\nfn main() { if through(true) { through(42) } else { 0 } }", "42"),
        ("type P = { x: i32 }\nfn replace(inout p, q) { p <- q }\nfn main() { let mut p = record P { x = 1 }; replace(inout p, record P { x = 42 }); p.x }", "42"),
        ("type Choice = Small(i32) | Big(i32, i32)\ntype R = { choice: Choice; tail: i32 }\nfn main() { let mut r = record R { choice = Small(1); tail = 2 }; r.choice <- Big(20, 20); match r.choice { Small(n) => n | Big(a, b) => a + b + r.tail } }", "42"),
        ("type R = { x: i32 }\ntype Choice = None | Some(R)\nfn main() { match Some(record R { x = 42 }) { None => 0 | Some(r) => r.x } }", "42"),
        ("type R = { x: i32 }\nfn main() { let p = record R { x = 42 }; match p { saved => saved.x } }", "42"),
        ("type R<a> = { bytes: a; text: str }\nfn main() { let mut r = record R { bytes = bytes_new(2); text = \"é\\0\" }; let old = r; r.bytes <- bytes_set(r.bytes, 1, 39); bytes_get(old.bytes, 1) + bytes_get(r.bytes, 1) + byte_len(r.text) }", "42"),
        ("type P = { x: i32 }\nfn update(previous, inout p: P) { p.x <- previous + 1 }\nfn main() { let mut p = record P { x = 41 }; update(p.x, inout p); p.x }", "42"),
        ("type P = { x: i32 }\nfn sum(read p: P, n) { p.x + n }\nfn main() { let p = record P { x = 21 }; sum(p, p.x) }", "42"),
        ("type E = {}\ntype R = { empty: E; flag: bool; unit: unit }\nfn main() { record R { unit = (); flag = true; empty = record E {} } }", "R { empty = E {}; flag = true; unit = () }"),
    ] { compare(source, Some(expected)); }
}
#[test]
fn record_construction_and_assignment_traps_preserve_order() {
    for source in [
        "type P = { x: i32; y: i32 }\nfn main() { record P { y = 1 / 0; x = 42 } }",
        "type P = { x: i32; y: i32 }\nfn main() { let mut p = record P { x = 1; y = 2 }; p.x <- { p.y <- 3; 1 / 0 }; p.x }",
    ] { compare(source, None); }
}
#[test]
fn copy_records_coexist_with_restricted_owner_cleanup() {
    for source in [
        "type P = { x: i32 }\nfn get(read p: P) { p.x }\nfn main() { let owner = system_box_i32_abort(40); let p = record P { x = 2 }; box_read(owner) + get(p) }",
        "type P = { x: i32 }\ntype Bundle = Bundle(P, SystemBoxI32)\nfn main() { let owner = system_box_i32_abort(40); let bundle = Bundle(record P { x = 2 }, move owner); match move bundle { Bundle(p, owner) => p.x + box_read(owner) } }",
        "type P = { x: i32; y: i32 }\nfn main() { let p = record P { y = { let owner = system_box_i32_abort(2); box_read(owner) }; x = { let owner = system_box_i32_abort(40); box_read(owner) } }; p.x + p.y }",
    ] { compare(source, Some("42")); }
}
#[test]
fn record_field_loans_share_checked_typed_execution() {
    for source in [
        "type P = { x: i32 }\nfn get(read x) { x + 0 }\nfn main() { let p = record P { x = 42 }; get(p.x) }",
        "type P = { x: i32 }\nfn bump(inout x) { x <- x + 1 }\nfn main() { let mut p = record P { x = 41 }; bump(inout p.x); p.x }",
    ] { compare(source,Some("42")); }
}

#[test]
fn record_layout_is_tag_free_and_empty_records_remain_valid_c() {
    let p=rill::parser::parse("type E = {}\ntype P = { a: E; x: i32 }\nfn main() { record P { x = 42; a = record E {} } }").unwrap();
    let checked = rill::check::analyze(&p).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    let c = rill::typed::emit_c(&typed).unwrap();
    assert!(c.contains("typedef struct { unsigned char unused; } rl_adt_0;"));
    assert!(c.contains("typedef struct { rl_adt_0 f0; int32_t f1; } rl_adt_1;"));
    assert!(!c.contains("int tag;") && !c.contains("union {"));
    compare_program(&p, "compact records", Some("P { a = E {}; x = 42 }"));
}
#[test]
fn typed_records_preserve_sixteen_step_projection_limit() {
    let mut source = String::from("type R0 = { value: i32 }\n");
    let mut value = String::from("record R0 { value = 41 }");
    for i in 1..16 {
        source.push_str(&format!("type R{i} = {{ next: R{} }}\n", i - 1));
        value = format!("record R{i} {{ next = {value} }}");
    }
    let path = format!("p{}.value", ".next".repeat(15));
    source.push_str(&format!(
        "fn main() {{ let mut p = {value}; {path} <- {path} + 1; {path} }}"
    ));
    compare(&source, Some("42"));
    assert!(
        rill::parser::parse(&format!("fn main() {{ p{}.value }}", ".next".repeat(16))).is_err()
    );
}
#[test]
fn imported_records_preserve_nominal_identity_and_field_names() {
    let dir =
        std::env::temp_dir().join(format!("rill-typed-record-modules-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for module in ["a", "b"] {
        fs::write(
            dir.join(format!("{module}.rill")),
            "export type P<a> = { value: a }\nexport fn wrap(x) { record P { value = x } }",
        )
        .unwrap();
    }
    let source="import a; import b;\nfn main() { let x = a.wrap(40); let y = b.wrap(2); let flag = a.wrap(true); if flag.value { x.value + y.value } else { 0 } }";
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let p = rill::modules::compile::load_program(&entry).unwrap();
    compare_program(&p, source, Some("42"));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn owned_and_positional_records_remain_outside_this_slice() {
    for source in [
        "type P = { x: i32 }\nfn main() { P(42) }",
        "type P = { x: i32 }\nfn main() { match record P { x = 42 } { P(n) => n } }",
        "type P = { owner: SystemBoxI32 }\nfn main() { let b = system_box_i32_abort(42); let p = record P { owner = move b }; 42 }",
    ]{let p=rill::parser::parse(source).unwrap(); assert!(rill::check::analyze(&p).is_err(),"{source}");assert!(rill::interpreter::run(&p).is_err());assert!(rill::backend::emit(&p).is_err());}
    compare("type P = { x: i32 }\nfn get(read x) { x + 0 }\nfn main() { let owner = system_box_i32_abort(40); let p = record P { x = 2 }; box_read(owner) + get(p.x) }",Some("42"));
    let source="type Maybe<a> = No | Yes(a)\ntype R<a> = { value: Maybe<a> }\nfn use(r: R<i32>) { 42 }\nfn main() { let r = record R { value = No }; use(r) }";
    compare(source, Some("42"));
}
