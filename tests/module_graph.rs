use rill::modules;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-module-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, source: &str) {
        fs::write(self.0.join(path), source).unwrap();
    }
    fn entry(&self) -> PathBuf {
        self.0.join("main.rill")
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fail(path: &Path, code: &str) {
    let error = modules::load(path).unwrap_err();
    assert_eq!(error.code, code, "{error}");
}

#[test]
fn loads_deterministic_graph_without_claiming_compilation() {
    let tree = Tree::new();
    let entry = "// use a library\nimport zoo as z;\nimport alpha;\nlet main = z.value()";
    tree.write("main.rill", entry);
    tree.write("alpha.rill", "import common;\nlet a = common.value()");
    tree.write("zoo.rill", "import common as c;\nlet value = c.value()");
    tree.write("common.rill", "let value = 42");
    let graph = modules::load(&tree.entry()).unwrap();
    assert_eq!(
        graph.modules.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["@entry", "alpha", "common", "zoo"]
    );
    let module = &graph.modules["@entry"];
    assert_eq!(module.source, entry);
    assert_eq!(module.imports[0].module, "zoo");
    assert_eq!(module.imports[0].alias, "z");
    assert_eq!((module.imports[0].line, module.imports[0].column), (2, 1));
    assert_eq!(graph.modules["common"].source, "let value = 42");
    assert!(rill::parser::parse(entry).is_err());
    assert!(rill::compile(entry).is_err());
}

#[test]
fn rejects_cycles_duplicates_bad_imports_and_missing_files() {
    let tree = Tree::new();
    tree.write("main.rill", "import alpha; let main = 0");
    fail(&tree.entry(), "E-MODULE-IO");
    tree.write("alpha.rill", "import beta; let a = 0");
    tree.write("beta.rill", "import alpha; let b = 0");
    let error = modules::load(&tree.entry()).unwrap_err();
    assert_eq!(error.code, "E-MODULE-CYCLE");
    assert!(error.message.contains("alpha -> beta -> alpha"));
    for source in [
        "import alpha; import alpha as other; let main = 0",
        "import alpha as x; import beta as x; let main = 0",
    ] {
        tree.write("main.rill", source);
        fail(&tree.entry(), "E-MODULE-DUPLICATE");
    }
    for source in [
        "import ../outside; let main = 0",
        "import Alpha; let main = 0",
        "import alpha as X; let main = 0",
        "import alpha let main = 0",
        "let main = 0; import alpha;",
    ] {
        tree.write("main.rill", source);
        fail(&tree.entry(), "E-MODULE-IMPORT");
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_and_duplicate_canonical_paths() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    outside.write("external.rill", "let x = 0");
    tree.write("main.rill", "import escaped; let main = 0");
    symlink(outside.0.join("external.rill"), tree.0.join("escaped.rill")).unwrap();
    fail(&tree.entry(), "E-MODULE-PATH");
    tree.write("alpha.rill", "let x = 0");
    symlink(tree.0.join("alpha.rill"), tree.0.join("beta.rill")).unwrap();
    tree.write("main.rill", "import alpha; import beta; let main = 0");
    fail(&tree.entry(), "E-MODULE-DUPLICATE");
}

#[cfg(unix)]
#[test]
fn rejects_distinct_hard_links_to_the_same_file() {
    let tree = Tree::new();
    tree.write("main.rill", "import alpha; import beta; let main = 0");
    tree.write("alpha.rill", "let x = 0");
    fs::hard_link(tree.0.join("alpha.rill"), tree.0.join("beta.rill")).unwrap();
    fail(&tree.entry(), "E-MODULE-DUPLICATE");
}

#[test]
fn bounds_file_size_and_import_depth() {
    let tree = Tree::new();
    tree.write("main.rill", &" ".repeat(1024 * 1024 + 1));
    fail(&tree.entry(), "E-MODULE-LIMIT");
    tree.write("main.rill", "import m0; let main = 0");
    for i in 0..64 {
        tree.write(
            &format!("m{i}.rill"),
            &format!("import m{}; let value = 0", i + 1),
        );
    }
    tree.write("m64.rill", "let value = 0");
    fail(&tree.entry(), "E-MODULE-LIMIT");
}

#[test]
fn rejects_invalid_utf8_and_bounds_module_count() {
    let tree = Tree::new();
    fs::write(tree.entry(), [0xff]).unwrap();
    fail(&tree.entry(), "E-MODULE-UTF8");
    let mut source = String::new();
    for i in 0..256 {
        source.push_str(&format!("import module{i};\n"));
        tree.write(&format!("module{i}.rill"), "let value = 0");
    }
    source.push_str("let main = 0");
    tree.write("main.rill", &source);
    fail(&tree.entry(), "E-MODULE-LIMIT");
}

#[test]
fn supports_project_paths_with_spaces() {
    let tree = Tree::new();
    let root = tree.0.join("project with spaces");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.rill"), "import helper; let main = 0").unwrap();
    fs::write(root.join("helper.rill"), "let value = 0").unwrap();
    let graph = modules::load(&root.join("main.rill")).unwrap();
    assert!(graph.modules.contains_key("helper"));
}

#[test]
fn import_remains_usable_as_a_local_or_function_name() {
    let tree = Tree::new();
    tree.write("main.rill", "let main = let import = 1 in import");
    assert!(modules::load(&tree.entry()).is_ok());
    tree.write("main.rill", "let import x = x\nlet main = import(42)");
    assert!(modules::load(&tree.entry()).is_ok());
}
