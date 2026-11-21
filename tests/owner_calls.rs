use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn transfer_example_runs_through_cli() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/owned/transfer.rill");
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"42\n");
}

fn traced(source: &str, expected: Option<i32>, trace: &str) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    let actual = rill::interpreter::run(&p);
    match expected {
        Some(value) => assert_eq!(actual.unwrap().to_string(), value.to_string()),
        None => assert_eq!(actual.unwrap_err(), "runtime trap"),
    }
    let dir = std::env::temp_dir().join(format!(
        "rill-owner-call-{}-{}",
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
                assert!(!err.contains("A2\n"), "callee should not run: {err}");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn owner_parameters_drop_locals_then_parameters_in_reverse_order() {
    let source="let consume a b = let local = system_box_i32_abort(7) in box_read(a) + box_read(b) + box_read(local)\nlet main = let a = system_box_i32_abort(1) in let b = system_box_i32_abort(2) in consume(move a, move b)";
    traced(source, Some(10), "A1\nA2\nA3\nF3\nF2\nF1\n");
}
#[test]
fn nested_forwarding_transfers_one_cleanup_obligation() {
    let source="let leaf x = box_read(x)\nlet relay y = leaf(move y)\nlet main = let a = system_box_i32_abort(42) in relay(move a)";
    traced(source, Some(42), "A1\nF1\n");
}

#[test]
fn consumed_parameter_can_be_moved_to_mutable_local() {
    let source = "let update p = let mut local = move p in let done = box_write(inout local, 42) in box_read(local)\nlet main = let a = system_box_i32_abort(1) in update(move a)";
    traced(source, Some(42), "A1\nF1\n");
}
#[test]
fn value_read_before_move_is_allowed_but_later_trap_prevents_call() {
    traced("let add n x = n + box_read(x)\nlet main = let a = system_box_i32_abort(21) in add(box_read(a), move a)",Some(42),"A1\nF1\n");
    traced("let consume x n = let extra = system_box_i32_abort(1) in box_read(x) + n + box_read(extra)\nlet main = let a = system_box_i32_abort(42) in consume(move a, 1 / 0)",None,"A1\n");
}
#[test]
fn owner_arguments_reject_copy_double_move_and_later_use() {
    for source in [
      "let consume x = box_read(x)\nlet main = let a = system_box_i32_abort(42) in consume(a)",
      "let consume x y = box_read(x) + box_read(y)\nlet main = let a = system_box_i32_abort(21) in consume(move a, move a)",
      "let consume x n = box_read(x) + n\nlet main = let a = system_box_i32_abort(42) in consume(move a, box_read(a))",
      "let consume x = box_read(x)\nlet main = let a = system_box_i32_abort(42) in let v = consume(move a) in box_read(a)",
    ] {
      let program=rill::parser::parse(source).unwrap();
      assert!(rill::check::check(&program).is_err(),"accepted {source}");
      assert!(rill::interpreter::run(&program).is_err());
      assert!(rill::backend::emit(&program).is_err());
    }
}
