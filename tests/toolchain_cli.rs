use std::{fs, path::PathBuf, process::Command};

struct Tree(PathBuf);
impl Tree {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rill-sdk-cli-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn relative_explicit_sdk_survives_project_working_directory_change() {
    let tree = Tree::new("relative");
    fs::create_dir(tree.0.join("sdk")).unwrap();
    fs::write(tree.0.join("sdk/toolchain.json"), serde_json::json!({
        "version":1, "rill":env!("CARGO_PKG_VERSION"), "tools":{}, "packages":{}, "runtime_dirs":[]
    }).to_string()).unwrap();
    fs::create_dir_all(tree.0.join("app/src")).unwrap();
    fs::write(tree.0.join("app/src/main.rill"), "fn main() { 42 }").unwrap();
    fs::write(
        tree.0.join("app/rill.json"),
        r#"{"version":1,"name":"app","entry":"src/main.rill"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .current_dir(&tree.0)
        .env("RILL_TOOLCHAIN_ROOT", "sdk")
        .args(["project", "run", "app"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
}

#[cfg(unix)]
#[test]
fn dangling_adjacent_sdk_reports_error_instead_of_switching_provider() {
    let tree = Tree::new("dangling");
    fs::create_dir(tree.0.join("bin")).unwrap();
    fs::create_dir(tree.0.join("libexec")).unwrap();
    let binary = tree.0.join("bin/rill");
    if fs::hard_link(env!("CARGO_BIN_EXE_rill"), &binary).is_err() {
        fs::copy(env!("CARGO_BIN_EXE_rill"), &binary).unwrap();
    }
    std::os::unix::fs::symlink("missing-sdk", tree.0.join("libexec/rill")).unwrap();
    fs::write(
        tree.0.join("shader.slang"),
        "[shader(\"compute\")][numthreads(1,1,1)] void main() {}",
    )
    .unwrap();
    fs::write(tree.0.join("rill.shaders.json"), serde_json::json!({
        "version":1, "shaders":[{"name":"test", "language":"slang", "stage":"compute", "entry":"main", "source":"shader.slang", "target":"spirv"}]
    }).to_string()).unwrap();
    let output = Command::new(&binary)
        .env_remove("RILL_TOOLCHAIN_ROOT")
        .args(["shader", "build"])
        .arg(tree.0.join("rill.shaders.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("E-TOOLCHAIN"),
        "{output:?}"
    );
}
