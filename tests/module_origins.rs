use rill::{
    ast::Expr,
    modules::{
        compile::{load_program, load_program_with_sources},
        provenance::{Contribution, Lowering, SourceProgram},
        resolve::Kind,
    },
    source_map::{AstNode, AstPath, AstStep, OriginKind},
    syntax::{AstDeclaration, SyntaxTree},
};
#[cfg(unix)]
use std::process::Command;
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
            "rill-module-origins-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: impl AsRef<Path>, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
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
fn loaded(tree: &Tree) -> SourceProgram {
    let ordinary = load_program(&tree.entry()).unwrap();
    let mapped = load_program_with_sources(&tree.entry()).unwrap();
    assert_eq!(mapped.program(), &ordinary);
    verify(&mapped);
    mapped
}
fn verify(mapped: &SourceProgram) {
    let program = mapped.program();
    let mut work: Vec<_> = (0..program.functions.len())
        .map(AstDeclaration::Function)
        .chain((0..program.externs.len()).map(AstDeclaration::ExternFunction))
        .chain((0..program.constants.len()).map(AstDeclaration::Constant))
        .chain((0..program.types.len()).map(AstDeclaration::Type))
        .map(AstPath::root)
        .collect();
    let mut count = 0;
    while let Some(path) = work.pop() {
        let node = mapped.node(&path).unwrap();
        let origin = mapped.origin(&path).unwrap();
        assert_eq!(node.kind(), origin.kind);
        assert_eq!(origin.origins[0].contribution, Contribution::Retained);
        assert!(origin
            .origins
            .iter()
            .skip(1)
            .all(|o| o.contribution == Contribution::CoalescedNative));
        for step in node.child_steps() {
            work.push(path.child(step));
        }
        count += 1;
    }
    assert_eq!(count, mapped.nodes().len());
    let mut contributions = 0;
    for source in mapped.sources() {
        assert_eq!(mapped.source(source.id), Some(source));
        let syntax = SyntaxTree::parse(&source.text);
        let map = syntax.ast_source_map().unwrap();
        let actual: Vec<_> = mapped
            .nodes()
            .iter()
            .flat_map(|n| &n.origins)
            .filter(|o| o.source == source.id)
            .collect();
        assert_eq!(actual.len(), map.entries().len());
        for origin in actual {
            let expected = map.origin(&origin.path).unwrap();
            assert_eq!(origin.span, expected.span);
            assert_eq!(origin.provenance, expected.provenance);
            assert_eq!(origin.syntax, syntax.node(expected.syntax).unwrap().kind());
            assert_eq!(
                mapped.origin_text(origin),
                Some(&source.text[origin.span.byte_range()])
            );
            contributions += 1;
        }
    }
    assert_eq!(
        contributions,
        mapped
            .nodes()
            .iter()
            .map(|n| n.origins.len())
            .sum::<usize>()
    );
    for symbol in mapped.symbols() {
        assert_eq!(mapped.symbol(symbol.symbol.id), Some(symbol));
        let origin = mapped.origin(&symbol.target).unwrap();
        assert!(origin
            .origins
            .iter()
            .any(|o| o.source == symbol.source && o.path == symbol.source_path));
    }
}
fn same_error(tree: &Tree, code: &str) {
    let old = load_program(&tree.entry()).unwrap_err();
    let new = load_program_with_sources(&tree.entry()).unwrap_err();
    assert_eq!(old.code, code, "{old}");
    assert_eq!(new.code, old.code);
    assert_eq!(new.message, old.message);
}

