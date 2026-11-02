use std::{fs, process::Command};

#[test]
fn build_creates_a_working_executable_and_rejects_bad_source() {
    let dir = std::env::temp_dir().join(format!("rill-cli-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("program.rill");
    let output = dir.join("program");
    fs::write(&source, "let main = 6 * 7").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_rill"))
        .args([
            "build",
            source.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let result = Command::new(&output).output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"42\n");
    fs::write(&source, "let main = true + 1").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .args([
            "build",
            source.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn build_passes_compiler_and_linker_options_without_a_shell() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rill-cli-options-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("program.rill");
    let compiler = dir.join("fake compiler");
    let recorded = dir.join("arguments");
    let input = dir.join("input");
    fs::write(&source, "let main = 42").unwrap();
    fs::write(
        &compiler,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RILL_TEST_ARGS\"\ncat > \"$RILL_TEST_INPUT\"\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(&source)
        .arg("--cc")
        .arg(&compiler)
        .args(["--link-arg", "-Ldir with spaces", "--link-arg", "-lmath"])
        .arg("-o")
        .arg(dir.join("output"))
        .env("RILL_TEST_ARGS", &recorded)
        .env("RILL_TEST_INPUT", &input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let args = fs::read_to_string(&recorded).unwrap();
    assert!(args.contains("-x\nnone\n-Ldir with spaces\n"));
    assert!(args.ends_with("-Ldir with spaces\n-lmath\n"));
    assert!(fs::read_to_string(&input)
        .unwrap()
        .contains("int main(void)"));

    for (mode, has_neon_define) in [("scalar", false), ("neon", true)] {
        let result = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(&source)
            .arg("--cc")
            .arg(&compiler)
            .arg("--simd")
            .arg(mode)
            .arg("-o")
            .arg(dir.join("output"))
            .env("RILL_TEST_ARGS", &recorded)
            .env("RILL_TEST_INPUT", &input)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read_to_string(&recorded)
                .unwrap()
                .lines()
                .any(|arg| arg == "-DRILL_SIMD_NEON=1"),
            has_neon_define
        );
    }
    for args in [
        &["--simd", "unknown"][..],
        &["--simd", "neon", "--simd", "scalar"][..],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(&source)
            .args(args)
            .arg("-o")
            .arg(dir.join("output"))
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("--simd"));
    }

    let zig = dir.join("zig");
    fs::copy(&compiler, &zig).unwrap();
    fs::set_permissions(&zig, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(&source)
        .arg("--zig-cc")
        .arg("-o")
        .arg(dir.join("output"))
        .env("PATH", path)
        .env("RILL_TEST_ARGS", &recorded)
        .env("RILL_TEST_INPUT", &input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::read_to_string(&recorded).unwrap().starts_with("cc\n"));

    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(&source)
        .arg("--zig-cc")
        .arg("--cc")
        .arg(&compiler)
        .arg("-o")
        .arg(dir.join("output"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be combined"));
    fs::remove_dir_all(dir).unwrap();
}
