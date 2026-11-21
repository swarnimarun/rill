use rill::{backend, modules::compile::load_program};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-native-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
    fn load(&self) -> Result<rill::ast::Program, rill::modules::ModuleError> {
        load_program(&self.0.join("main.rill"))
    }
    fn run(&self, expected: &[u8]) {
        let program = self.load().unwrap();
        self.write("out.c", &backend::emit(&program).unwrap());
        for opt in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", opt, "-I"])
                .arg(&self.0)
                .arg(self.0.join("out.c"))
                .arg(self.0.join("api.c"))
                .arg("-o")
                .arg(self.0.join("out"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(self.0.join("out")).output().unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, expected);
        }
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn private_wrappers_and_public_foreign_values_keep_symbol_identity() {
    let t = Tree::new();
    t.write("main.rill", "import bindings as b; import other; extern fn answer(x: i32) -> i32; fn main() { b.wrap(10) + b.answer(10) + answer(10) + other.answer(10) + b.FLAG }");
    t.write("api.h", "#include <stdint.h>\nint32_t hidden(int32_t); int32_t answer(int32_t); enum { PRIVATE = 1, FLAG = 0 };\n");
    t.write("bindings.rill", "@include(\"api.h\"); extern fn hidden(x: i32) -> i32; extern const PRIVATE: i32; export extern fn answer(value: i32) -> i32; export extern const FLAG: i32; export fn wrap(x) { hidden(x) + PRIVATE }");
    t.write("other.rill", "export fn answer(x) { x + 1 }");
    t.write("api.c", "#include <stdint.h>\nint32_t hidden(int32_t x) { return x; }\nint32_t answer(int32_t x) { return x; }\n");
    let p = t.load().unwrap();
    assert_eq!(p.externs.iter().filter(|f| f.name == "answer").count(), 1);
    t.run(b"42\n");
}

#[test]
fn private_foreign_declarations_stay_private() {
    for body in ["b.hidden()", "b.FLAG"] {
        let t = Tree::new();
        t.write(
            "main.rill",
            &format!("import bindings as b; fn main() {{ {body} }}"),
        );
        t.write(
            "bindings.rill",
            "extern fn hidden() -> i32; extern const FLAG: i32;",
        );
        assert_eq!(t.load().unwrap_err().code, "E-MODULE-PRIVATE");
    }
}

#[test]
fn incompatible_global_c_declarations_fail_before_emission() {
    for declarations in [
        (
            "extern fn same(x: i32) -> i32;",
            "extern fn same(x: u32) -> i32;",
        ),
        ("extern fn same() -> i32;", "extern fn same() -> bool;"),
        ("extern fn same() -> i32;", "extern fn same(x: i32) -> i32;"),
        ("extern fn same() -> i32;", "extern const same: i32;"),
        ("extern const same: i32;", "extern const same: u32;"),
        (
            "extern fn same(x: ptr<Item>) -> unit;",
            "extern fn same(x: const_ptr<Item>) -> unit;",
        ),
    ] {
        let t = Tree::new();
        t.write("main.rill", "import a; import b; fn main() { 0 }");
        t.write("a.rill", declarations.0);
        t.write("b.rill", declarations.1);
        assert_eq!(t.load().unwrap_err().code, "E-MODULE-EXTERN");
    }
}

#[test]
fn transitive_directives_and_compatible_constants_are_retained() {
    let t = Tree::new();
    t.write("main.rill", "import a; fn main() { a.value() }");
    t.write("a.rill", "@include(\"a.h\"); @link(\"a\"); import b; extern const FLAG: i32; export fn value() { b.value() + FLAG }");
    t.write("b.rill", "@include(\"b.h\"); @link(\"b\"); @pkg_config(\"b-pkg\"); extern const FLAG: i32; export fn value() { FLAG }");
    let p = t.load().unwrap();
    assert_eq!(p.constants.len(), 1);
    assert_eq!(p.native.includes, ["a.h", "b.h"]);
    assert_eq!(p.native.links, ["a", "b"]);
    assert_eq!(p.native.packages, ["b-pkg"]);
}

#[test]
fn coalescing_validates_each_declarations_parameter_names() {
    let valid = "extern fn same(x: i32, y: i32) -> i32;";
    let invalid = "extern fn same(x: i32, x: i32) -> i32;";
    // Exercise both sorted module orders, including entry-first traversal.
    for (first, second) in [(valid, invalid), (invalid, valid)] {
        for entry_declaration in [false, true] {
            let t = Tree::new();
            if entry_declaration {
                t.write("main.rill", &format!("import b; {first} fn main() {{ 0 }}"));
            } else {
                t.write("main.rill", "import a; import b; fn main() { 0 }");
                t.write("a.rill", first);
            }
            t.write("b.rill", second);
            let error = t.load().unwrap_err();
            assert_eq!(error.code, "E-MODULE-CHECK");
            assert!(
                error.message.contains("duplicate parameter 'x' in 'same'"),
                "{error}"
            );
        }
    }
}
