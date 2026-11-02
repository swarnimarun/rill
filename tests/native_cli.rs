use std::{
    fs,
    path::Path,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rill native cli {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn source(&self, text: &str) -> std::path::PathBuf {
        let path = self.0.join("main.rill");
        fs::write(&path, text).unwrap();
        path
    }
    fn command(&self, operation: &str, source: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.arg(operation).arg(source).args(["--cc", "cc"]);
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_and_emit_c_reject_header_prototype_mismatches() {
    let f = Fixture::new();
    fs::write(
        f.0.join("api.h"),
        "#include <stdint.h>\n_Bool exact(uint32_t value);\n",
    )
    .unwrap();
    let source =
        f.source("@include(\"api.h\"); extern fn exact(value: u32) -> i32; fn main() { 0 }");
    for operation in ["check", "emit-c"] {
        let output = f
            .command(operation, &source)
            .args(["--c-arg", "-I", "--c-arg"])
            .arg(&f.0)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("incompatible header prototype for exact"));
        assert!(output.stdout.is_empty());
    }
    let source =
        f.source("@include(\"api.h\"); extern fn exact(value: u32) -> bool; fn main() { 0 }");
    for operation in ["check", "emit-c"] {
        let output = f
            .command(operation, &source)
            .args(["--c-arg", "-I", "--c-arg"])
            .arg(&f.0)
            .output()
            .unwrap();
        succeeds(&output);
        if operation == "emit-c" {
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("__builtin_types_compatible_p")
            );
        }
    }
}

#[test]
fn source_include_and_link_select_a_native_library_for_run() {
    let f = Fixture::new();
    fs::write(
        f.0.join("api.h"),
        "#include <stdint.h>\nint32_t fixture_add(int32_t a, int32_t b);\n",
    )
    .unwrap();
    let implementation = f.0.join("fixture.c");
    let object = f.0.join("fixture.o");
    fs::write(
        &implementation,
        "#include \"api.h\"\nint32_t fixture_add(int32_t a, int32_t b) { return a + b; }\n",
    )
    .unwrap();
    succeeds(
        &Command::new("cc")
            .args(["-std=c11", "-c"])
            .arg(&implementation)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    succeeds(
        &Command::new("ar")
            .arg("rcs")
            .arg(f.0.join("libfixture.a"))
            .arg(&object)
            .output()
            .unwrap(),
    );
    let source = f.source("@include(\"api.h\"); @link(\"fixture\"); extern fn fixture_add(a: i32, b: i32) -> i32; fn main() { fixture_add(19, 23) }");
    // The directives select native execution without --native. Each path with
    // spaces remains one compiler/linker argument.
    let output = f
        .command("run", &source)
        .args(["--c-arg", "-I", "--c-arg"])
        .arg(&f.0)
        .arg("--link-arg")
        .arg(format!("-L{}", f.0.display()))
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn native_flag_runs_a_pure_program() {
    let f = Fixture::new();
    let source = f.source("fn main() { 6 * 7 }");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .arg("--native")
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn repeated_c_arguments_preserve_include_paths_with_spaces() {
    let f = Fixture::new();
    let first = f.0.join("first headers");
    let second = f.0.join("second headers");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    fs::write(first.join("api.h"), "#include <stdint.h>\n#include <detail.h>\nstatic int32_t answer(void) { return DETAIL + EXTRA; }\n").unwrap();
    fs::write(second.join("detail.h"), "#define DETAIL 40\n").unwrap();
    let source = f.source("@include(\"api.h\"); extern fn answer() -> i32; fn main() { answer() }");
    let output = f
        .command("run", &source)
        .args(["--c-arg", "-I", "--c-arg"])
        .arg(&first)
        .args(["--c-arg", "-I", "--c-arg"])
        .arg(&second)
        .args(["--c-arg", "-DEXTRA=2"])
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn native_run_propagates_program_and_compiler_failures() {
    let f = Fixture::new();
    fs::write(
        f.0.join("exit.h"),
        "#include <stdlib.h>\nstatic void fail(void) { _Exit(7); }\n",
    )
    .unwrap();
    let source = f.source("@include(\"exit.h\"); extern fn fail() -> unit; fn main() { fail() }");
    let output = f
        .command("run", &source)
        .args(["--c-arg", "-I", "--c-arg"])
        .arg(&f.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("native program failed"), "{error}");
    assert!(error.contains('7'), "{error}");
    assert!(output.stdout.is_empty());

    let source = f.source("fn main() { 42 }");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .arg("--cc")
        .arg(f.0.join("missing compiler"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to start C compiler"));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn source_pkg_config_uses_argument_arrays_and_selects_native_run() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    fs::write(
        f.0.join("api.h"),
        "#include <stdint.h>\nstatic int32_t answer(void) { return 42; }\n",
    )
    .unwrap();
    let tools = f.0.join("fake tools");
    fs::create_dir(&tools).unwrap();
    let pkg = tools.join("pkg-config");
    fs::write(&pkg, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$RILL_PKG_ARGS\"\ncase \"$1\" in\n--cflags) printf '%s\\n' \"$RILL_PKG_CFLAGS\" ;;\n--libs) printf '\\n' ;;\n*) exit 9 ;;\nesac\n").unwrap();
    fs::set_permissions(&pkg, fs::Permissions::from_mode(0o755)).unwrap();
    let recorded = f.0.join("package arguments");
    let source = f.source("@include(\"api.h\"); @pkg_config(\"fixture-package\"); extern fn answer() -> i32; fn main() { answer() }");
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    // No CLI compiler/native flag selects execution. Source imports select it.
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("CC", "cc")
        .env("RILL_PKG_ARGS", &recorded)
        .env("RILL_PKG_CFLAGS", format!("-I'{}'", f.0.display()))
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
    assert_eq!(
        fs::read_to_string(recorded).unwrap(),
        "--cflags\n--\nfixture-package\n--libs\n--\nfixture-package\n"
    );
}

#[cfg(unix)]
#[test]
fn source_links_supplement_combined_packages_without_reordering_raw_link_args() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let tools = f.0.join("tools");
    fs::create_dir(&tools).unwrap();
    let pkg = tools.join("pkg-config");
    fs::write(&pkg, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$RILL_PKG_ARGS\"\ncase \"$1\" in\n--cflags) printf '%s\\n' '-DREQUESTED=1' ;;\n--libs) printf '%s\\n' '-lshared -lcycleA -lcycleB -lcycleA' ;;\n*) exit 9 ;;\nesac\n").unwrap();
    fs::set_permissions(&pkg, fs::Permissions::from_mode(0o755)).unwrap();
    let compiler = tools.join("compiler");
    fs::write(
        &compiler,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RILL_CC_ARGS\"\ncat > /dev/null\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
    let pkg_args = f.0.join("pkg args");
    let cc_args = f.0.join("cc args");
    let source = f.source("@pkg_config(\"first-package\"); @pkg_config(\"second-package\"); @link(\"shared\"); @link(\"shared\"); @link(\"extra\"); @link(\"extra\"); fn main() { 42 }");
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(f.0.join("program"))
        .arg("--cc")
        .arg(&compiler)
        .args(["--link-arg", "-lshared", "--link-arg", "-lshared"])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("RILL_PKG_ARGS", &pkg_args)
        .env("RILL_CC_ARGS", &cc_args)
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(
        fs::read_to_string(pkg_args).unwrap(),
        "--cflags\n--\nfirst-package\nsecond-package\n--libs\n--\nfirst-package\nsecond-package\n"
    );
    let arguments = fs::read_to_string(cc_args).unwrap();
    let arguments: Vec<_> = arguments.lines().collect();
    let link_start = arguments
        .windows(2)
        .position(|a| a == ["-x", "none"])
        .unwrap()
        + 2;
    assert_eq!(
        &arguments[link_start..],
        ["-lshared", "-lcycleA", "-lcycleB", "-lcycleA", "-lextra", "-lshared", "-lshared"]
    );
    assert!(arguments.contains(&"-DREQUESTED=1"));
}
