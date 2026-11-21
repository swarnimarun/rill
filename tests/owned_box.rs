use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn runnable_example_prints_42() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/owned/box.rill");
    let run = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"42\n");
}

#[test]
fn ownership_checks_reject_aliases_escapes_and_use_after_move() {
    for source in [
        "let main = let x = system_box_i32_abort(1) in let y = move x in box_read(x)",
        "let main = let x = system_box_i32_abort(1) in let y = x in box_read(y)",
        "let main = let x = system_box_i32_abort(1) in box_write(inout x, 2)",
        "let main = let x = system_box_i32_abort(1) in x",
        "let main = system_box_i32_abort(1)",
        "let main = let x = system_box_i32_abort(1) in let y = move x in let z = move x in box_read(y)",
        "let main = let x = system_box_i32_abort(1) in let y = if true then move x else move x in box_read(x)",
        "let main = let x = system_box_i32_abort(1) in let ignored = while false do (let y = move x in ()) in box_read(x)",
        "let main = let x = system_box_i32_abort(1) in false && (let y = move x in true)",
        "let identity x = x\nlet main = let x = system_box_i32_abort(1) in identity(x)",
        "type Box<a> = Box(a)\nlet main = let x = system_box_i32_abort(1) in Box(x)",
    ] {
        let program=rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&program).is_err(),"accepted {source}");
        assert!(rill::interpreter::run(&program).is_err(),"interpreted {source}");
        assert!(rill::backend::emit(&program).is_err(),"emitted {source}");
    }
}

#[test]
fn normal_cleanup_is_once_reverse_order_and_oom_traps() {
    let source="let main = let a = system_box_i32_abort(20) in let b = system_box_i32_abort(22) in let moved = move a in box_read(moved) + box_read(b)";
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
    let c = rill::backend::emit(&program).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "rill-owned-box-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("generated.c");
    let fixture = dir.join("fixture.c");
    let object = dir.join("generated.o");
    let binary = dir.join("program");
    fs::write(&cfile, c).unwrap();
    fs::write(
        &fixture,
        r#"#include <stdio.h>
#include <stdlib.h>
static void *allocated[8];
static int count;
void *fixture_malloc(size_t size) {
    int n = ++count;
    if (getenv("RILL_FAIL_FIRST") && n == 1) { fputs("A1-fail\n", stderr); return NULL; }
    if (getenv("RILL_FAIL_SECOND") && n == 2) { fputs("A2-fail\n", stderr); return NULL; }
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
            .arg(&cfile)
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
        assert!(run.status.success());
        assert_eq!(run.stdout, b"42\n");
        assert_eq!(run.stderr, b"A1\nA2\nF1\nF2\n");
        let oom = Command::new(&binary)
            .env("RILL_FAIL_SECOND", "1")
            .output()
            .unwrap();
        assert!(!oom.status.success());
        assert!(oom.stdout.is_empty());
        assert_eq!(oom.stderr, b"A1\nA2-fail\nrill: runtime trap\n");
        let oom_first = Command::new(&binary)
            .env("RILL_FAIL_FIRST", "1")
            .output()
            .unwrap();
        assert!(!oom_first.status.success());
        assert!(oom_first.stdout.is_empty());
        assert_eq!(oom_first.stderr, b"A1-fail\nrill: runtime trap\n");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mutable_owned_box_read_and_write_match_generated_c() {
    let source="let main = let mut x = system_box_i32_abort(41) in let done = box_write(inout x, 42) in box_read(x)";
    let p = rill::parser::parse(source).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(rill::interpreter::run(&p).unwrap().to_string(), "42");
    let dir = std::env::temp_dir().join(format!(
        "rill-owned-write-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&cfile, rill::backend::emit(&p).unwrap()).unwrap();
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
