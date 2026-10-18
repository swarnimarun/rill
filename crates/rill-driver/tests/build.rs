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
    // A left-associative chain tries every alternative, including the tail.
    assert!(
        stdout.contains("pick3[0, 0, 9] = 9 pick3[0, 0, 0] = 0"),
        "unexpected stdout: {stdout}"
    );
    // A deciding arm that is not a call: the `fail` inside selects the fallback
    // instead of leaving the function.
    assert!(
        stdout.contains("inline_pick[5] = 5 inline_pick[-1] = 0"),
        "unexpected stdout: {stdout}"
    );
    // An arm that is not taken must not perform its effects: the block arm's
    // print happens only for the call that reaches it.
    assert_eq!(
        stdout.matches("untaken arm ran").count(),
        1,
        "the untaken arm's statement ran: {stdout}"
    );
    assert!(
        stdout.contains("guarded[5] = 5 guarded[-1] = 99"),
        "unexpected stdout: {stdout}"
    );
    // `?` propagates the call's failure to the same fallback.
    assert!(
        stdout.contains("try_pick[5, 0] = 5 try_pick[0, 0] = 0"),
        "unexpected stdout: {stdout}"
    );
    // A deciding *call* nested inside such an arm is caught by the same fallback:
    // `first_positive[0, 0]` fails inside the arm, so the next arm's 7 wins.
    assert!(
        stdout.contains("nested[5] = 5 nested[0] = 7 nested[-1] = 7"),
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

#[test]
fn a_total_choice_failure_exits_one() {
    let dir = workdir("decides_fail");
    let out = dir.join("decides_fail");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/decides_fail.rill")
        .canonicalize()
        .expect("examples/decides_fail.rill")
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
    // Both transfers are underfunded, so the choice cannot produce a value: the
    // statement after it must not run and the exit status must report failure.
    // (A `Some(1)` status also proves it exited, rather than dying on a signal.)
    assert!(
        output.stdout.is_empty(),
        "the unreachable print ran: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn builds_and_runs_transacts() {
    let dir = workdir("transacts");
    let out = dir.join("transacts");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/transacts.rill")
        .canonicalize()
        .expect("examples/transacts.rill")
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
    // The 200 arm debits alice (500 -> 300) and then fails on bob's guard, so
    // the undo trail must restore her before the fallback's 50 is applied:
    // 500 - 50 = 450. Without rollback this reads 250.
    assert!(
        stdout.contains("alice = 450 bob = 150"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_trail_loop() {
    let dir = workdir("trail_loop");
    let out = dir.join("trail_loop");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/trail_loop.rill")
        .canonicalize()
        .expect("examples/trail_loop.rill")
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
    // 5000 speculative regions of two writes each: 10000 records against a
    // 4096-entry trail. Before `rill_txn_end` released the trail at depth zero
    // this aborted; the balances also pin the arithmetic (1 per iteration).
    assert!(
        stdout.contains("alice = 995000 bob = 1005000"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_one_handler() {
    let dir = workdir("one");
    let out = dir.join("one");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/one.rill")
        .canonicalize()
        .expect("examples/one.rill")
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
    // `one` takes the first successful arm (7), then a later one (9), and when
    // every arm fails the enclosing `| 5` catches it.
    assert!(stdout.contains("one: 7 9 5"), "unexpected stdout: {stdout}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn one_handler_with_no_success_exits_one() {
    let dir = workdir("one_fail");
    let out = dir.join("one_fail");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/one_fail.rill")
        .canonicalize()
        .expect("examples/one_fail.rill")
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
    // Every arm fails, nothing handles it: no output, and a clean exit 1.
    assert!(
        output.stdout.is_empty(),
        "the unreachable print ran: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn builds_and_runs_option_handler() {
    let dir = workdir("option");
    let out = dir.join("option");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/option.rill")
        .canonicalize()
        .expect("examples/option.rill")
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
    // The handler is a speculative region: a body that debits and then fails
    // must leave the balance as it was (50, not 0).
    assert!(
        stdout.contains("balance after a failed transfer 50"),
        "the failed body's write was not rolled back: {stdout}"
    );
    // `when (cond)` guards the rest of its block: the print happens only for the
    // call where the condition holds.
    assert_eq!(
        stdout.matches("positive").count(),
        1,
        "the guard did not skip the rest of the block: {stdout}"
    );
    assert!(
        stdout.contains("guarded[5] = 5 guarded[0] = 0"),
        "unexpected stdout: {stdout}"
    );
    // A body with statements runs under the handler's continuation: the print
    // after the failed binding must not run, so it appears exactly once.
    assert_eq!(
        stdout.matches("after the binding").count(),
        1,
        "the statement after a failed binding ran: {stdout}"
    );
    assert!(
        stdout.contains("noisy[5] = 5 noisy[0] = 0"),
        "unexpected stdout: {stdout}"
    );
    // `option { first_positive[x, y] }? | 0`: a present value, then an absent one
    // that the fallback catches.
    assert!(
        stdout.contains("maybe[5, 0] = 5 maybe[0, 0] = 0"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_sync_composition() {
    let dir = workdir("sync");
    let out = dir.join("sync");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sync.rill")
        .canonicalize()
        .expect("examples/sync.rill")
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
    // Several valued arms form a tuple, read by field access.
    assert!(
        stdout.contains("sync tuple 40,2"),
        "unexpected stdout: {stdout}"
    );
    // Arms with no suspension point run in order and join.
    assert!(
        stdout.contains("step first\nstep second\njoined"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_let_try() {
    let dir = workdir("let_try");
    let out = dir.join("let_try");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/let_try.rill")
        .canonicalize()
        .expect("examples/let_try.rill")
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
    // Two `let?` bindings: both succeed (3 + 4), then the first fails and the
    // failure leaves `sum_two` for the caller's fallback.
    assert!(
        stdout.contains("sum_two[3, 4] = 7 sum_two[0, 4] = 0"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_all_handler() {
    let dir = workdir("all");
    let out = dir.join("all");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/all.rill")
        .canonicalize()
        .expect("examples/all.rill")
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
    // Two of three arms succeed, so the slice has two elements; when none
    // succeeds it is empty rather than absent.
    assert!(
        stdout.contains("found 2 none 0"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_layers() {
    let dir = workdir("layers");
    let out = dir.join("layers");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/layers.rill")
        .canonicalize()
        .expect("examples/layers.rill")
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
    // A handler body that allocates and then fails hands the arena back: the
    // watermark after the failed region equals the one before it.
    let after = stdout
        .split("after a failed region ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse::<i64>().ok())
        .unwrap_or_else(|| panic!("no post-region figure in {stdout}"));
    let before = stdout
        .split("(before ")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<i64>().ok())
        .unwrap_or_else(|| panic!("no pre-region figure in {stdout}"));
    assert_eq!(
        before, after,
        "the failed region kept its allocations: {stdout}"
    );
    // The collector found two values, and its storage came from the bound arena:
    // the exact byte count carries allocator overhead, but zero would mean the
    // heap path was taken.
    assert!(stdout.contains("found 2"), "unexpected stdout: {stdout}");
    let used: i64 = stdout
        .split("arena used ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no arena figure in {stdout}"));
    assert!(used > 0, "the arena was not used: {stdout}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_showcase() {
    let dir = workdir("showcase");
    let out = dir.join("showcase");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/showcase.rill")
        .canonicalize()
        .expect("examples/showcase.rill")
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
    // The §11 result: the refused 800-transfer is rolled back and the 200 lands.
    assert!(
        stdout.contains("alice 300 bob 300"),
        "unexpected stdout: {stdout}"
    );
    // `option` with `?`, a handler guard, the arena layer, valued `sync` and
    // tuple field access, in one program.
    assert!(stdout.contains("picked 7 and 0"), "{stdout}");
    assert!(stdout.contains("guard held"), "{stdout}");
    assert!(stdout.contains("guarded 8"), "{stdout}");
    assert!(stdout.contains("pairs 2 arena 12"), "{stdout}");
    assert!(stdout.contains("sync 42 tuple 42,43"), "{stdout}");
    // `main` returns alice's balance, truncated to the process exit status.
    assert_eq!(output.status.code(), Some(44));
}

#[test]
fn every_example_builds_and_exits() {
    // The showcase sat broken for most of this project because nothing built the
    // examples as a set. Individual tests pin their output; this one makes sure
    // no example can rot into something that does not compile or does not exit
    // cleanly, without pinning what each prints.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    let mut failures: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("examples directory") {
        let path = entry.expect("example entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rill") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let out = workdir(&format!("corpus_{name}")).join(&name);
        let status = Command::new(env!("CARGO_BIN_EXE_rillc"))
            .arg("build")
            .arg(&path)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run rillc build");
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            let first = stderr.lines().next().unwrap_or("(no message)").to_string();
            failures.push(format!("{name}: {first}"));
            continue;
        }

        let run = Command::new(&out)
            .output()
            .expect("run the produced binary");
        assert!(
            run.status.code().is_some(),
            "examples/{name}.rill died on a signal: {:?}",
            run.status
        );
        checked += 1;
    }
    assert!(
        checked >= 20,
        "expected the corpus, found {checked} examples"
    );
    assert!(
        failures.is_empty(),
        "examples that do not build:\n{}",
        failures.join("\n")
    );
}

#[test]
fn builds_and_runs_captures() {
    let dir = workdir("captures");
    let out = dir.join("captures");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/captures.rill")
        .canonicalize()
        .expect("examples/captures.rill")
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
    // The closure captures `n := 7` and is passed to another function.
    assert!(
        stdout.contains("captured 42"),
        "unexpected stdout: {stdout}"
    );
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn builds_and_runs_dyn() {
    let dir = workdir("dyn");
    let out = dir.join("dyn");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/dyn.rill")
        .canonicalize()
        .expect("examples/dyn.rill")
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
    // One call site, two implementations: the vtable decides.
    assert!(stdout.contains("point rect"), "unexpected stdout: {stdout}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn builds_and_runs_executor() {
    let dir = workdir("executor");
    let out = dir.join("executor");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/executor.rill")
        .canonicalize()
        .expect("examples/executor.rill")
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
    // Four tasks: the third spawns the fourth *during* the run, so the loop picks
    // it up - that is what makes this a scheduler rather than a sequence of calls.
    assert!(stdout.contains("task one"), "{stdout}");
    assert!(stdout.contains("task three spawns another"), "{stdout}");
    assert!(stdout.contains("task four"), "{stdout}");
    assert!(stdout.contains("ran 4"), "{stdout}");
    assert_eq!(output.status.code(), Some(0));
}
