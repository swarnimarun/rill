use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: i32) {
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected.to_string()
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-field-loan-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&c, rill::compile(source).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&c)
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
fn disjoint_fields_can_be_borrowed_and_modified() {
    for (source,expected) in [
  ("type Pair = { x: i32; y: i32 }\nlet swap (inout a) (inout b) = let old = a in let one = a <- b in b <- old\nlet main = let mut p = record Pair { x = 1; y = 2 } in let done = swap(inout p.x, inout p.y) in p.x * 10 + p.y",21),
  ("type Pair = { x: i32; y: i32 }\nlet set (read a) (inout b) = b <- a + 1\nlet main = let mut p = record Pair { x = 1; y = 0 } in let done = set(p.x, inout p.y) in p.x * 10 + p.y",12),
  ("type Pair = { x: i32; y: i32 }\nlet set (inout a) b = a <- b\nlet main = let mut p = record Pair { x = 1; y = 2 } in let done = set(inout p.x, (let change = p.y <- 3 in p.y)) in p.x * 10 + p.y",33),
  ("type Pair = { x: i32; y: i32 }\nlet inc (inout x) = x <- x + 1\nlet forward (inout p: Pair) = inc(inout p.x)\nlet main = let mut p = record Pair { x = 41; y = 0 } in let done = forward(inout p) in p.x",42),
        ("type Choice = Small(i32) | Big(i32, i32)\ntype Pair = { x: Choice; y: i32 }\nlet set (inout n) value = n <- value\nlet main = let mut p = record Pair { x = Small(1); y = 0 } in let done = set(inout p.y, (let changed = p.x <- Big(20, 20) in 42)) in match p.x { Small(n) => n | Big(a, b) => a + b + p.y }",82),
 ] {compare(source,expected);}
}

#[test]
fn overlapping_field_and_root_loans_are_rejected() {
    for source in [
  "type Pair = { x: i32; y: i32 }\nlet f (inout a) (inout b) = ()\nlet main = let mut p = record Pair { x = 1; y = 2 } in f(inout p.x, inout p.x)",
  "type Pair = { x: i32; y: i32 }\nlet f (inout a: Pair) (inout b) = ()\nlet main = let mut p = record Pair { x = 1; y = 2 } in f(inout p, inout p.y)",
        "type Pair = { x: i32; y: i32 }\nlet f (inout a) (inout b: Pair) = ()\nlet main = let mut p = record Pair { x = 1; y = 2 } in f(inout p.x, inout p)",
  "type Pair = { x: i32; y: i32 }\nlet f (read a) (inout b) = ()\nlet main = let mut p = record Pair { x = 1; y = 2 } in f(p.x, inout p.x)",
  "type Pair = { x: i32; y: i32 }\nlet f (inout a) b = ()\nlet main = let mut p = record Pair { x = 1; y = 2 } in f(inout p.x, p.x)",
  "type Pair = { x: i32; y: i32 }\nlet f (inout a) = ()\nlet main = let p = record Pair { x = 1; y = 2 } in f(inout p.x)",
 ] {let p=rill::parser::parse(source).unwrap(); assert!(rill::check::check(&p).is_err(),"accepted {source}"); assert!(rill::compile(source).is_err());}
}
