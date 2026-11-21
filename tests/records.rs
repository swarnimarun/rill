use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: i32) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(
        rill::interpreter::run(&p).unwrap().to_string(),
        expected.to_string()
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-record-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&cfile, rill::compile(source).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&cfile)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&binary).output().unwrap();
        assert!(run.status.success());
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn construction_order_projection_and_annotated_parameters() {
    for (source,expected) in [
        ("type Point = { x: i32; y: i32 }\nlet main = let mut n = 0 in let p = record Point { y = (let z = n <- n + 1 in n); x = (let z = n <- n + 1 in n) } in p.x * 10 + p.y",21),
        ("type Point = { x: i32; y: i32 }\nlet inc (inout p: Point) = p.x <- p.x + 1\nlet read_x (read p: Point) = p.x\nlet main = let mut p = record Point { x = 41; y = 0 } in let done = inc(inout p) in read_x(p)",42),
        ("type Point = { x: i32; y: i32 }\nlet take (p: Point) = p.x + p.y\nlet main = take(record Point { x = 20; y = 22 })",42),
        ("type Box<a> = { value: a }\nlet get (p: Box<i32>) = p.value\nlet main = get(record Box { value = 42 })",42),
        ("type Choice = Small(i32) | Big(i32, i32)\ntype R = { value: Choice; tail: i32 }\nlet main = let mut r = record R { value = Small(1); tail = 2 } in let done = r.value <- Big(20, 20) in match r.value { Small(x) => x | Big(x, y) => x + y + r.tail }",42),
    ] { compare(source,expected); }
}

#[test]
fn rejects_invalid_records_and_borrow_conflicts() {
    for source in [
        "type P = { x: i32; y: i32 }\nlet main = record P { x = 1 }",
        "type P = { x: i32 }\nlet main = record P { x = 1; x = 2 }",
        "type P = { x: i32 }\nlet main = record P { x = true }",
        "type P = { x: i32 }\nlet main = record P { unknown = 1 }",
        "type P = { x: i32; x: i32 }\nlet main = 0",
        "type P = { x: i32 }\nlet main = let p = record P { x = 1 } in p.x <- 2",
        "type P = { x: i32 }\nlet main = P(1)",
        "type P = { x: i32 }\nlet main = match record P { x = 1 } { P(x) => x }",
        "type P = {}\nlet main = P",
        "type P = { x: i32 }\nlet f p = p.x\nlet main = 0",
        "type P = { x: i32 }\nlet f (inout p: P) n = p.x <- n\nlet main = let mut p = record P { x = 0 } in f(inout p, p.x)",
    ] {
        let p=rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&p).is_err(),"accepted {source}");
        assert!(rill::compile(source).is_err(),"emitted {source}");
    }
}

#[test]
fn imported_records_preserve_nominal_identity_and_resolve_qualified_types() {
    let dir = std::env::temp_dir().join(format!(
        "rill-record-module-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join("point.rill"),
        "export type Point = { x: i32; y: i32 }\nexport let get (p: Point) = p.x + p.y",
    )
    .unwrap();
    fs::write(dir.join("main.rill"),"import point as p; let main = let value = record p.Point { y = 22; x = 20 } in p.get(value)").unwrap();
    let program = rill::modules::compile::load_program(&dir.join("main.rill")).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
    let cfile = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&cfile, rill::backend::emit(&program).unwrap()).unwrap();
    let build = Command::new("cc")
        .args(["-std=c11", "-O2"])
        .arg(&cfile)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(run.stdout, b"42\n");
    fs::remove_dir_all(dir).unwrap();
}