#[test]
fn nested_modules_same_names_exports_aliases_shadowing_and_order() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "import b as right; import a as left; fn main() { left.answer(20) + right.answer(20) }",
    );
    tree.write(
        "a.rill",
        "import helpers.math as m; fn helper(x) { m.add(x) } export fn answer(x) { helper(x) }",
    );
    tree.write(
        "b.rill",
        "fn helper(x) { x + 1 } export fn answer(x) { helper(x) }",
    );
    tree.write("helpers/math.rill", "export fn add(x) { x + 1 }");
    let first = loaded(&tree);
    assert_eq!(
        rill::interpreter::run(first.program()).unwrap().to_string(),
        "42"
    );
    let helpers: Vec<_> = first
        .symbols()
        .iter()
        .filter(|s| s.symbol.name == "helper")
        .collect();
    assert_eq!(helpers.len(), 2);
    assert_ne!(helpers[0].symbol.id, helpers[1].symbol.id);
    assert_ne!(helpers[0].source, helpers[1].source);
    assert!(helpers.iter().all(|s| !s.symbol.exported));
    assert!(first
        .symbols()
        .iter()
        .filter(|s| s.symbol.name == "answer")
        .all(|s| s.symbol.exported));
    tree.write(
        "main.rill",
        "import a as left; import b as right; fn main() { left.answer(20) + right.answer(20) }",
    );
    let second = loaded(&tree);
    assert_eq!(first.program(), second.program());
    assert_eq!(first.symbols(), second.symbols());
    tree.write(
        "main.rill",
        "import a as left; fn main() { left.helper(1) }",
    );
    same_error(&tree, "E-MODULE-PRIVATE");
}

