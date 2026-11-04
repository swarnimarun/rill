use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill native validation {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("api.h"),
            "#include <stdint.h>\n_Bool exact(uint32_t value);\n",
        )
        .unwrap();
        Self(path)
    }

    fn check(&self, operation: &str, result: &str, flags: &[&str]) -> Output {
        let source = self.0.join("main.rill");
        fs::write(
            &source,
            format!(
                "@include(\"api.h\"); extern fn exact(value: u32) -> {result}; fn main() {{ 0 }}"
            ),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .arg(operation)
            .arg(source)
            .args(["--cc", "cc", "--c-arg", "-I", "--c-arg"])
            .arg(&self.0);
        for flag in flags {
            command.args(["--c-arg", flag]);
        }
        command.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_validation_rejects_compiler_action_overrides() {
    let fixture = Fixture::new();
    for flags in [
        vec!["-E"],
        vec!["-M"],
        vec!["-MM"],
        vec!["--preprocess"],
        vec!["--dependencies"],
        vec!["--user-dependencies"],
        vec!["-S"],
        vec!["-c"],
        vec!["-fsyntax-only"],
        vec!["-x", "assembler"],
        vec!["-xc++"],
        vec!["-std=c++17"],
        vec!["--std=c99"],
        vec!["--help"],
        vec!["-help"],
        vec!["--version"],
        vec!["-version"],
        vec!["-ccc-print-phases"],
        vec!["-ccc-print-bindings"],
        vec!["--precompile"],
        vec!["-precompile"],
        vec!["--autocomplete=-E"],
        vec!["-###"],
        vec!["-fdriver-only"],
        vec!["-Xclang", "-E"],
        vec!["-Xpreprocessor", "-M"],
        vec!["-Wp,-M"],
        vec!["--driver-mode=cpp"],
        vec!["-cc1"],
        vec!["@compiler-options"],
        vec!["--config=compiler-options"],
        vec!["extra.c"],
        vec!["--"],
    ] {
        for operation in ["check", "emit-c"] {
            let output = fixture.check(operation, "i32", &flags);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{operation} {flags:?}: {stderr}");
            assert!(
                stderr.contains("E-C-IMPORT: unsupported validation C argument"),
                "{operation} {flags:?}: {stderr}"
            );
            assert!(output.stdout.is_empty(), "{operation} {flags:?}");
        }
    }
}

#[test]
fn builds_reject_actions_that_do_not_produce_executables() {
    let fixture = Fixture::new();
    let source = fixture.0.join("pure.rill");
    fs::write(&source, "fn main() { 42 }\n").unwrap();
    let destination = fixture.0.join("program");
    for action in ["-E", "-M", "-MM", "-S", "-c", "-fsyntax-only", "-shared"] {
        fs::write(&destination, "sentinel\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(&source)
            .arg("-o")
            .arg(&destination)
            .args(["--c-arg", action])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{action}");
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("E-C-IMPORT: unsupported validation C argument"));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "sentinel\n");
    }
}

#[test]
fn native_validation_cannot_redirect_output() {
    let fixture = Fixture::new();
    let output_file = fixture.0.join("existing output");
    fs::write(&output_file, "sentinel\n").unwrap();
    let path = output_file.to_str().unwrap();
    for flags in [vec!["-o", path], vec!["-ooverridden-output"]] {
        for operation in ["check", "emit-c"] {
            let output = fixture.check(operation, "bool", &flags);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("E-C-IMPORT: unsupported validation C argument"));
            assert!(output.stdout.is_empty());
            assert_eq!(fs::read_to_string(&output_file).unwrap(), "sentinel\n");
        }
    }
}

#[test]
fn native_validation_preserves_compile_flags_and_header_assertions() {
    let fixture = Fixture::new();
    let flags = [
        "-O2",
        "-g",
        "-Wall",
        "-Wno-error",
        "-Wno-incompatible-pointer-types",
        "-DVALIDATION_FIXTURE=1",
    ];
    for operation in ["check", "emit-c"] {
        let output = fixture.check(operation, "bool", &flags);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = fixture.check(operation, "i32", &flags);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("incompatible header prototype for exact"));
        assert!(output.stdout.is_empty());
    }
}
