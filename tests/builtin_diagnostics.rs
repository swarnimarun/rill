use rill::{check, interpreter, modules, parser};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-builtin-diagnostics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn entry(&self, source: &str) -> PathBuf {
        let path = self.0.join("main.rill");
        fs::write(&path, source).unwrap();
        path
    }

    fn error(&self, source: &str) -> String {
        modules::compile::load_program(&self.entry(source))
            .expect_err("invalid source must reject")
            .to_string()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn checker_error(source: &str) -> String {
    check::check(&parser::parse(source).unwrap()).expect_err("invalid source must reject")
}

#[test]
fn checker_and_file_loader_suggest_actual_unqualified_builtins() {
    let project = Project::new();
    for (name, suggestion, arguments) in [
        ("byte_lne", "byte_len", "\"abc\""),
        ("bytes_gte", "bytes_get", "bytes_new(4), 0"),
        (
            "u8x16_lnae",
            "u8x16_lane",
            "u8x16_load(bytes_new(16), 0), 0",
        ),
        (
            "owned_bytes_gte",
            "owned_bytes_get",
            "system_bytes_zeroed_abort(4), 0",
        ),
        ("system_box_i23_abort", "system_box_i32_abort", "42"),
        ("is_nul", "is_null", "c_null()"),
    ] {
        let source = format!("fn main() {{ {name}({arguments}) }}");
        for error in [checker_error(&source), project.error(&source)] {
            assert!(error.contains(name), "{error}");
            assert!(
                error.contains(&format!("help: did you mean '{suggestion}'?")),
                "{error}"
            );
        }
    }
}

#[test]
fn cli_builtin_typo_keeps_rejection_and_reports_help() {
    let project = Project::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("check")
        .arg(project.entry("fn main() { bytes_gte(bytes_new(4), 0) }"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("E-MODULE-NAME"), "{error}");
    assert!(error.contains("help: did you mean 'bytes_get'?"), "{error}");
}

#[test]
fn module_members_and_type_names_do_not_acquire_builtin_value_candidates() {
    let project = Project::new();
    fs::write(
        project.0.join("utils.rill"),
        "export fn identity(value) { value }",
    )
    .unwrap();
    for source in [
        "import utils; fn main() { utils.bytes_gte(bytes_new(4), 0) }",
        "import utils; fn main() { utils.owned_bytes_gte(bytes_new(4), 0) }",
        "import utils; fn main() { utils.return(42) }",
        "type Container = { value: bytes_gte } fn main() { 42 }",
    ] {
        let error = project.error(source);
        assert!(!error.contains("did you mean 'bytes_get'"), "{error}");
        assert!(!error.contains("did you mean 'owned_bytes_get'"), "{error}");
        assert!(!error.contains("not built-in control flow"), "{error}");
    }
}

#[test]
fn visible_names_compete_with_builtins_without_arbitrary_ties() {
    let project = Project::new();
    // byte_cat is one edit from both this local and the actual byte_at builtin.
    let source = "fn main() { let byte_car = 42; byte_cat }";
    for _ in 0..8 {
        for error in [checker_error(source), project.error(source)] {
            assert!(!error.contains("did you mean"), "{error}");
            assert!(!error.contains("M$"), "{error}");
        }
    }
    for source in [
        "fn main() { bytes_ret(bytes_new(4), 0) }",
        "fn main() { memcopi() }",
    ] {
        for error in [checker_error(source), project.error(source)] {
            assert!(!error.contains("did you mean"), "{error}");
        }
    }
}

#[test]
fn real_builtins_retain_type_capacity_and_reservation_checks() {
    let project = Project::new();
    let source = "fn main() { bytes_get(bytes_new(4), 0) + byte_len(\"answer\") }";
    let program = modules::compile::load_program(&project.entry(source)).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "6");

    for (source, original) in [
        ("fn main() { bytes_new(257) }", "E-BUFFER"),
        (
            "fn main() { system_box_i32(42) }",
            "E-ALLOC: import std.alloc",
        ),
        (
            "fn main() { let bytes = bytes_new(4); owned_bytes_get(bytes, 0) }",
            "E-TYPE",
        ),
    ] {
        for error in [checker_error(source), project.error(source)] {
            assert!(error.contains(original), "{error}");
            assert!(!error.contains("did you mean"), "{error}");
        }
    }
    let reserved = "fn bytes_get(value) { value } fn main() { 42 }";
    assert!(checker_error(reserved).contains("duplicate function or constructor 'bytes_get'"));
    assert!(project
        .error(reserved)
        .contains("reserved declaration bytes_get"));
}