#[test]
fn qualification_changes_are_explicit_but_local_field_shadowing_is_not_rewritten() {
    let tree = Tree::new();
    tree.write(
        "a.rill",
        "export type Choice = Zero | Wrap(i32) export type Point = { x: i32 }",
    );
    tree.write("main.rill", "import a as m; fn take(x: m.Choice) { match x { m.Zero => 0 | m.Wrap(n) => n } } fn main() { let first = take(m.Zero); let m = record m.Point { x = 42 }; first + m.x }");
    let mapped = loaded(&tree);
    let mut qualified = 0;
    let mut shadowed = 0;
    for node in mapped.nodes() {
        let source = mapped.origin_text(&node.origins[0]).unwrap();
        if source == "m.Zero" && matches!(mapped.node(&node.path), Some(AstNode::Expression(_))) {
            qualified += 1;
            let Lowering::QualifiedValue(id) = node.origins[0].lowering else {
                panic!("missing Field -> Var marker")
            };
            assert_eq!(mapped.symbol(id).unwrap().symbol.kind, Kind::Constructor);
            assert!(matches!(
                mapped.node(&node.path),
                Some(AstNode::Expression(Expr::Var(_)))
            ));
        }
        if source == "m.x" {
            shadowed += 1;
            assert_eq!(node.origins[0].lowering, Lowering::Unchanged);
            assert!(matches!(
                mapped.node(&node.path),
                Some(AstNode::Expression(Expr::Field(..)))
            ));
        }
    }
    assert_eq!((qualified, shadowed), (1, 1));
    assert_eq!(
        rill::interpreter::run(mapped.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert!(mapped
        .nodes()
        .iter()
        .any(|n| n.origins[0].provenance == OriginKind::RecordVariant));
    assert!(mapped.symbols().iter().any(
        |s| s.symbol.kind == Kind::Constructor && s.source_path.steps == [AstStep::Variant(0)]
    ));
}

#[test]
fn repeated_nodes_and_synthetic_anchors_survive_module_lowering() {
    let tree = Tree::new();
    tree.write(
        "lib.rill",
        "export fn value(x) { let y = x; 1; 1; if y > 0 { -y } else { -y } }",
    );
    tree.write("main.rill", "import lib; fn main() { lib.value(2) }");
    let mapped = loaded(&tree);
    let ones: Vec<_> = mapped
        .nodes()
        .iter()
        .filter(|n| {
            matches!(
                mapped.node(&n.path),
                Some(AstNode::Expression(Expr::Int(1)))
            )
        })
        .collect();
    assert_eq!(ones.len(), 2);
    assert_ne!(ones[0].path, ones[1].path);
    assert_ne!(ones[0].origins[0].span, ones[1].origins[0].span);
    for provenance in [
        OriginKind::BlockBinding,
        OriginKind::BlockSequence,
        OriginKind::StatementDiscard,
        OriginKind::ImplicitUnit,
        OriginKind::UnaryNegation,
    ] {
        assert!(
            mapped
                .nodes()
                .iter()
                .any(|n| n.origins[0].provenance == provenance),
            "{provenance:?}"
        );
    }
    let binding = mapped
        .nodes()
        .iter()
        .find(|n| n.origins[0].provenance == OriginKind::BlockBinding)
        .unwrap();
    let body = mapped
        .origin(&binding.path.child(AstStep::LetBody))
        .unwrap();
    assert!(body.origins[0].span.start.byte >= binding.origins[0].span.end.byte);
    for node in mapped.nodes() {
        if matches!(
            mapped.node(&node.path),
            Some(AstNode::Expression(Expr::Int(0)))
        ) && node.origins[0].provenance == OriginKind::UnaryNegation
        {
            assert_eq!(mapped.origin_text(&node.origins[0]), Some("-"));
        }
    }
}

#[test]
fn compatible_native_contributors_keep_parameter_names_visibility_and_order() {
    let tree = Tree::new();
    tree.write("main.rill", "import b; import a; @include(\"entry.h\"); @link(\"entry\"); fn main() { a.call(1) + b.call(1) }");
    tree.write("a.rill", "@include(\"a.h\"); @link(\"a\"); @pkg_config(\"package_a\"); extern fn native(x: i32) -> i32; export extern const VALUE: i32; export fn call(x) { native(x) + VALUE }");
    tree.write("b.rill", "@include(\"b.h\"); @link(\"b\"); @pkg_config(\"package_b\"); export extern fn native(different: i32) -> i32; extern const VALUE: i32; export fn call(x) { native(x) + VALUE }");
    let mapped = loaded(&tree);
    assert_eq!(mapped.program().externs.len(), 1);
    assert_eq!(mapped.program().constants.len(), 1);
    assert_eq!(mapped.program().externs[0].params[0].name, "x");
    assert_eq!(mapped.program().native.includes, ["entry.h", "a.h", "b.h"]);
    assert_eq!(mapped.program().native.links, ["entry", "a", "b"]);
    assert_eq!(mapped.program().native.packages, ["package_a", "package_b"]);
    for node in mapped.nodes().iter().filter(|n| {
        matches!(
            n.path.declaration,
            AstDeclaration::ExternFunction(_) | AstDeclaration::Constant(_)
        )
    }) {
        assert_eq!(node.origins.len(), 2);
        assert_eq!(node.origins[1].lowering, Lowering::Unchanged);
    }
    let param = mapped
        .origin(
            &AstPath::root(AstDeclaration::ExternFunction(0)).child(AstStep::ForeignParameter(0)),
        )
        .unwrap();
    assert_eq!(mapped.origin_text(&param.origins[0]), Some("x: i32"));
    assert_eq!(
        mapped.origin_text(&param.origins[1]),
        Some("different: i32")
    );
    let native: Vec<_> = mapped
        .symbols()
        .iter()
        .filter(|s| s.symbol.name == "native")
        .collect();
    assert_eq!(native.len(), 2);
    assert_ne!(native[0].symbol.id, native[1].symbol.id);
    assert_eq!(native[0].target, native[1].target);
    assert!(!native[0].symbol.exported);
    assert!(native[1].symbol.exported);
    tree.write("main.rill", "import a; import b; fn main() { a.call(1) }");
    tree.write(
        "a.rill",
        "extern fn native(a: i32, b: i32) -> i32; export fn call(x) { native(x, x) }",
    );
    tree.write("b.rill", "extern fn native(x: i32, x: i32) -> i32;");
    same_error(&tree, "E-MODULE-CHECK");
    tree.write("b.rill", "extern fn native(x: bool, y: i32) -> i32;");
    same_error(&tree, "E-MODULE-EXTERN");
}

#[test]
fn bundled_allocation_identity_and_all_bundled_sources_are_preserved() {
    let tree = Tree::new();
    tree.write("main.rill", "import std.alloc as a; import std.result as r; import std.json_input; import std.json_output; fn main() { let x = a.box_i32(42); match move x { r.Ok(owner) => box_read(owner) | r.Err(e) => 0 } }");
    let mapped = loaded(&tree);
    let allocation = mapped.program().allocation_types.as_ref().unwrap();
    assert!(mapped.symbols().iter().any(|s| s.symbol.name == "AllocError" && matches!(mapped.node(&s.target), Some(AstNode::TypeDeclaration(t)) if t.name == allocation.error_type)));
    for module in [
        "std.alloc",
        "std.result",
        "std.json_input",
        "std.json_output",
    ] {
        let source = mapped
            .sources()
            .iter()
            .find(|s| s.module == module)
            .unwrap();
        assert_eq!(source.path, PathBuf::from(format!("<{module}>")));
        assert!(!source.text.is_empty());
    }
}

#[test]
fn unicode_snapshots_positions_and_owned_lifetime() {
    let mapped = {
        let tree = Tree::new();
        tree.write("main.rill", "// λ🙂\r\nfn main() { byte_len(\"é🙂\") }");
        let mapped = loaded(&tree);
        fs::write(tree.entry(), "invalid replacement").unwrap();
        mapped
    };
    let moved = std::thread::spawn(move || mapped).join().unwrap();
    verify(&moved);
    let literal = moved
        .nodes()
        .iter()
        .find(|n| matches!(moved.node(&n.path), Some(AstNode::Expression(Expr::Str(_)))))
        .unwrap();
    let origin = &literal.origins[0];
    assert_eq!(moved.origin_text(origin), Some("\"é🙂\""));
    assert_eq!(origin.span.start.line, 2);
    assert_eq!(origin.span.end.byte - origin.span.start.byte, 8);
    assert_eq!(origin.span.end.column - origin.span.start.column, 4);
    assert_eq!(moved.clone().into_program(), *moved.program());
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_are_preserved_without_lossy_conversion() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let tree = Tree::new();
    let relative = PathBuf::from(OsString::from_vec(b"dir-\xff".to_vec()));
    tree.write(relative.join("main.rill"), "fn main() { 42 }");
    let entry = tree.0.join(&relative).join("main.rill");
    let mapped = load_program_with_sources(&entry).unwrap();
    assert_eq!(mapped.program(), &load_program(&entry).unwrap());
    assert_eq!(mapped.sources()[0].path, fs::canonicalize(&entry).unwrap());
    assert!(mapped.sources()[0].path.to_str().is_none());
    verify(&mapped);
}

#[test]
fn rejected_imports_and_privacy_keep_exact_ordinary_errors() {
    let tree = Tree::new();
    tree.write("main.rill", "import absent; fn main() { 0 }");
    same_error(&tree, "E-MODULE-IO");
    tree.write("main.rill", "import a; fn main() { 0 }");
    tree.write("a.rill", "import b;");
    tree.write("b.rill", "import a;");
    same_error(&tree, "E-MODULE-CYCLE");
    tree.write("a.rill", "fn main() { 0 }");
    same_error(&tree, "E-MODULE-ENTRY");
    tree.write(
        "a.rill",
        "type Private = Private export fn leak() { Private }",
    );
    same_error(&tree, "E-MODULE-PRIVATE-TYPE");
    tree.write("a.rill", "export fn broken() { missing }");
    same_error(&tree, "E-MODULE-NAME");
    tree.write("a.rill", "export fn broken( { 0 }");
    same_error(&tree, "E-MODULE-PARSE");
    tree.write("main.rill", "import std.no_such_module; fn main() { 0 }");
    same_error(&tree, "E-MODULE-STDLIB");
    tree.write("main.rill", "import a; import a as again; fn main() { 0 }");
    tree.write("a.rill", "export fn value() { 0 }");
    same_error(&tree, "E-MODULE-DUPLICATE");
}

#[test]
fn graph_source_count_and_depth_limits_are_unchanged() {
    let tree = Tree::new();
    tree.write("main.rill", &format!("//{}", "x".repeat(1024 * 1024)));
    same_error(&tree, "E-MODULE-LIMIT");
    tree.write("main.rill", "import m0; fn main() { 0 }");
    for i in 0..65 {
        tree.write(format!("m{i}.rill"), &format!("import m{};", i + 1));
    }
    tree.write("m65.rill", "");
    same_error(&tree, "E-MODULE-LIMIT");
    for i in 0..256 {
        tree.write(format!("m{i}.rill"), "");
    }
    let imports: String = (0..256).map(|i| format!("import m{i};")).collect();
    tree.write("main.rill", &format!("{imports} fn main() {{ 0 }}"));
    same_error(&tree, "E-MODULE-LIMIT");
}

#[test]
fn all_example_loads_have_exact_old_new_program_or_error_equivalence() {
    let mut work = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")];
    let mut accepted = 0;
    let mut rejected = 0;
    while let Some(path) = work.pop() {
        if path.is_dir() {
            work.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else if path.extension().is_some_and(|e| e == "rill") {
            match (load_program(&path), load_program_with_sources(&path)) {
                (Ok(old), Ok(new)) => {
                    assert_eq!(&old, new.program(), "{}", path.display());
                    verify(&new);
                    accepted += 1;
                }
                (Err(old), Err(new)) => {
                    assert_eq!(
                        (old.code, old.message),
                        (new.code, new.message),
                        "{}",
                        path.display()
                    );
                    rejected += 1;
                }
                (a, b) => panic!("{}: old {a:?}, new {b:?}", path.display()),
            }
        }
    }
    assert!(accepted >= 20, "only {accepted} accepted examples");
    eprintln!("module origin corpus: {accepted} accepted, {rejected} rejected examples");
}

#[cfg(unix)]
#[test]
fn metadata_reads_do_not_launch_native_tools() {
    const CHILD: &str = "RILL_MODULE_ORIGINS_NO_NATIVE_TOOLS";
    if let Some(root) = std::env::var_os(CHILD) {
        let entry = PathBuf::from(root).join("main.rill");
        let ordinary = load_program(&entry).unwrap();
        let mapped = load_program_with_sources(&entry).unwrap();
        assert_eq!(mapped.program(), &ordinary);
        for _ in 0..3 {
            verify(&mapped);
        }
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    tree.write("main.rill", "@include(\"this-header-must-not-exist.h\"); @pkg_config(\"this-package-must-not-exist\"); extern fn native(x: i32) -> i32; fn main() { native(42) }");
    for tool in ["cc", "pkg-config", "clang"] {
        tree.write(
            tool,
            &format!(
                "#!/bin/sh\necho invoked >> '{}'\nexit 99\n",
                tree.0.join("unexpected-tool").display()
            ),
        );
        fs::set_permissions(tree.0.join(tool), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "metadata_reads_do_not_launch_native_tools",
            "--nocapture",
        ])
        .env(CHILD, &tree.0)
        .env("PATH", &tree.0)
        .env("CC", tree.0.join("cc"))
        .env("PKG_CONFIG", tree.0.join("pkg-config"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!tree.0.join("unexpected-tool").exists());
}

#[cfg(unix)]
#[test]
fn symlink_escape_and_duplicate_file_identities_are_still_rejected() {
    use std::os::unix::fs::symlink;
    let outside = Tree::new();
    outside.write("lib.rill", "export fn value() { 1 }");
    let tree = Tree::new();
    tree.write("main.rill", "import escape; fn main() { escape.value() }");
    symlink(outside.0.join("lib.rill"), tree.0.join("escape.rill")).unwrap();
    same_error(&tree, "E-MODULE-PATH");
    tree.write("main.rill", "import a; import b; fn main() { 0 }");
    tree.write("a.rill", "export fn value() { 1 }");
    fs::hard_link(tree.0.join("a.rill"), tree.0.join("b.rill")).unwrap();
    same_error(&tree, "E-MODULE-DUPLICATE");
}

#[test]
fn source_mapping_and_rewriting_fit_a_two_mib_stack_near_parser_limits() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let tree = Tree::new();
            tree.write(
                "lib.rill",
                "export type Nest<a> = Nest(a) export fn value(x) { x + 1 }",
            );
            for body in [
                format!("0{}", "+1".repeat(60)),
                format!("{}lib.value(x)", "let x = 1;".repeat(50)),
            ] {
                tree.write("main.rill", &format!("import lib; fn main() {{ {body} }}"));
                loaded(&tree);
            }
            tree.write(
                "main.rill",
                &format!(
                    "import lib; fn deep(x: {}i32{}) {{ 0 }} fn main() {{ 0 }}",
                    "lib.Nest<".repeat(40),
                    ">".repeat(40)
                ),
            );
            loaded(&tree);
        })
        .unwrap()
        .join()
        .unwrap();
}
