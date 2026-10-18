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

#[test]
fn builds_and_runs_early_exit_loops() {
    let dir = workdir("loops");
    let out = dir.join("counter");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.rill")
        .canonicalize()
        .expect("examples/counter.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `count_while 7` = 7; `count_for 100` breaks at i = 5 after 0+1+2+3+4 = 10.
    assert!(
        stdout.contains("while=7 for=10"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(17));
}

#[test]
fn builds_and_runs_a_growable_buffer_program() {
    let dir = workdir("alloc");
    let out = dir.join("alloc");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/alloc.rill")
        .canonicalize()
        .expect("examples/alloc.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("len 10"), "unexpected stdout: {stdout}");
    assert!(
        stdout.contains("text A,B,C,D,E,"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(10));
}

#[test]
fn allocation_outside_declared_effects_is_rejected() {
    let dir = workdir("alloc-effect");
    let path = dir.join("pure.rill");
    std::fs::write(
        &path,
        "use std::io;\n\
         let f () -> i32 <computes> = { var b = buf_new (); buf_len (b) as i32 };\n\
         let main () -> i32 <io> = f ();\n",
    )
    .expect("write fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .arg("build")
        .arg(&path)
        .arg("-o")
        .arg(dir.join("pure"))
        .output()
        .expect("run rillc build");
    assert!(!output.status.success(), "must reject undeclared `<alloc>`");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("performs alloc"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn builds_and_runs_static_trait_dispatch() {
    let dir = workdir("traits");
    let out = dir.join("traits");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traits.rill")
        .canonicalize()
        .expect("examples/traits.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("p Point(3, 4) r Rect(6x7)"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn builds_and_calls_libc_through_ffi() {
    let dir = workdir("ffi");
    let out = dir.join("ffi");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/ffi.rill")
        .canonicalize()
        .expect("examples/ffi.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `puts` writes its own line; `strlen` measures the greeting.
    assert!(
        stdout.contains("hello from libc"),
        "unexpected stdout: {stdout}"
    );
    assert!(
        stdout.contains("strlen = 15"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(15));
}

#[test]
fn builds_and_runs_layered_arenas() {
    let dir = workdir("arena");
    let out = dir.join("arena");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/arena.rill")
        .canonicalize()
        .expect("examples/arena.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A mark records the watermark; resetting to it frees the layer, so the
    // arena ends empty and each layer's cost is reported.
    assert!(
        stdout.contains("first 100 second 64 used 0"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(36));
}

#[test]
fn builds_and_runs_partial_application() {
    let dir = workdir("currying");
    let out = dir.join("currying");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/currying.rill")
        .canonicalize()
        .expect("examples/currying.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("inc 41 = 42"),
        "unexpected stdout: {stdout}"
    );
    assert!(
        stdout.contains("triple 14 = 42"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn builds_and_runs_place_projections() {
    let dir = workdir("projections");
    let out = dir.join("projections");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/projections.rill")
        .canonicalize()
        .expect("examples/projections.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Two deposits through the projection: 100 + 20 + 22.
    assert!(
        stdout.contains("balance 142"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(142));
}

#[test]
fn builds_and_runs_adjacent_calls() {
    let dir = workdir("adjacent_calls");
    let out = dir.join("adjacent_calls");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/adjacent_calls.rill")
        .canonicalize()
        .expect("examples/adjacent_calls.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `add[20, 1] = 21 twice[10] = 20`, and the exit code is 21 + 20 - 21.
    assert!(
        stdout.contains("add[20, 1] = 21 twice[10] = 20"),
        "stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(20));
}

#[test]
fn builds_and_runs_decides() {
    let dir = workdir("decides");
    let out = dir.join("decides");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/decides.rill")
        .canonicalize()
        .expect("examples/decides.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `first_positive[x, y] | 0`: success, fallback, and failure-then-fallback.
    assert!(
        stdout.contains("pick[5, 0] = 5 pick[0, 7] = 7 pick[0, 0] = 0"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(57));
}

#[test]
fn builds_and_runs_transfer() {
    let dir = workdir("transfer");
    let out = dir.join("transfer");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/transfer.rill")
        .canonicalize()
        .expect("examples/transfer.rill")
        .to_string_lossy()
        .to_string();
    let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build", &example, "-o"])
        .arg(&out)
        .status()
        .expect("run rillc build");
    assert!(status.success(), "build failed: {status}");

    let output = Command::new(&out)
        .output()
        .expect("run the produced binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The 800 arm fails its guard; the `|` fallback performs 200. `main` is
    // `<decides>`; both arms' failure would exit 1, so 0 proves it succeeded.
    assert!(
        stdout.contains("Alice: 300 Bob: 300"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}
