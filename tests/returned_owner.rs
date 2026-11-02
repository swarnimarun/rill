use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn traced(source: &str, expected: Option<i32>, trace: &str) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    let actual = rill::interpreter::run(&p);
    match expected {
        Some(value) => assert_eq!(actual.unwrap().to_string(), value.to_string()),
        None => assert_eq!(actual.unwrap_err(), "runtime trap"),
    }
    let dir = std::env::temp_dir().join(format!(
        "rill-returned-owner-{}-{}",
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
static void *allocated[8];
static int count;
void *fixture_malloc(size_t size) {
    int n = ++count;
    void *p = malloc(size);
    if (!p) return NULL;
    allocated[n] = p;
    fprintf(stderr, "A%d\n", n);
    return p;
}
void fixture_free(void *p) {
    int n = 1;
    while (n <= count && allocated[n] != p) ++n;
    fprintf(stderr, "F%d\n", n);
    allocated[n] = NULL;
    free(p);
}
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
        let result = Command::new(&binary).output().unwrap();
        match expected {
            Some(value) => {
                assert!(result.status.success());
                assert_eq!(result.stdout, format!("{value}\n").as_bytes());
                assert_eq!(result.stderr, trace.as_bytes());
            }
            None => {
                assert!(!result.status.success());
                assert!(result.stdout.is_empty());
                let err = String::from_utf8(result.stderr).unwrap();
                assert!(err.starts_with(trace), "{err}");
                assert!(err.contains("rill: runtime trap"), "{err}");
                assert!(
                    !err.lines().any(|line| line.starts_with('F')),
                    "abort unexpectedly cleaned up: {err}"
                );
                assert!(!err.contains("A2\n"), "callee should not run: {err}");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn returned_box_survives_callee_cleanup_and_drops_in_caller() {
    traced("let make n = let result = system_box_i32_abort(n) in let scratch = system_box_i32_abort(7) in move result\nlet main = let returned = make(42) in box_read(returned)", Some(42), "A1\nA2\nF2\nF1\n");
}

#[test]
fn returned_parameter_and_tail_calls_transfer_cleanup_once() {
    traced("let pass owner = let inspected = box_read(owner) in let scratch = system_box_i32_abort(7) in move owner\nlet relay owner = pass(move owner)\nlet main = let original = system_box_i32_abort(42) in let returned = relay(move original) in let sibling = system_box_i32_abort(1) in box_read(returned)", Some(42), "A1\nA2\nF2\nA3\nF3\nF1\n");
}

#[test]
fn returning_one_parameter_drops_other_live_parameters() {
    traced("let select returned discarded = let first = box_read(returned) in let second = box_read(discarded) in let scratch = system_box_i32_abort(7) in move returned\nlet main = let first = system_box_i32_abort(42) in let second = system_box_i32_abort(1) in let result = select(move first, move second) in box_read(result)", Some(42), "A1\nA2\nA3\nF3\nF2\nF1\n");
}

#[test]
fn returned_bytes_support_zero_length_mutation_and_unused_cleanup() {
    traced("let make n = let output = system_bytes_zeroed_abort(n) in move output\nlet forward n = make(n)\nlet main = let empty = forward(0) in let mut output = forward(2) in let changed = owned_bytes_set(inout output, 1, 42) in owned_bytes_len(empty) + owned_bytes_get(output, 1)", Some(42), "A1\nA2\nF2\nF1\n");
    traced("let make n = let output = system_box_i32_abort(n) in move output\nlet main = let unused = make(42) in 7", Some(7), "A1\nF1\n");
}

#[test]
fn owner_return_rejects_copies_borrow_escapes_and_unsupported_joins() {
    for source in [
        "let make n = let owner = system_box_i32_abort(n) in owner\nlet main = let result = make(42) in box_read(result)",
        "let escape (read owner) = let inspected = box_read(owner) in move owner\nlet main = let owner = system_box_i32_abort(42) in let escaped = escape(owner) in box_read(escaped)",
        "let escape (inout owner) = let inspected = box_read(owner) in move owner\nlet main = let mut owner = system_box_i32_abort(42) in let escaped = escape(inout owner) in box_read(escaped)",
        "let main = let owner = system_box_i32_abort(42) in move owner",
        "let make n = system_box_i32_abort(n)\nlet main = let result = make(42) in box_read(result)",
        "let choose yes = let a = system_box_i32_abort(1) in let b = system_box_i32_abort(2) in if yes then move a else move b\nlet main = let owner = choose(true) in box_read(owner)",
        "let pass owner = let inspected = box_read(owner) in move owner\nlet main = let source = system_box_i32_abort(42) in let returned = pass(move source) in box_read(source)",
    ] {
        let p = rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&p).is_err(), "accepted {source}");
        assert!(rill::interpreter::run(&p).is_err(), "executed {source}");
        assert!(rill::backend::emit(&p).is_err(), "emitted {source}");
    }
}

#[test]
fn generic_owner_identity_remains_explicitly_unsupported() {
    let source = "let pass owner = move owner\nlet main = let original = system_box_i32_abort(42) in let returned = pass(move original) in box_read(returned)";
    let p = rill::parser::parse(source).unwrap();
    let error = rill::check::check(&p).unwrap_err();
    assert!(error.starts_with("E-OWNED:"), "{error}");
    assert!(error.contains("generic functions"), "{error}");
    assert!(rill::interpreter::run(&p).is_err());
    assert!(rill::backend::emit(&p).is_err());
}

#[test]
fn returned_owners_preserve_abort_policy_and_argument_order() {
    traced(
        "let make n = let owner = system_box_i32_abort(n) in move owner\nlet main = let returned = make(42) in 1 / 0",
        None,
        "A1\n",
    );
    traced(
        "let pass owner n = let inspected = box_read(owner) in let scratch = system_box_i32_abort(n) in move owner\nlet main = let original = system_box_i32_abort(42) in let returned = pass(move original, 1 / 0) in box_read(returned)",
        None,
        "A1\n",
    );
}
