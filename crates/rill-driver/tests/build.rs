//! End-to-end: `rillc build` produces a binary that runs, plus the static
//! artifact when `zig` is available.

use std::path::PathBuf;
use std::process::Command;

fn example() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fib.rill")
        .canonicalize()
        .expect("examples/fib.rill")
        .to_string_lossy()
        .to_string()
}

fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rill-e2e-{}-{tag}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn builds_and_runs_a_native_binary() {
    let dir = workdir("native");
    let out = dir.join("fib");
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example(), "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "rillc build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fib(10) = 55"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(55),
        "the program's value is the process exit status"
    );
}

#[test]
fn builds_a_statically_linked_elf_when_zig_is_available() {
    if Command::new("zig")
        .arg("version")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("skipping: zig is not installed");
        return;
    }
    let dir = workdir("static");
    let out = dir.join("fib-static");
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example(), "-o"])
        .arg(&out)
        .args(["--target", "linux-musl"])
        .status()
        .expect("run rillc build --target linux-musl");
    assert!(status.success(), "static build failed: {status}");

    let description = Command::new("file")
        .arg(&out)
        .output()
        .expect("run file(1)");
    let text = String::from_utf8_lossy(&description.stdout);
    assert!(
        text.contains("statically linked"),
        "expected a static ELF, got: {text}"
    );
    assert!(text.contains("ELF"), "expected an ELF, got: {text}");
}
