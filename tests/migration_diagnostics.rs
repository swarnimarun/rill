use rill::{check, interpreter, modules, parser, syntax::SyntaxTree};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn reject(source: &str) -> String {
    match parser::parse(source) {
        Err(error) => error,
        Ok(program) => check::check(&program).expect_err("source must be rejected"),
    }
}

#[test]
fn rust_declaration_help_keeps_the_original_error_and_location() {
    for (source, hint) in [
        ("struct Point { value: i32 }", "type Name = { field: i32 }"),
        (
            "enum Choice { First, Second }",
            "type Name = First | Second(i32)",
        ),
        ("pub fn main() { 42 }", "export fn"),
        ("use utils;", "import module;"),
        ("mod utils;", "Qualified names use dots"),
        (
            "impl Point {}",
            "Traits and impl blocks are not implemented",
        ),
        (
            "trait Read {}",
            "Traits and impl blocks are not implemented",
        ),
        (
            "async fn main() { 42 }",
            "Async functions are not implemented",
        ),
    ] {
        let error = parser::parse_module(source).unwrap_err();
        assert!(
            error.starts_with("1:1: expected Let (found Ident("),
            "{error}"
        );
        assert!(error.contains(hint), "{error}");
    }
}

#[test]
fn local_annotation_assignment_and_macro_errors_explain_supported_syntax() {
    for (source, original, hint) in [
        (
            "fn main() { let value: i32 = 42; value }",
            "1:22: expected Equal",
            "Omit `: Type` to infer the local type",
        ),
        (
            "let main = let value: i32 = 42 in value",
            "1:21: expected Equal",
            "parameters and function results can be annotated",
        ),
        (
            "fn main() { let mut value = 1; value = 42; value }",
            "expected ';' between block statements",
            "Use `<-` to mutate a `let mut` binding, or `==` to compare values",
        ),
        (
            "fn main() { println!(42); }",
            "expected ';' between block statements",
            "Rust-style `name!(...)` macros are not implemented",
        ),
    ] {
        let error = reject(source);
        assert!(error.contains(original), "{error}");
        assert!(error.contains(hint), "{error}");
    }
}

#[test]
fn unsupported_control_words_only_receive_help_when_undefined() {
    for (word, alternative) in [
        ("break", "mutable boolean"),
        ("continue", "Guard the remaining loop body with `if`"),
        ("return", "final expression"),
    ] {
        let error = reject(&format!("fn main() {{ {word}; }}"));
        assert!(
            error.contains(&format!("E-NAME: unknown variable '{word}'")),
            "{error}"
        );
        assert!(error.contains(alternative), "{error}");
        assert!(
            error.contains("docs/rust-to-rill.md#control-flow"),
            "{error}"
        );
    }
    let error = reject("fn main() { return 42; }");
    assert!(
        error.contains("E-NAME: unknown function 'return'"),
        "{error}"
    );
    assert!(error.contains("final expression"), "{error}");

    let source = "fn break(value) { value } fn continue(value) { break(value) } fn return(value) { continue(value) } fn struct(value) { value } fn main() { let break = 1; let continue = 2; let return = 39; struct(break + continue + return) }";
    let program = parser::parse(source).unwrap();
    check::check(&program).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
}

#[test]
fn loop_and_for_parse_help_does_not_reserve_custom_names() {
    for (source, hint) in [
        (
            "fn main() { loop { 42; } }",
            "If you intended an unconditional `loop` expression",
        ),
        (
            "fn main() { for item in items { item; } }",
            "If you intended a `for name in ...` loop",
        ),
    ] {
        let error = reject(source);
        assert!(error.contains("expected ';'"), "{error}");
        assert!(error.contains(hint), "{error}");
        assert!(error.contains("while"), "{error}");
    }
    let program = parser::parse("fn loop(value) { value } fn for(value) { loop(value) } fn main() { let loop = 20; let for = 22; loop + for }").unwrap();
    check::check(&program).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
}

#[test]
fn lexical_rejections_offer_help_without_changing_boolean_negation() {
    for (source, hint) in [
        (
            "fn main() { value? }",
            "Handle a result explicitly with `match`",
        ),
        (
            "fn main() { &value }",
            "Borrowed parameters use `read` or `inout`",
        ),
    ] {
        let error = reject(source);
        assert!(error.contains("unexpected character"), "{error}");
        assert!(error.contains(hint), "{error}");
    }
    let program = parser::parse("fn main() { if !false { 42 } else { 0 } }").unwrap();
    check::check(&program).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
}

#[test]
fn suggestions_cover_values_constructors_types_records_and_fields() {
    for (source, suggestion) in [
        ("fn calculate(value) { value } fn main() { calcualte(42) }", "calculate"),
        ("fn main() { let value = 42; vaule }", "value"),
        ("type Choice = First | Second; fn main() { Frist }", "First"),
        ("type Value = Wrap(i32); fn inspect(value: Vaule) { 42 } fn main() { 42 }", "Value"),
        ("fn inspect(value: bol) { 42 } fn main() { 42 }", "bool"),
        ("type Point = { value: i32 }; fn main() { record Piont { value = 42 } }", "Point"),
        ("type Point = { value: i32 }; fn main() { record Point { vaule = 42 } }", "value"),
        ("type Point = { value: i32 }; fn main() { let point = record Point { value = 42 }; point.vaule }", "value"),
    ] {
        let error = reject(source);
        assert!(error.contains(&format!("did you mean '{suggestion}'?")), "{error}");
    }
}

