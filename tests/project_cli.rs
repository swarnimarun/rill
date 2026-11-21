use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rill project {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.current_dir(&self.0);
        command
    }

    fn scaffold(&self) -> PathBuf {
        let output = self.command().args(["new", "my tool"]).output().unwrap();
        success(&output);
        self.0.join("my tool")
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_manifest(root: &Path, text: &str) {
    fs::write(root.join("rill.json"), text).unwrap();
}

#[test]
fn scaffold_checks_runs_and_builds_from_a_directory_with_spaces() {
    let tree = Tree::new();
    let root = tree.scaffold();
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("rill.json")).unwrap()).unwrap();
    assert_eq!(manifest["name"], "my-tool");
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        "/build/\n"
    );
    fs::write(
        root.join("src/main.rill"),
        "import math; fn main() { math.double(21) }",
    )
    .unwrap();
    fs::write(root.join("src/math.rill"), "export fn double(x) { x + x }").unwrap();
    success(
        &tree
            .command()
            .args(["project", "check", "my tool"])
            .output()
            .unwrap(),
    );
    for mode in ["--interpret", "--native"] {
        let output = tree
            .command()
            .args(["project", "run", "my tool", mode])
            .output()
            .unwrap();
        success(&output);
        assert_eq!(output.stdout, b"42\n");
    }
    success(
        &tree
            .command()
            .args(["project", "build", "my tool"])
            .output()
            .unwrap(),
    );
    let executable = root.join(format!("build/my-tool{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new(executable).output().unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");

    let output = tree
        .command()
        .current_dir(&root)
        .args([
            "project",
            "build",
            "--opt-level",
            "0",
            "-o",
            "out/debug tool",
        ])
        .output()
        .unwrap();
    success(&output);
    let output = Command::new(root.join("out/debug tool")).output().unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn new_never_overwrites_existing_directories_or_files() {
    let tree = Tree::new();
    let root = tree.scaffold();
    fs::write(root.join("src/main.rill"), "fn main() { 7 }").unwrap();
    let output = tree.command().args(["new", "my tool"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E-PROJECT-CREATE"));
    assert_eq!(
        fs::read_to_string(root.join("src/main.rill")).unwrap(),
        "fn main() { 7 }"
    );
    fs::write(tree.0.join("existing"), "preserve").unwrap();
    let output = tree.command().args(["new", "existing"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(tree.0.join("existing")).unwrap(),
        "preserve"
    );
}

#[test]
fn manifests_reject_unknown_duplicate_and_invalid_values() {
    let tree = Tree::new();
    let root = tree.scaffold();
    for manifest in [
        r#"{"version":2,"name":"app","entry":"src/main.rill"}"#,
        r#"{"version":1.0,"name":"app","entry":"src/main.rill"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","command":"touch bad"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","version":1}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","\u006eame":"app"}"#,
        r#"{"version":1,"name":"../app","entry":"src/main.rill"}"#,
        r#"{"version":1,"name":"app","entry":"../outside.rill"}"#,
        r#"{"version":1,"name":"app","entry":"src/./main.rill"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"../escape"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"src/main.rill"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"rill.json"}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":null}"#,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"src"}"#,
        r#"[]"#,
        r#"{"version":1}"#,
    ] {
        write_manifest(&root, manifest);
        let output = tree
            .command()
            .args(["project", "check", "my tool"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {manifest}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("E-PROJECT-"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    write_manifest(&root, &" ".repeat(65_537));
    let output = tree
        .command()
        .args(["project", "check", "my tool"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 65536"));
}

#[test]
fn custom_manifest_output_is_used_and_source_errors_fail_the_command() {
    let tree = Tree::new();
    let root = tree.scaffold();
    write_manifest(
        &root,
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"out/custom app"}"#,
    );
    success(
        &tree
            .command()
            .args(["project", "build", "my tool"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        Command::new(root.join("out/custom app"))
            .output()
            .unwrap()
            .stdout,
        b"42\n"
    );
    fs::write(root.join("src/main.rill"), "fn main() { true + 1 }").unwrap();
    for action in ["check", "build", "run"] {
        let output = tree
            .command()
            .args(["project", action, "my tool"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("E-PROJECT-RUN"));
    }
}

#[test]
fn project_watch_once_runs_the_checked_entry() {
    let tree = Tree::new();
    tree.scaffold();
    let output = tree
        .command()
        .args(["project", "watch", "my tool", "--once"])
        .output()
        .unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn output_override_cannot_replace_project_inputs_or_escape() {
    let tree = Tree::new();
    let root = tree.scaffold();
    let original = fs::read(root.join("src/main.rill")).unwrap();
    for path in [
        "../escape",
        "src/main.rill",
        "rill.json",
        "src",
        "/tmp/escape",
    ] {
        let output = tree
            .command()
            .args(["project", "build", "my tool", "-o", path])
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {path}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("E-PROJECT-"));
    }
    assert_eq!(fs::read(root.join("src/main.rill")).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn project_rejects_entry_and_output_symlink_escapes_and_source_aliases() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let root = tree.scaffold();
    fs::write(tree.0.join("outside.rill"), "fn main() { 0 }").unwrap();
    symlink(tree.0.join("outside.rill"), root.join("src/linked.rill")).unwrap();
    write_manifest(
        &root,
        r#"{"version":1,"name":"app","entry":"src/linked.rill"}"#,
    );
    let output = tree
        .command()
        .args(["project", "check", "my tool"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    write_manifest(
        &root,
        r#"{"version":1,"name":"app","entry":"src/main.rill"}"#,
    );
    symlink(&tree.0, root.join("outside")).unwrap();
    symlink(root.join("src/main.rill"), root.join("source-alias")).unwrap();
    symlink(tree.0.join("missing"), root.join("dangling")).unwrap();
    for path in ["outside/escape", "source-alias", "dangling"] {
        let output = tree
            .command()
            .args(["project", "build", "my tool", "-o", path])
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {path}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("E-PROJECT-"));
    }
    assert_eq!(
        fs::read_to_string(root.join("src/main.rill")).unwrap(),
        "fn main() {\n    42\n}\n"
    );
}
