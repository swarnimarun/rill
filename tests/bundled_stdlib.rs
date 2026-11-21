use rill::modules;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-bundled-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn entry(&self) -> PathBuf {
        self.0.join("main.rill")
    }
    fn cli(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .current_dir(&self.0)
            .arg(command)
            .arg("main.rill")
            .output()
            .unwrap()
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn compare(source: &str, expected: &[u8]) {
    let tree = Tree::new();
    tree.write("main.rill", source);
    success(tree.cli("check"));
    assert_eq!(success(tree.cli("run")), expected);
    fs::write(tree.0.join("out.c"), success(tree.cli("emit-c"))).unwrap();
    for opt in ["-O0", "-O2"] {
        success(
            Command::new("cc")
                .current_dir(&tree.0)
                .args(["-std=c11", opt, "out.c", "-o", "out"])
                .output()
                .unwrap(),
        );
        assert_eq!(
            success(Command::new(tree.0.join("out")).output().unwrap()),
            expected
        );
    }
    success(
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .current_dir(&tree.0)
            .args(["build", "main.rill", "-o", "built"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        success(Command::new(tree.0.join("built")).output().unwrap()),
        expected
    );
}
#[test]
fn bundled_json_works_outside_repository_without_copied_sources() {
    compare(include_str!("../examples/stdlib/main.rill"), b"42\n");
}
#[test]
fn result_is_an_explicit_ordinary_scalar_adt() {
    compare("import std.result as r; let unwrap (value: r.Result<i32, bool>) = match value { r.Ok(x) => x | r.Err(e) => 0 } let main = unwrap(r.Ok(42))", b"42\n");
    let tree = Tree::new();
    tree.write("main.rill", "let main = Ok(42)");
    assert!(modules::compile::load_program(&tree.entry()).is_err());
}
#[test]
fn nested_modules_use_last_segment_alias_and_entry_relative_transitive_imports() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "import nested.helper; let main = helper.answer()",
    );
    tree.write(
        "nested/helper.rill",
        "import other.value; export let answer = value.get()",
    );
    tree.write("other/value.rill", "export let get = 42");
    assert_eq!(success(tree.cli("run")), b"42\n");
    tree.write("other/value.rill", "let get = 42");
    assert_eq!(
        modules::compile::load_program(&tree.entry())
            .unwrap_err()
            .code,
        "E-MODULE-PRIVATE"
    );
}
#[test]
fn bundled_namespace_is_reserved_and_graph_snapshots_are_deterministic() {
    let tree = Tree::new();
    tree.write("std/json_input.rill", "let main = invalid");
    let source = "import std.result; import std.json_input as json; let main = 42";
    tree.write("main.rill", source);
    let graph = modules::load(&tree.entry()).unwrap();
    assert_eq!(
        graph.modules.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["@entry", "std.json_input", "std.result"]
    );
    assert_eq!(
        graph.modules["std.json_input"].source,
        include_str!("../stdlib/json_input.rill")
    );
    assert_eq!(
        graph.modules["std.result"].path,
        PathBuf::from("<std.result>")
    );
    assert_eq!(graph.modules["@entry"].imports[0].alias, "result");
    assert_eq!(graph.modules["@entry"].source, source);
    let again = modules::load(&tree.entry()).unwrap();
    for (name, module) in graph.modules {
        assert_eq!(module.source, again.modules[&name].source);
    }
    tree.write("std/unknown.rill", "export let answer = 42");
    tree.write("std.rill", "export let answer = 42");
    for name in ["std.unknown", "std", "std.json_input.extra"] {
        tree.write("main.rill", &format!("import {name}; let main = 0"));
        assert_eq!(
            modules::load(&tree.entry()).unwrap_err().code,
            "E-MODULE-STDLIB"
        );
    }
}
#[test]
fn malformed_qualified_segments_are_rejected() {
    let tree = Tree::new();
    for name in ["a..b", "a.", ".a", "a.B", "a.as", "a.1", "a/b"] {
        tree.write("main.rill", &format!("import {name}; let main = 0"));
        assert_eq!(
            modules::load(&tree.entry()).unwrap_err().code,
            "E-MODULE-IMPORT",
            "{name}"
        );
    }
    tree.write("main.rill", "let main = 0; import std.result;");
    assert_eq!(
        modules::load(&tree.entry()).unwrap_err().code,
        "E-MODULE-IMPORT"
    );
}
#[cfg(unix)]
#[test]
fn nested_symlinks_stay_confined() {
    let tree = Tree::new();
    let outside = Tree::new();
    outside.write("value.rill", "export let get = 42");
    std::os::unix::fs::symlink(&outside.0, tree.0.join("nested")).unwrap();
    tree.write("main.rill", "import nested.value; let main = value.get()");
    assert_eq!(
        modules::load(&tree.entry()).unwrap_err().code,
        "E-MODULE-PATH"
    );
}