#[test]
fn ambiguous_short_and_distant_names_have_no_suggestion() {
    for source in [
        "fn cat() { 1 } fn cut() { 2 } fn main() { cot() }",
        "fn xyz() { 42 } fn main() { xy() }",
        "fn calculate() { 42 } fn main() { different() }",
        "type Point = { cat: i32; cut: i32 }; fn main() { let point = record Point { cat = 1; cut = 2 }; point.cot }",
    ] {
        for _ in 0..8 {
            assert!(!reject(source).contains("did you mean"));
        }
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rill-migration-diagnostics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn error(&self, entry: &str, library: &str) -> String {
        fs::write(self.0.join("main.rill"), entry).unwrap();
        fs::write(self.0.join("utils.rill"), library).unwrap();
        modules::compile::load_program(&self.0.join("main.rill"))
            .unwrap_err()
            .to_string()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn module_suggestions_respect_exports_namespaces_and_local_scope() {
    let project = Project::new();
    for (entry, library, expected) in [
        (
            "import utils; fn main() { utils.calcualte(42) }",
            "export fn calculate(value) { value }",
            "did you mean 'calculate'?",
        ),
        (
            "import utils; fn main() { utlis.calculate(42) }",
            "export fn calculate(value) { value }",
            "did you mean 'utils'?",
        ),
        (
            "import utils; fn inspect(value: ii32) { value } fn main() { 42 }",
            "export fn calculate(value) { value }",
            "did you mean 'i32'?",
        ),
        (
            "import utils; fn inspect(value: utlis.Payload) { 42 } fn main() { 42 }",
            "export type Payload = Wrap(i32)",
            "did you mean 'utils'?",
        ),
        (
            "import utils; fn main() { let value = 42; vaule }",
            "export fn calculate(value) { value }",
            "did you mean 'value'?",
        ),
        (
            "import utils; fn main() { continue; }",
            "export fn calculate(value) { value }",
            "Guard the remaining loop body",
        ),
    ] {
        let error = project.error(entry, library);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("M$"), "{error}");
    }

    for (entry, library, private_name) in [
        (
            "import utils; fn main() { utils.calcualte_secret(42) }",
            "fn calculate_secret(value) { value } export fn public(value) { value }",
            "calculate_secret",
        ),
        (
            "import utils; fn inspect(value: utils.Paylod) { 42 } fn main() { 42 }",
            "export fn Payload(value) { value }",
            "Payload",
        ),
        (
            "import utils; fn main() { utils.Paylod(42) }",
            "export type Payload = Wrap(i32)",
            "Payload",
        ),
    ] {
        let error = project.error(entry, library);
        assert!(!error.contains("did you mean"), "{error}");
        assert!(!error.contains(private_name), "{error}");
        assert!(!error.contains("M$"), "{error}");
    }
    let error = project.error(
        "import utils; fn main() { utils.secret(42) }",
        "fn secret(value) { value }",
    );
    assert!(error.contains("E-MODULE-PRIVATE"), "{error}");
    assert!(!error.contains("did you mean"), "{error}");
}

#[test]
fn cli_type_typo_is_helpful_before_the_checker_runs() {
    let project = Project::new();
    let entry = project.0.join("main.rill");
    fs::write(&entry, "fn inspect(value: ii32) { value } fn main() { 42 }").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("check")
        .arg(entry)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("E-MODULE-NAME"), "{error}");
    assert!(error.contains("did you mean 'i32'?"), "{error}");
}

#[test]
fn qualified_control_word_members_are_ordinary_exported_names() {
    let project = Project::new();
    for word in ["break", "continue", "return"] {
        let error = project.error(
            &format!("import utils; fn main() {{ utils.{word}(42) }}"),
            "export fn identity(value) { value }",
        );
        assert!(error.contains("E-MODULE-NAME"), "{error}");
        assert!(error.contains(&format!("unknown symbol {word}")), "{error}");
        assert!(!error.contains("not built-in control flow"), "{error}");
        assert!(
            !error.contains("docs/rust-to-rill.md#control-flow"),
            "{error}"
        );

        fs::write(
            project.0.join("utils.rill"),
            format!("export fn {word}(value) {{ value }}"),
        )
        .unwrap();
        let program = modules::compile::load_program(&project.0.join("main.rill")).unwrap();
        assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
    }
}

#[test]
fn helpful_syntax_errors_preserve_lossless_recovery() {
    let source = "struct Point { value: i32 }\nfn valid() { 1 }\nfn main() { 42 }\n";
    let tree = SyntaxTree::parse(source);
    assert_eq!(tree.reconstruct(), source);
    assert_eq!(
        tree.diagnostics()[0].message,
        parser::parse_module(source).unwrap_err()
    );
    assert!(tree.diagnostics()[0].message.contains("Declare a record"));
    assert!(tree
        .nodes()
        .any(|(id, _)| tree.node_text(id) == Some("fn valid() { 1 }")));
    assert!(tree
        .nodes()
        .any(|(id, _)| tree.node_text(id) == Some("fn main() { 42 }")));
}
