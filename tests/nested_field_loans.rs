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
        "rill-nested-loan-{}-{}",
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
fn nested_disjoint_paths_forwarding_and_writeback() {
    for (source,expected) in [
  ("type Inner = { x: i32; y: i32 }\ntype Outer = { inner: Inner; other: i32 }\nlet swap (inout a) (inout b) = let old = a in let one = a <- b in b <- old\nlet main = let mut p = record Outer { inner = record Inner { x = 1; y = 2 }; other = 0 } in let done = swap(inout p.inner.x, inout p.inner.y) in p.inner.x * 10 + p.inner.y",21),
  ("type Inner = { x: i32; y: i32 }\ntype Outer = { inner: Inner; other: i32 }\nlet set (read a) (inout b) = b <- a + 1\nlet main = let mut p = record Outer { inner = record Inner { x = 1; y = 0 }; other = 0 } in let done = set(p.inner.x, inout p.inner.y) in p.inner.x * 10 + p.inner.y",12),
  ("type Inner = { x: i32 }\ntype Outer = { inner: Inner; other: i32 }\nlet inc (inout x) = x <- x + 1\nlet forward (inout p: Outer) = inc(inout p.inner.x)\nlet main = let mut p = record Outer { inner = record Inner { x = 41 }; other = 0 } in let done = forward(inout p) in p.inner.x",42),
  ("type Choice = Small(i32) | Big(i32, i32)\ntype Inner = { before: Choice; value: i32 }\ntype Outer = { before: Choice; inner: Inner }\nlet set (inout x) n = x <- n\nlet main = let mut p = record Outer { before = Small(1); inner = record Inner { before = Small(2); value = 0 } } in let done = set(inout p.inner.value, (let a = p.before <- Big(10, 10) in let b = p.inner.before <- Big(20, 20) in 42)) in match p.before { Small(n) => n | Big(a, b) => match p.inner.before { Small(n) => n | Big(c, d) => a + b + c + d + p.inner.value } }",102),
 ] {compare(source,expected);}
}
#[test]
fn root_and_prefix_overlaps_are_rejected() {
    for source in [
  "type I = { x: i32; y: i32 }\ntype O = { inner: I }\nlet f (inout a) (inout b) = ()\nlet main = let mut p = record O { inner = record I { x = 1; y = 2 } } in f(inout p.inner, inout p.inner.x)",
  "type I = { x: i32; y: i32 }\ntype O = { inner: I }\nlet f (inout a) (inout b) = ()\nlet main = let mut p = record O { inner = record I { x = 1; y = 2 } } in f(inout p.inner.x, inout p.inner)",
  "type I = { x: i32 }\ntype O = { inner: I }\nlet f (inout a) b = ()\nlet main = let mut p = record O { inner = record I { x = 1 } } in f(inout p.inner.x, p.inner)",
  "type I = { x: i32 }\ntype O = { inner: I }\nlet f (read a) (inout b) = ()\nlet main = let mut p = record O { inner = record I { x = 1 } } in f(p.inner, inout p.inner.x)",
 ] {let p=rill::parser::parse(source).unwrap(); assert!(rill::check::check(&p).is_err(),"accepted {source}"); assert!(rill::compile(source).is_err());}
}

#[test]
fn sixteen_field_path_is_supported_and_seventeen_is_rejected() {
    let mut source = String::from("type R0 = { value: i32 }\n");
    let mut value = String::from("record R0 { value = 42 }");
    for i in 1..16 {
        source.push_str(&format!("type R{i} = {{ next: R{} }}\n", i - 1));
        value = format!("record R{i} {{ next = {value} }}");
    }
    source.push_str(&format!(
        "let main = let p = {value} in p{}.value",
        ".next".repeat(15)
    ));
    compare(&source, 42);
    assert!(rill::parser::parse(&format!("let main = p{}.value", ".next".repeat(16))).is_err());
}
