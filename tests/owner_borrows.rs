use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn borrow_example_runs_through_cli() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/owned/borrow.rill");
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

fn traced(source: &str, expected: i32, trace: &str) {
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(
        rill::interpreter::run(&p).unwrap().to_string(),
        expected.to_string()
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-owner-borrow-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let fixture = dir.join("fixture.c");
    let object = dir.join("program.o");
    let binary = dir.join("program");
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
    fs::write(&fixture,r#"#include <stdio.h>
#include <stdlib.h>
static void *allocated[8]; static int count;
void *fixture_malloc(size_t size) { int n=++count; void *p=malloc(size); if (!p) return NULL; allocated[n]=p; fprintf(stderr,"A%d\n",n); return p; }
void fixture_free(void *p) { int n=1; while(n<=count && allocated[n]!=p) ++n; fprintf(stderr,"F%d\n",n); free(p); }
int rl_generated_main(void);
int main(void) { return rl_generated_main(); }
"#).unwrap();
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
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
        assert_eq!(run.stderr, trace.as_bytes());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn owner_borrows_forward_without_transferring_cleanup() {
    for (source,expected,trace) in [
  ("let twice (read a) (read b) = box_read(a) + box_read(b)\nlet main = let x = system_box_i32_abort(21) in twice(x, x)",42,"A1\nF1\n"),
  ("let get (read x) = box_read(x)\nlet update (inout x) = let value = get(x) in box_write(inout x, value + 1)\nlet forward (inout x) = update(inout x)\nlet main = let mut x = system_box_i32_abort(41) in let done = forward(inout x) in box_read(x)",42,"A1\nF1\n"),
  ("let set (read x) (inout y) = box_write(inout y, box_read(x))\nlet main = let a = system_box_i32_abort(42) in let mut b = system_box_i32_abort(0) in let done = set(a, inout b) in box_read(b)",42,"A1\nA2\nF2\nF1\n"),
  ("let inspect (read x) = box_read(x)\nlet consume x = box_read(x)\nlet main = let b = system_box_i32_abort(42) in let seen = inspect(b) in consume(move b)",42,"A1\nF1\n"),
  ("let observe (read x) n = box_read(x) + n\nlet main = let x = system_box_i32_abort(41) in observe(x, (let x = system_box_i32_abort(1) in box_read(x)))",42,"A1\nA2\nF2\nF1\n"),
 ] {traced(source,expected,trace);}
}

#[test]
fn owner_borrow_conflicts_and_escapes_are_rejected() {
    for source in [
  "let f (read a) (inout b) = let n = box_read(a) in box_write(inout b, n)\nlet main = let mut x = system_box_i32_abort(1) in f(x, inout x)",
  "let f (inout a) (read b) = let n = box_read(b) in let done = box_write(inout a, n) in n\nlet main = let mut x = system_box_i32_abort(1) in f(inout x, x)",
  "let f (read a) n = box_read(a) + n\nlet main = let mut x = system_box_i32_abort(1) in f(x, (let done = box_write(inout x, 2) in 0))",
  "let consume x = box_read(x)\nlet observe (read x) n = box_read(x) + n\nlet main = let x = system_box_i32_abort(1) in observe(x, consume(move x))",
  "let main = let mut x = system_box_i32_abort(1) in box_write(inout x, box_read(x))",
  "let bad (read x) = let y = move x in box_read(y)\nlet main = let x = system_box_i32_abort(1) in bad(x)",
  "let bad (read x) = let y = x in box_read(y)\nlet main = let x = system_box_i32_abort(1) in bad(x)",
  "let consume x = box_read(x)\nlet bad (read x) = consume(move x)\nlet main = let x = system_box_i32_abort(1) in bad(x)",
  "let bad (inout x) = let y = move x in box_read(y)\nlet main = let mut x = system_box_i32_abort(1) in bad(inout x)",
  "let bad (read x) = let seen = box_read(x) in x\nlet main = let x = system_box_i32_abort(1) in bad(x)",
  "let bad (read x) = box_write(inout x, 2)\nlet main = let x = system_box_i32_abort(1) in bad(x)",
  "let get (read x) = box_read(x)\nlet main = get(system_box_i32_abort(1))",
 ] {
  let p=rill::parser::parse(source).unwrap();
  assert!(rill::check::check(&p).is_err(),"accepted {source}");
  assert!(rill::interpreter::run(&p).is_err()); assert!(rill::backend::emit(&p).is_err());
 }
}
