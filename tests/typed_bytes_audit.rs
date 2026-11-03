use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn compare_programs(source: &str, fallback: &str) {
    let typed_program = rill::parser::parse(source).unwrap();
    let reference = rill::interpreter::run_reference(&typed_program);
    assert_eq!(rill::interpreter::run(&typed_program), reference);
    let fallback_program = rill::parser::parse(fallback).unwrap();
    // The unused raw-C value forces the uniform native emitter. Raw C values
    // deliberately have no interpreter support; compare its C to the checked
    // AST oracle for the unmodified, semantically equivalent Copy program.
    assert!(rill::interpreter::run(&fallback_program)
        .unwrap_err()
        .contains("E-FFI"));
    let typed = rill::backend::emit(&typed_program).unwrap();
    let fallback = rill::backend::emit(&fallback_program).unwrap();
    assert!(
        !typed.contains("rl_value"),
        "expected typed backend for {source}"
    );
    assert!(
        fallback.contains("rl_value"),
        "expected fallback backend for {source}"
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-bytes-audit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let src = dir.join("main.c");
    let exe = dir.join("main");
    for c in [typed, fallback] {
        fs::write(&src, c).unwrap();
        for opt in ["-O0", "-O2"] {
            let out = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&src)
                .arg("-o")
                .arg(&exe)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let out = Command::new(&exe).output().unwrap();
            match &reference {
                Ok(value) => {
                    assert!(out.status.success(), "{source}");
                    assert_eq!(out.stdout, format!("{value}\n").as_bytes(), "{source}");
                }
                Err(_) => {
                    assert!(!out.status.success(), "{source}");
                    assert_eq!(out.stderr, b"rill: runtime trap\n", "{source}");
                }
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn byte_buffers_match_ast_and_native_fallback_at_boundaries() {
    for expr in [
        "bytes_new(0)",
        "bytes_len(bytes_new(0))",
        "bytes_get(bytes_new(0), 0)",
        "bytes_get(bytes_new(256), 255)",
        "bytes_get(bytes_new(256), 256)",
        "bytes_set(bytes_new(1), 0, -1)",
        "bytes_set(bytes_new(1), 0, 256)",
        "bytes_get(bytes_set(bytes_new(256), 255, 255), 255)",
        "let mut b = bytes_new(2) in let old = b in let changed = b <- bytes_set(b, 1, 42) in bytes_get(old, 1) + bytes_get(b, 1)",
        "let mut b = bytes_new(1) in bytes_get(bytes_set(b, (let changed = b <- bytes_set(b, 0, 1) in 0), 2), 0)",
    ] {
        compare_programs(&format!("let main = {expr}"),&format!("let main = let marker = u32(0) in {expr}"));
    }
}
#[test]
fn borrowed_buffer_snapshot_is_independent_of_later_mutation() {
    let source="let identity (read x) = x\nlet main = let mut b = bytes_new(4) in let before = identity(b) in let changed = b <- bytes_set(b, 0, 10) in bytes_get(before, 0) + bytes_get(b, 0)";
    let value_source = "let main = let mut b = bytes_new(4) in let before = b in let changed = b <- bytes_set(b, 0, 10) in bytes_get(before, 0) + bytes_get(b, 0)";
    let fallback = source.replace("let main =", "let main = let marker = u32(0) in");
    compare_programs(value_source, &fallback);
    compare_programs(source, &fallback);
}
