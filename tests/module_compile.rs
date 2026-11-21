use rill::{backend, interpreter, modules::compile::load_program};
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
            "rill-module-compile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, file: &str, source: &str) {
        fs::write(self.0.join(file), source).unwrap();
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
fn compare(tree: &Tree, expected: &str) {
    let program = load_program(&tree.entry()).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), expected);
    let cfile = tree.0.join("output.c");
    let binary = tree.0.join("output");
    fs::write(&cfile, backend::emit(&program).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&cfile)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&binary).output().unwrap();
        assert!(run.status.success());
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
    }
}
fn error(tree: &Tree, code: &str) {
    let err = load_program(&tree.entry()).unwrap_err();
    assert!(err.to_string().contains(code), "{err}");
}
#[test]
fn qualified_exports_generic_helpers_and_colliding_names() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "import a; import b; let main = a.add(a.identity(20)) + b.add(20)",
    );
    tree.write(
        "a.rill",
        "let helper x = x + 1\nexport let identity x = x\nexport let add x = helper(x)",
    );
    tree.write(
        "b.rill",
        "let helper x = x + 1\nexport let add x = helper(x)",
    );
    compare(&tree, "42");
}
#[test]
fn exported_types_and_qualified_constructors() {
    let tree = Tree::new();
    tree.write("main.rill", "import a; type Envelope = Envelope(a.Choice)\nlet main = match Envelope(a.Wrap(42)) { Envelope(a.Wrap(n)) => n | Envelope(a.Zero) => 0 }");
    tree.write("a.rill", "export type Choice = Zero | Wrap(i32)");
    compare(&tree, "42");
}

#[test]
fn vectors_survive_exported_types_and_module_calls() {
    let tree = Tree::new();
    tree.write("main.rill", "import a; let main = a.verify(a.make())");
    tree.write("a.rill", "export type Packet = Packet(u8x16)\nexport let make = Packet(u8x16_load(bytes_new(16), 0))\nexport let verify p = match p { Packet(x) => u8x16_all(u8x16_eq(x, x)) }");
    compare(&tree, "true");
    tree.write("a.rill", "export let u8x16_lane x = x");
    error(&tree, "E-MODULE-NAME");
}

#[test]
fn imported_constructor_output_uses_source_name() {
    let tree = Tree::new();
    tree.write("main.rill", "import a; let main = a.Some(1)");
    tree.write("a.rill", "export type Option<a> = None | Some(a)");
    compare(&tree, "Some(1)");
}

#[test]
fn imported_records_allow_nested_local_paths_and_alias_shadowing() {
    let tree = Tree::new();
    tree.write(
        "types.rill",
        "export type Inner = { x: i32 }\nexport type Outer = { inner: Inner }",
    );
    tree.write("main.rill", "import types as t; let main = let mut t = record t.Outer { inner = record t.Inner { x = 41 } } in let done = t.inner.x <- t.inner.x + 1 in t.inner.x");
    compare(&tree, "42");
    tree.write("main.rill", "import types as t; let main = t.Outer.inner");
    error(&tree, "E-MODULE-NAME");
}

#[test]
fn entry_extern_preserves_c_symbol_and_library_externs_are_rejected() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "import a; extern fn c_plus(x: i32, y: i32) -> i32; let main = a.add(c_plus(1, 1))",
    );
    tree.write("a.rill", "export let add x = x + 40");
    let program = load_program(&tree.entry()).unwrap();
    assert!(interpreter::run(&program)
        .unwrap_err()
        .contains("E-INTERP-FFI"));
    let cfile = tree.0.join("program.c");
    let external = tree.0.join("external.c");
    let binary = tree.0.join("program");
    fs::write(&cfile, backend::emit(&program).unwrap()).unwrap();
    fs::write(
        &external,
        "#include <stdint.h>\nint32_t c_plus(int32_t x, int32_t y) { return x + y; }\n",
    )
    .unwrap();
    let build = Command::new("cc")
        .args(["-std=c11", "-O2"])
        .arg(&cfile)
        .arg(&external)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(run.stdout, b"42\n");

    tree.write(
        "a.rill",
        "extern fn hidden() -> i32; export let add x = hidden()",
    );
    assert!(load_program(&tree.entry()).is_ok());
}

#[test]
fn exported_read_and_inout_functions_keep_parameter_modes() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "import a; let main = let mut n = 40 in let x = a.inc(inout n) in a.read_value(n)",
    );
    tree.write(
        "a.rill",
        "export let inc (inout x) = x <- x + 2\nexport let read_value (read x) = x",
    );
    compare(&tree, "42");
}
#[test]
fn rejects_private_symbols_and_inferred_private_type_leaks() {
    let tree = Tree::new();
    tree.write("main.rill", "import a; let main = a.secret(0)");
    tree.write("a.rill", "let secret x = x");
    error(&tree, "E-MODULE-PRIVATE");
    tree.write("main.rill", "import a; let main = 0");
    tree.write(
        "a.rill",
        "type Hidden = Hidden(i32)\nexport let leak x = Hidden(x)",
    );
    error(&tree, "E-MODULE-PRIVATE-TYPE");
    tree.write(
        "a.rill",
        "type Hidden = Hidden(i32)\nexport type Public = Public(Hidden)",
    );
    error(&tree, "E-MODULE-PRIVATE-TYPE");
}
#[test]
fn rejects_import_cycles_and_cross_module_inout_aliasing() {
    let tree = Tree::new();
    tree.write("main.rill", "import a; let main = 0");
    tree.write("a.rill", "import b; let x = 0");
    tree.write("b.rill", "import a; let x = 0");
    error(&tree, "E-MODULE-CYCLE");
    tree.write(
        "main.rill",
        "import a; let main = let mut x = 0 in a.pair(inout x, inout x)",
    );
    tree.write("a.rill", "export let pair (inout x) (inout y) = x <- y");
    error(&tree, "E-LOAN");
}

#[test]
fn owner_primitive_type_names_are_reserved_before_module_mangling() {
    for name in ["SystemBoxI32", "SystemBytes"] {
        let source = format!("type {name} = Fake\nlet main = Fake");
        let direct = rill::parser::parse(&source).unwrap();
        let direct_error = rill::check::check(&direct).unwrap_err();
        assert!(direct_error.contains("reserved type"), "{direct_error}");

        let tree = Tree::new();
        tree.write("main.rill", &source);
        error(&tree, "E-MODULE-NAME");

        tree.write("main.rill", "import library; let main = library.make()");
        tree.write(
            "library.rill",
            &format!("export type {name} = Fake\nexport let make = Fake"),
        );
        error(&tree, "E-MODULE-NAME");

        // Primitive annotation lookup must never silently bypass a local type
        // declaration with the same spelling.
        tree.write(
            "main.rill",
            &format!("type {name} = Fake\nlet identity (value: {name}) = value\nlet main = identity(Fake)"),
        );
        error(&tree, "E-MODULE-NAME");
    }
}
