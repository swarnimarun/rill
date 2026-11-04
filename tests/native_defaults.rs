use std::{fs, path::PathBuf, process::Command};

struct Directory(PathBuf);
impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rill-defaults-{}-{name}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn run_defaults_to_native_and_interpret_is_an_explicit_escape() {
    let directory = Directory::new("run");
    let source = directory.0.join("main.rill");
    fs::write(&source, "fn main() { 40 + 2 }").unwrap();
    let missing = directory.0.join("missing-c-compiler");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .env("CC", &missing)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to start C compiler"));
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .arg("--interpret")
        .env("CC", &missing)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn native_default_and_explicit_optimization_levels_preserve_results() {
    let directory = Directory::new("levels");
    let source = directory.0.join("main.rill");
    fs::write(
        &source,
        "fn main() { let mut n = 0; while n < 42 { n <- n + 1; }; n }",
    )
    .unwrap();
    for level in [None, Some("0"), Some("2"), Some("3"), Some("s"), Some("z")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.arg("run").arg(&source);
        if let Some(level) = level {
            command.args(["--opt-level", level]);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{level:?}: {:?}", output.stderr);
        assert_eq!(output.stdout, b"42\n", "{level:?}");
    }
}

#[test]
fn invalid_or_conflicting_native_options_fail_before_execution() {
    let directory = Directory::new("invalid");
    let source = directory.0.join("main.rill");
    fs::write(&source, "fn main() { 42 }").unwrap();
    for args in [
        vec!["--opt-level", "fast"],
        vec!["--opt-level", "2", "--opt-level", "3"],
        vec!["--interpret", "--native"],
        vec!["--interpret", "--opt-level", "0"],
        vec!["--interpret", "--cc", "cc"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("run")
            .arg(&source)
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn interpret_rejects_native_sources_before_running_dependency_tools() {
    let directory = Directory::new("interpret-native");
    let source = directory.0.join("main.rill");
    fs::write(&source, "@include(\"missing-api.h\"); @pkg_config(\"rill-no-such-native-package\"); fn main() { missing_api() }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&source)
        .arg("--interpret")
        .env("PATH", "")
        .env("CC", "rill-no-such-c-compiler")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--interpret cannot execute native dependencies"),
        "{:?}",
        output.stderr
    );
}

#[cfg(unix)]
#[test]
fn native_compiler_receives_o3_by_default_and_explicit_arguments_follow_it() {
    use std::os::unix::fs::PermissionsExt;
    let directory = Directory::new("compiler-arguments");
    let source = directory.0.join("main.rill");
    fs::write(&source, "fn main() { 42 }").unwrap();
    let compiler = directory.0.join("recording compiler");
    fs::write(
        &compiler,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RILL_TEST_ARGUMENT_LOG\"\nexec cc \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o700)).unwrap();
    let log = directory.0.join("arguments");
    for raw in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .arg("run")
            .arg(&source)
            .arg("--cc")
            .arg(&compiler)
            .env("RILL_TEST_ARGUMENT_LOG", &log);
        if raw {
            command.args(["--c-arg", "-O0"]);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert_eq!(output.stdout, b"42\n");
        let arguments = fs::read_to_string(&log).unwrap();
        let args: Vec<_> = arguments.lines().collect();
        let default = args.iter().position(|arg| *arg == "-O3").unwrap();
        if raw {
            assert!(args.iter().position(|arg| *arg == "-O0").unwrap() > default);
        } else {
            assert_eq!(args.iter().filter(|arg| arg.starts_with("-O")).count(), 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn saved_native_configurations_own_execution_and_optimization() {
    let directory = Directory::new("configured-options");
    let source = directory.0.join("main.rill");
    fs::write(&source, "fn main() { 42 }").unwrap();
    for extra in [vec!["--opt-level", "3"], vec!["--interpret"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("run")
            .arg(&source)
            .args(["--native-config", "missing.rillcfg", "--cc", "cc"])
            .args(extra)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("cannot be combined"), "{error}");
    }
}
