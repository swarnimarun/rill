use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: Result<&str, &str>) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    let actual = rill::interpreter::run(&p);
    match expected {
        Ok(value) => assert_eq!(actual.unwrap().to_string(), value),
        Err(error) => assert_eq!(actual.unwrap_err(), error),
    }
    let dir = std::env::temp_dir().join(format!(
        "rill-owned-bytes-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
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
        match expected {
            Ok(value) => {
                assert!(run.status.success());
                assert_eq!(run.stdout, format!("{value}\n").as_bytes());
            }
            Err(_) => {
                assert!(!run.status.success());
                assert!(run.stdout.is_empty());
                assert_eq!(run.stderr, b"rill: runtime trap\n");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn zero_initialization_moves_and_borrowed_helpers_match_c() {
    for (source, expected) in [
        ("let main = let b = system_bytes_zeroed_abort(0) in owned_bytes_len(b)","0"),
        ("let main = let b = system_bytes_zeroed_abort(1) in owned_bytes_get(b, 0)","0"),
        ("let main = let n = 2 + 2 in let mut b = system_bytes_zeroed_abort(n) in let done = owned_bytes_set(inout b, 3, 255) in owned_bytes_get(b, 0) + owned_bytes_get(b, 3) + owned_bytes_len(b)","259"),
        ("let get (read b) = owned_bytes_get(b, 0)\nlet put (inout b) = owned_bytes_set(inout b, 0, 42)\nlet main = let mut a = system_bytes_zeroed_abort(4) in let b = move a in let mut c = move b in let done = put(inout c) in get(c)","42"),
        ("let main = let mut n = 0 in let mut b = system_bytes_zeroed_abort(2) in let done = owned_bytes_set(inout b, (let update = n <- n + 1 in n - 1), (let update = n <- n + 1 in n)) in n * 100 + owned_bytes_get(b, 0)","202"),
    ] { compare(source, Ok(expected)); }
}

#[test]
fn invalid_lengths_indices_and_byte_values_trap() {
    for source in [
        "let main = let b = system_bytes_zeroed_abort(-1) in owned_bytes_len(b)",
        "let main = let b = system_bytes_zeroed_abort(0) in owned_bytes_get(b, 0)",
        "let main = let b = system_bytes_zeroed_abort(2) in owned_bytes_get(b, -1)",
        "let main = let b = system_bytes_zeroed_abort(2) in owned_bytes_get(b, 2)",
        "let main = let mut b = system_bytes_zeroed_abort(2) in owned_bytes_set(inout b, 2, 0)",
        "let main = let mut b = system_bytes_zeroed_abort(2) in owned_bytes_set(inout b, 0, -1)",
        "let main = let mut b = system_bytes_zeroed_abort(2) in owned_bytes_set(inout b, 0, 256)",
    ] {
        compare(source, Err("runtime trap"));
    }
}

#[test]
fn owner_escapes_aliasing_and_implicit_copies_are_rejected() {
    for source in [
        "let main = let b = system_bytes_zeroed_abort(1) in let c = b in owned_bytes_len(c)",
        "let main = let b = system_bytes_zeroed_abort(1) in let c = move b in owned_bytes_len(b)",
        "let main = let b = system_bytes_zeroed_abort(1) in b",
        "let main = system_bytes_zeroed_abort(1)",
        "let main = let b = system_bytes_zeroed_abort(1) in owned_bytes_set(inout b, 0, 2)",
        "let main = let mut b = system_bytes_zeroed_abort(1) in owned_bytes_set(inout b, 0, owned_bytes_get(b, 0))",
        "let f (read a) (inout b) = owned_bytes_set(inout b, 0, owned_bytes_get(a, 0))\nlet main = let mut b = system_bytes_zeroed_abort(1) in f(b, inout b)",
    ] {
        let p=rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&p).is_err(), "accepted {source}");
        assert!(rill::interpreter::run(&p).is_err());
        assert!(rill::backend::emit(&p).is_err());
    }
}

fn traced(source: &str, expected: &str, normal_trace: &str, fail: bool) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "rill-owned-bytes-trace-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let fixture = dir.join("fixture.c");
    let object = dir.join("program.o");
    let binary = dir.join("program");
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
    fs::write(
        &fixture,
        r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void *allocated;
void *fixture_malloc(size_t size) {
    if (getenv("RILL_FAIL")) { fputs("A-fail\n", stderr); return NULL; }
    void *p = malloc(size);
    if (!p) return NULL;
    memset(p, 0xa5, size);
    allocated = p;
    fputs("A\n", stderr);
    return p;
}
void fixture_free(void *p) { fputs(p == allocated ? "F\n" : "F-unknown\n", stderr); free(p); }
int rl_generated_main(void);
int main(void) { return rl_generated_main(); }
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args([
                "-std=c11",
                opt,
                "-Dmain=rl_generated_main",
                "-Dmalloc=fixture_malloc",
                "-Dfree=fixture_free",
                "-c",
            ])
            .arg(&c)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let link = Command::new("cc")
            .arg(&object)
            .arg(&fixture)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let run = Command::new(&binary).output().unwrap();
        if expected == "trap" {
            assert!(!run.status.success());
            assert!(run.stdout.is_empty());
        } else {
            assert!(run.status.success());
            assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
        }
        assert_eq!(run.stderr, normal_trace.as_bytes());
        if fail {
            let oom = Command::new(&binary)
                .env("RILL_FAIL", "1")
                .output()
                .unwrap();
            assert!(!oom.status.success());
            assert!(oom.stdout.is_empty());
            assert_eq!(oom.stderr, b"A-fail\nrill: runtime trap\n");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn allocator_trace_zero_length_initialization_and_failure() {
    traced(
        "let main = let b = system_bytes_zeroed_abort(0) in owned_bytes_len(b)",
        "0",
        "A\nF\n",
        true,
    );
    traced("let main = let b = system_bytes_zeroed_abort(16) in owned_bytes_get(b, 0) + owned_bytes_get(b, 15)","0","A\nF\n",true);
    traced(
        "let main = let b = system_bytes_zeroed_abort(-1) in owned_bytes_len(b)",
        "trap",
        "rill: runtime trap\n",
        false,
    );
}

#[test]
fn example_runs_through_cli() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/owned/bytes.rill");
    let run = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(source)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"65\n");
}
