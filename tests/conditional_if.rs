use rill::{
    ast::Expr,
    source_map::{AstNode, AstPath, AstStep, OriginKind},
    syntax::{AstDeclaration, SyntaxKind, SyntaxTree},
};
use std::{fs, path::PathBuf, process::Command};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn native(source: &str, expected: &str, case: usize) {
    let program = rill::parser::parse(source).unwrap();
    let checked = rill::check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    assert_eq!(rill::typed::evaluate(&typed).unwrap().to_string(), expected);
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected
    );
    if !checked.has_owned() {
        assert_eq!(
            rill::interpreter::run_reference(&program)
                .unwrap()
                .to_string(),
            expected
        );
    }
    let directory = Directory(
        std::env::temp_dir().join(format!("rill conditional {} {case}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    let c = directory.0.join("main.c");
    let executable = directory.0.join("program");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&c)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, format!("{expected}\n").as_bytes());
    }
}

#[test]
fn omitted_else_is_unit_and_conditional_effects_do_not_return_early() {
    for (case, (source, expected)) in [
        ("fn main() { if true {} }", "()"),
        ("fn main() { if false { 42; } }", "()"),
        ("fn main() { let mut n = 0; if true { n <- n + 1; }; if false { n <- 999; }; n }", "1"),
        ("fn tick(inout n) { n <- n + 1; true } fn main() { let mut n = 0; if tick(inout n) { n <- n + 41; }; n }", "42"),
        ("fn tick(inout n) { n <- n + 1; false } fn main() { let mut n = 0; if tick(inout n) { n <- 999; }; n }", "1"),
        ("fn main() { let mut n = 0; if true { if false { n <- 1; }; n <- 42; }; n }", "42"),
        ("fn main() { let mut n = 0; if true then n <- 42 else (); n }", "42"),
    ].into_iter().enumerate() {
        native(source, expected, case);
    }
}

#[test]
fn omitted_else_rejects_non_unit_then_branches_and_non_boolean_conditions() {
    for source in [
        "fn main() { if true { 42 } }",
        "fn main() { if false { 42 } }",
        "fn main() { if 42 {} }",
        "fn main() { if true { system_box_i32_abort(42) } }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&program).is_err(), "{source}");
        assert!(rill::backend::emit(&program).is_err(), "{source}");
    }
}

#[test]
fn malformed_conditionals_and_missing_statement_semicolons_still_fail() {
    for source in [
        "fn main() { if true {} 42 }",
        "fn main() { if true {}\nlet value = 42; value }",
        "fn main() { if true then () }",
        "fn main() { if true { } else }",
        "fn main() { if true { } else 42 }",
        "fn main() { if { } }",
        "fn main() { if true {",
    ] {
        assert!(rill::parser::parse(source).is_err(), "{source}");
    }
}

#[test]
fn omitted_else_has_a_complete_zero_width_implicit_unit_origin() {
    let source = "// 😀\nfn main() { if true {} }";
    let tree = SyntaxTree::parse(source);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let map = tree.ast_source_map().unwrap();
    let body = AstPath::root(AstDeclaration::Function(0)).child(AstStep::FunctionBody);
    assert!(
        matches!(body.resolve(tree.parsed_module().unwrap()), Some(AstNode::Expression(Expr::If(_, _, otherwise))) if **otherwise == Expr::Unit)
    );
    let otherwise = map.origin(&body.child(AstStep::Else)).unwrap();
    assert_eq!(otherwise.provenance, OriginKind::ImplicitUnit);
    let if_node = tree.node(otherwise.syntax).unwrap();
    assert_eq!(if_node.kind(), SyntaxKind::IfExpression);
    assert_eq!(otherwise.span.start, if_node.span().end);
    assert_eq!(otherwise.span.start, otherwise.span.end);
    let then = map.origin(&body.child(AstStep::Then)).unwrap();
    assert!(then.span.start.byte < otherwise.span.start.byte);
    for origin in map.entries() {
        assert_eq!(
            origin
                .path
                .resolve(tree.parsed_module().unwrap())
                .unwrap()
                .kind(),
            origin.kind
        );
    }
    let output = rill::formatter::format(source).unwrap();
    assert_eq!(rill::formatter::format(&output).unwrap(), output);
    assert_eq!(
        rill::parser::parse_module(source).unwrap(),
        rill::parser::parse_module(&output).unwrap()
    );
    assert_eq!(tree.reconstruct(), source);
}

#[test]
fn conditional_owner_locals_clean_up_only_on_the_taken_branch() {
    let source = "fn main() { let keep = system_box_i32_abort(42); if true { let temporary = system_box_i32_abort(7); box_read(temporary); }; if false { let never = system_box_i32_abort(99); box_read(never); }; box_read(keep) }";
    native(source, "42", 100);
    let directory = Directory(
        std::env::temp_dir().join(format!("rill conditional cleanup {}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    fs::write(
        directory.0.join("generated.c"),
        rill::compile(source).unwrap(),
    )
    .unwrap();
    let harness = directory.0.join("harness.c");
    fs::write(
        &harness,
        r#"#define main generated_main
#include "generated.c"
#undef main
static void *allocate(size_t size) { void *p = malloc(size); fputs("A\n", stderr); return p; }
static void release(void *p) { fprintf(stderr, "F%d\n", *(int32_t *)p); free(p); }
int main(void) { rl_allocate = allocate; rl_release = release; return generated_main(); }
"#,
    )
    .unwrap();
    let executable = directory.0.join("program");
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&harness)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"42\n");
        assert_eq!(output.stderr, b"A\nA\nF7\nF42\n");
    }
}
