use std::{fs, process::Command};

#[test]
fn boolean_negation_matches_interpreter_and_optimized_c() {
    for (index, (body, expected)) in [
        ("not false", "true"),
        ("not true", "false"),
        ("!false", "true"),
        ("!!true", "true"),
        ("not not true", "true"),
        ("not false && false", "false"),
        ("not (false && false)", "true"),
        ("not true == false", "true"),
        ("not false || (1 / 0 == 0)", "true"),
        ("not true && (1 / 0 == 0)", "false"),
        (
            "let mut n = 0; let result = not tick(inout n); if result { 0 } else { n }",
            "1",
        ),
        ("let not_ready = false; not not_ready", "true"),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!("fn tick(inout n) {{ n <- n + 1; true }} fn main() {{ {body} }}");
        let program = rill::parser::parse(&source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        let typed = rill::typed::lower_main(&checked).unwrap();
        assert_eq!(rill::typed::evaluate(&typed).unwrap().to_string(), expected);
        assert_eq!(
            rill::interpreter::run_reference(&program)
                .unwrap()
                .to_string(),
            expected
        );
        assert_eq!(
            rill::interpreter::run(&program).unwrap().to_string(),
            expected
        );
        let dir = std::env::temp_dir().join(format!("rill-not-{}-{index}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let source_file = dir.join("main.c");
        let executable = dir.join("program");
        fs::write(&source_file, rill::backend::emit(&program).unwrap()).unwrap();
        for optimization in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", optimization])
                .arg(&source_file)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(&executable).output().unwrap();
            assert!(output.status.success(), "{body}");
            assert_eq!(output.stdout, format!("{expected}\n").as_bytes(), "{body}");
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn negation_rejects_non_booleans_and_missing_operands() {
    for operand in ["42", "()", "\"false\"", "system_box_i32_abort(1)"] {
        for operator in ["not", "!"] {
            let source = format!("fn main() {{ {operator} {operand} }}");
            let program = rill::parser::parse(&source).unwrap();
            assert!(rill::check::check(&program).is_err(), "{source}");
            assert!(rill::backend::emit(&program).is_err(), "{source}");
        }
    }
    for source in ["fn main() { not }", "fn main() { ! }", "fn main() { not; }"] {
        assert!(rill::parser::parse(source).is_err(), "{source}");
    }
    let source = format!("fn main() {{ {}true }}", "not ".repeat(1000));
    assert!(rill::parser::parse(&source)
        .unwrap_err()
        .contains("E-PARSE-DEPTH"));
}
