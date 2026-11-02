use std::{fs, process::Command};

fn run_both(source: &str, expected: &str, case: &str) {
    let program = rill::parser::parse(source).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected
    );
    let c = rill::backend::emit(&program).unwrap();
    let dir = std::env::temp_dir().join(format!("rill-braces-{}-{case}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let cfile = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&cfile, c).unwrap();
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
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
        let result = Command::new(&binary).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!("{expected}\n")
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn blocks_control_flow_and_parameter_modes() {
    run_both(
        include_str!("../examples/blocks/main.rill"),
        "42",
        "example",
    );
    run_both(
        r#"
        fn inspect(read value: i32) { value }
        fn main() {
            let mut value = 1;
            { let value = 100; inspect(value); };
            while value < 4 { value <- value + 1; };
            let answer = if value == 4 { 42 } else { 0 };
            answer
        }
    "#,
        "42",
        "control",
    );
}

#[test]
fn empty_and_terminated_blocks_are_unit() {
    for source in [
        "fn main() {}",
        "fn main() { 42; }",
        "fn main() { let x = 42; }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "()");
    }
}

#[test]
fn statements_preserve_evaluation_order() {
    run_both(
        r#"
        fn tick(inout value) { value <- value * 10 + 1; value }
        fn main() {
            let mut value = 0;
            tick(inout value);
            tick(inout value);
            value + 31
        }
    "#,
        "42",
        "order",
    );
}

#[test]
fn owner_statements_release_at_semicolon_and_bindings_at_block_exit() {
    let source = r#"fn main() {
        let a = system_box_i32_abort(20);
        system_box_i32_abort(99);
        let b = system_box_i32_abort(22);
        let moved = move a;
        box_read(moved) + box_read(b)
    }"#;
    run_both(source, "42", "owners");
    run_both(
        "fn make() { let owner = system_box_i32_abort(42); move owner } fn main() { let owner = make(); box_read(owner) }",
        "42", "owner-return",
    );
    let moved = rill::parser::parse(
        "fn main() { let owner = system_box_i32_abort(42); move owner; box_read(owner) }",
    )
    .unwrap();
    assert!(rill::check::check(&moved).is_err());
    let generated = rill::compile(source).unwrap();
    let dir = std::env::temp_dir().join(format!("rill-braces-cleanup-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("generated.c"), generated).unwrap();
    fs::write(
        dir.join("harness.c"),
        r#"
#define main generated_main
#include "generated.c"
#undef main
static void *traced_allocate(size_t size) {
    void *p = malloc(size);
    fputs("A\n", stderr);
    return p;
}
static void traced_release(void *p) {
    fprintf(stderr, "F%d\n", *(int32_t *)p);
    free(p);
}
int main(void) {
    rl_allocate = traced_allocate;
    rl_release = traced_release;
    return generated_main();
}
"#,
    )
    .unwrap();
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(dir.join("harness.c"))
            .arg("-o")
            .arg(dir.join("program"))
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(dir.join("program")).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"42\n");
        assert_eq!(output.stderr, b"A\nA\nF99\nA\nF20\nF22\n");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_delimiters_report_the_offending_token() {
    for (source, location, message) in [
        (
            "fn main() {\n let x = 1\n x\n}",
            "3:2:",
            "expected ';' after local binding",
        ),
        (
            "fn main() {\n 1\n 2\n}",
            "3:2:",
            "expected ';' between block statements",
        ),
        ("fn main() {\n 1", "2:3:", "expected '}'"),
        ("fn main() 1", "1:11:", "expected '{'"),
        ("fn main() { let x = ; }", "1:21:", "expected expression"),
        (
            "fn main() { if true { 1 } else 2 }",
            "1:32:",
            "expected '{'",
        ),
        ("fn main() { while true 1 }", "1:24:", "expected '{' block"),
    ] {
        let error = rill::parser::parse(source).unwrap_err();
        assert!(error.starts_with(location), "{error}");
        assert!(error.contains(message), "{error}");
    }
    assert!(rill::parser::parse("fn main() { let x = 1\n; x }").is_ok());
    assert!(rill::parser::parse("fn main() { let x = 1\n x }").is_err());
}

#[test]
fn braced_syntax_keeps_legacy_expressions_and_module_exports() {
    let source = "type Option<a> = None | Some(a); fn id(x) { x } let legacy x = x + 1; fn main() { match Some(id(41)) { Some(x) => legacy(x) | None => 0 } }";
    run_both(source, "42", "legacy");
    let module = rill::parser::parse_module("export fn answer() { 42 }").unwrap();
    assert_eq!(module.exports[0].name, "answer");
}

#[test]
fn block_nesting_and_statement_depth_are_bounded() {
    for source in [
        format!("fn main() {{ {}0{} }}", "{".repeat(1000), "}".repeat(1000)),
        format!("fn main() {{ {}0 }}", "0;".repeat(1000)),
    ] {
        assert!(rill::parser::parse(&source)
            .unwrap_err()
            .contains("E-PARSE-DEPTH"));
    }
}
