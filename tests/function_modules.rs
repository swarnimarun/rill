use rill::modules::compile::{load_program, load_program_with_sources};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-function-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn callable_parameter_and_result_types_preserve_module_privacy() {
    for body in [
        "fn(value: Secret){value.value}",
        "fn(){record Secret {value=42}}",
    ] {
        let workspace = Workspace::new();
        workspace.write(
            "hidden.rill",
            &format!("type Secret = {{value:i32}} export fn leak(){{{body}}}"),
        );
        workspace.write("main.rill", "import hidden as h; fn main(){0}");
        let error = load_program(&workspace.0.join("main.rill")).unwrap_err();
        assert_eq!(error.code, "E-MODULE-PRIVATE-TYPE", "{error}");
    }
}

#[test]
fn private_capture_representation_does_not_expose_a_private_value() {
    let workspace = Workspace::new();
    workspace.write("hidden.rill","type Secret = {value:i32} export fn factory(){let private=record Secret {value=42}; fn(){private.value}}");
    workspace.write("main.rill", "import hidden as h; fn main(){h.factory()()}");
    let mapped = load_program_with_sources(&workspace.0.join("main.rill")).unwrap();
    assert_eq!(
        rill::interpreter::run_reference(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert_eq!(
        rill::interpreter::run(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
}

#[test]
fn exported_function_values_and_lambda_annotations_have_resolved_origins() {
    let workspace = Workspace::new();
    workspace.write(
        "helper.rill",
        "export type Value = {number:i32} export fn increment(x){x+1}",
    );
    workspace.write("main.rill","import helper as h; fn invoke(f,x){f(x)} fn main(){let increment=h.increment; let extract=fn(value:h.Value){value.number}; invoke(increment,extract(record h.Value {number=41}))}");
    let mapped = load_program_with_sources(&workspace.0.join("main.rill")).unwrap();
    assert_eq!(
        rill::interpreter::run_reference(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert_eq!(
        rill::interpreter::run(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert!(mapped.nodes().iter().any(|node| node
        .path
        .steps
        .iter()
        .any(|step| matches!(step, rill::source_map::AstStep::LambdaParameter(_)))));
}

#[test]
fn record_field_calls_are_indirect_in_module_compilation() {
    let workspace = Workspace::new();
    workspace.write("main.rill","type Holder<a> = {callback:a} fn main(){let holder=record Holder{callback=fn(x){x+1}}; holder.callback(41)}");
    let entry = workspace.0.join("main.rill");
    let mapped = load_program_with_sources(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run_reference(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert_eq!(
        rill::interpreter::run(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&entry)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(status.stdout, b"42\n");
}
