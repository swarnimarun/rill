use rill::{interpreter, modules::compile::load_program};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-module-audit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
    fn load(&self) -> Result<rill::ast::Program, rill::modules::ModuleError> {
        load_program(&self.0.join("main.rill"))
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap()
    }
}
#[test]
fn shadowed_type_parameters_and_constructor_names_resolve_lexically() {
    let tree = Tree::new();
    tree.put(
        "a.rill",
        "export type Box<Box> = Box(Box)\nexport let id Box = Box",
    );
    tree.put(
        "main.rill",
        "import a; let main = match a.Box(a.id(42)) { a.Box(n) => n }",
    );
    assert_eq!(
        interpreter::run(&tree.load().unwrap()).unwrap().to_string(),
        "42"
    );
}
#[test]
fn identical_public_type_names_remain_nominally_distinct() {
    let tree = Tree::new();
    tree.put("a.rill", "export type Thing = Mk(i32)");
    tree.put("b.rill", "export type Thing = Mk(i32)");
    tree.put(
        "main.rill",
        "import a; import b; let main = match a.Mk(1) { b.Mk(x) => x }",
    );
    assert!(tree.load().unwrap_err().to_string().contains("E-TYPE"));
}
#[test]
fn imported_names_require_alias_and_visible_declarations() {
    let tree = Tree::new();
    tree.put(
        "a.rill",
        "type Hidden = Secret(i32)\nexport let public x = x",
    );
    tree.put("main.rill", "import a; let main = public(1)");
    assert!(tree
        .load()
        .unwrap_err()
        .to_string()
        .contains("E-MODULE-NAME"));
    tree.put("main.rill", "import a; let main = a.Secret(1)");
    assert!(tree
        .load()
        .unwrap_err()
        .to_string()
        .contains("E-MODULE-PRIVATE"));
    tree.put(
        "main.rill",
        "import a; type Public = Public(a.Hidden)\nlet main = 1",
    );
    assert!(tree
        .load()
        .unwrap_err()
        .to_string()
        .contains("E-MODULE-PRIVATE"));
    tree.put("main.rill", "import a; let main = missing.public(1)");
    assert!(tree
        .load()
        .unwrap_err()
        .to_string()
        .contains("E-MODULE-ALIAS"));
}
#[test]
fn local_alias_shadow_does_not_change_explicit_qualified_lookup() {
    let tree = Tree::new();
    tree.put("a.rill", "export let value x = x + 1");
    tree.put("main.rill", "import a; let main = let a = 40 in a.value(a)");
    assert_eq!(
        interpreter::run(&tree.load().unwrap()).unwrap().to_string(),
        "41"
    );
}
