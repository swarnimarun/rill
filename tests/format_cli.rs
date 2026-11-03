use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-format-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stdout_and_check_modes_never_change_the_input() {
    let dir = Scratch::new();
    let path = dir.0.join("source with spaces.rill");
    let source = "fn main(){40+2}";
    fs::write(&path, source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"fn main() {\n    40 + 2\n}\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    let check = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .arg("--check")
        .output()
        .unwrap();
    assert!(!check.status.success());
    assert!(check.stdout.is_empty());
    assert!(String::from_utf8_lossy(&check.stderr).contains("E-FMT-CHECK"));
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::write(&path, result.stdout).unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .arg("--check")
        .output()
        .unwrap();
    assert!(check.status.success());
    assert!(check.stdout.is_empty());
    assert!(check.stderr.is_empty());
}

#[test]
fn malformed_invalid_utf8_missing_files_and_bad_options_emit_no_output_or_rewrites() {
    let dir = Scratch::new();
    let path = dir.0.join("source.rill");
    for source in [
        b"fn main(){".as_slice(),
        b"fn main(){/* not supported */42}",
        b"\xff",
    ] {
        fs::write(&path, source).unwrap();
        for options in [
            vec![],
            vec!["--check"],
            vec!["--write"],
            vec!["--check", "--check"],
            vec!["-o", "other.rill"],
        ] {
            let result = Command::new(env!("CARGO_BIN_EXE_rill"))
                .arg("fmt")
                .arg(&path)
                .args(&options)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            assert_eq!(fs::read(&path).unwrap(), source);
        }
    }
    fs::remove_file(&path).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(!path.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

#[test]
fn native_binding_formatting_does_not_require_external_tools_or_imports() {
    let dir = Scratch::new();
    let path = dir.0.join("bindings.rill");
    let source = "@include(\"missing/header.h\");@pkg_config(\"not-installed\");import absent.module;export extern fn f()->unit;";
    fs::write(&path, source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .env("PATH", "")
        .env("CC", "does-not-exist")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        result.stdout,
        rill::formatter::format(source).unwrap().as_bytes()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
}

#[cfg(unix)]
#[test]
fn accepts_non_utf8_source_paths() {
    use std::os::unix::ffi::OsStringExt;
    let dir = Scratch::new();
    let path = dir
        .0
        .join(std::ffi::OsString::from_vec(b"source-\xff.rill".to_vec()));
    fs::write(&path, "fn main(){42}").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("fmt")
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"fn main() {\n    42\n}\n");
}
