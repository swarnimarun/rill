use rill::{backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "rill-string-equality-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn run_c(&self, c: &str, expected: &str, label: &str) {
        let source = self.0.join("program.c");
        let executable = self.0.join("program");
        fs::write(&source, c).unwrap();
        for optimization in ["-O0", "-O3"] {
            let build = Command::new("cc")
                .args(["-std=c11", optimization])
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{label} {optimization}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{label} {optimization}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(
                run.stdout,
                format!("{expected}\n").as_bytes(),
                "{label} {optimization}"
            );
            assert!(run.stderr.is_empty(), "{label} {optimization}");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compare_all_paths(source: &str, expected: &str) {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let ir = typed::lower_main(&checked).unwrap();
    typed::verify(&ir).unwrap();
    assert_eq!(typed::evaluate(&ir).unwrap().to_string(), expected);
    assert_eq!(interpreter::run(&program).unwrap().to_string(), expected);
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        expected
    );
    let production = backend::emit(&program).unwrap();
    assert_eq!(production, typed::emit_c(&ir).unwrap());
    let fixture = Fixture::new();
    fixture.run_c(&production, expected, "typed C");
    fixture.run_c(
        &backend::emit_reference(&program).unwrap(),
        expected,
        "reference C",
    );
}

fn quoted(value: &str) -> String {
    let mut literal = String::from("\"");
    for character in value.chars() {
        match character {
            '\0' => literal.push_str("\\0"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            other => literal.push(other),
        }
    }
    literal.push('"');
    literal
}

#[test]
fn string_pairs_compare_exact_bytes_and_lengths_in_all_backends() {
    let corpus = [
        "",
        "one",
        "One",
        "on",
        "one!",
        "\0",
        "\0x",
        "\0y",
        "x\0",
        "x\0y",
        "xy",
        "é",
        "e\u{301}",
        "😀",
        "🤖",
        "\"\\\n\r\t",
    ];
    let mut source = String::new();
    let mut count = 0_usize;
    for left in corpus {
        for right in corpus {
            let equal = left.as_bytes() == right.as_bytes();
            source.push_str(&format!(
                "fn case{count}() {{ ({} == {}) == {equal} && ({} != {}) == {} }}\n",
                quoted(left),
                quoted(right),
                quoted(left),
                quoted(right),
                !equal
            ));
            count += 1;
        }
    }
    for group in 0..count.div_ceil(16) {
        let calls = (group * 16..((group + 1) * 16).min(count))
            .map(|index| format!("case{index}()"))
            .collect::<Vec<_>>()
            .join(" && ");
        source.push_str(&format!("fn group{group}() {{ {calls} }}\n"));
    }
    let calls = (0..count.div_ceil(16))
        .map(|group| format!("group{group}()"))
        .collect::<Vec<_>>()
        .join(" && ");
    source.push_str(&format!("fn main() {{ {calls} }}\n"));
    compare_all_paths(&source, "true");
}

#[test]
fn strings_from_functions_records_and_enum_matches_compare_as_contents() {
    compare_all_paths(
        r#"
        type Label = { text: str }
        type Token = Word(str) | Missing
        fn identity(value) { value }
        fn choose(flag: bool) -> str { if flag { "café" } else { "other" } }
        fn unpack(token: Token) -> str {
            match token { Word(text) => text | Missing => "" }
        }
        fn equal(left: str, right) { left == right }
        fn main() {
            let value = identity(choose(true));
            let label = record Label { text = value };
            let token = Word("café");
            equal(label.text, unpack(token)) && label.text != "CAFE" &&
            (match token { Word(text) => text == "café" | Missing => false })
        }
        "#,
        "true",
    );
}

#[test]
fn both_operands_execute_once_from_left_to_right() {
    compare_all_paths(
        r#"
        fn tick(inout counter: i32, digit: i32, text: str) -> str {
            counter <- counter * 10 + digit;
            text
        }
        fn main() {
            let mut counter = 0;
            let equal = tick(inout counter, 1, "left") == tick(inout counter, 2, "right");
            let different = tick(inout counter, 3, "same") != tick(inout counter, 4, "same");
            if !equal && !different { counter } else { 0 }
        }
        "#,
        "1234",
    );
}

#[test]
fn discarded_comparisons_and_branch_conditions_keep_boolean_results() {
    compare_all_paths(
        r#"
        fn equal(left: str, right) { left == right }
        fn main() {
            "a" == "b";
            equal("x", "x");
            let selected = if "a" != "b" { "yes" } else { "no" };
            if selected == "yes" { 42 } else { 0 }
        }
        "#,
        "42",
    );
}

#[test]
fn string_comparison_is_available_in_the_checked_owner_path() {
    let program = parser::parse(
        r#"
        fn equal(left: str, right: str) { left == right }
        fn main() {
            let owner = system_box_i32_abort(42);
            if equal("a\0b", "a\0b") && "a\0b" != "a\0c" {
                box_read(owner)
            } else { 0 }
        }
        "#,
    )
    .unwrap();
    let checked = check::analyze(&program).unwrap();
    assert!(checked.has_owned());
    let ir = typed::lower_main(&checked).unwrap();
    typed::verify(&ir).unwrap();
    assert_eq!(typed::evaluate(&ir).unwrap().to_string(), "42");
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
    assert!(interpreter::run_reference(&program)
        .unwrap_err()
        .contains("E-REFERENCE"));
    assert!(backend::emit_reference(&program)
        .unwrap_err()
        .contains("E-REFERENCE"));
    Fixture::new().run_c(&backend::emit(&program).unwrap(), "42", "owner C");
}

#[test]
fn equality_does_not_admit_mixed_strings_or_structural_aggregate_comparison() {
    for source in [
        r#"fn main() { "one" == 1 }"#,
        r#"fn main() { 1 == "one" }"#,
        r#"fn main() { "one" != true }"#,
        r#"fn main() { c"one" == "one" }"#,
        r#"fn main() { c"one" == c"one" }"#,
        "fn main() { bytes_new(1) == bytes_new(1) }",
        r#"fn main() { "one" < "two" }"#,
        r#"fn main() { "one" <= "two" }"#,
        "type Point = { x: i32 } fn main() { record Point { x = 1 } == record Point { x = 1 } }",
        "type Color = Red | Blue fn main() { Red == Red }",
        r#"type Word = Word(str) fn main() { Word("one") != Word("two") }"#,
    ] {
        let program = parser::parse(source).unwrap();
        assert!(check::check(&program).is_err(), "checked {source}");
        assert!(interpreter::run(&program).is_err(), "interpreted {source}");
        assert!(
            interpreter::run_reference(&program).is_err(),
            "reference interpreted {source}"
        );
        assert!(backend::emit(&program).is_err(), "emitted {source}");
        assert!(
            backend::emit_reference(&program).is_err(),
            "reference emitted {source}"
        );
    }
}

#[test]
fn c_runtime_compares_distinct_arrays_without_pointer_or_nul_shortcuts() {
    let program = parser::parse(r#"fn main() { "x" == "x" }"#).unwrap();
    let typed_c = backend::emit(&program).unwrap();
    let reference_c = backend::emit_reference(&program).unwrap();
    let arrays = r#"
    unsigned char left[] = {97, 0, 120};
    unsigned char same[] = {97, 0, 120};
    unsigned char other[] = {97, 0, 121};
    unsigned char shorter[] = {97, 0};
    "#;
    let typed_c = format!(
        "#define main rill_generated_main\n{typed_c}\n#undef main\nint main(void) {{\n{arrays}\n\
        rl_str a = {{left, 3}}, b = {{same, 3}}, c = {{other, 3}}, d = {{shorter, 2}};\n\
        int valid = rl_str_equal(a, b) && !rl_str_equal(a, c) && !rl_str_equal(a, d)\n\
          && rl_str_equal((rl_str){{left, 0}}, (rl_str){{other, 0}});\n\
        printf(\"%s\\n\", valid ? \"true\" : \"false\");\nreturn 0;\n}}\n"
    );
    let reference_c = format!(
        "#define main rill_generated_main\n{reference_c}\n#undef main\nint main(void) {{\n{arrays}\n\
        rl_value a = rl_string(left, 3), b = rl_string(same, 3), c = rl_string(other, 3), d = rl_string(shorter, 2);\n\
        rl_value same_result = rl_compare(0, a, b), other_result = rl_compare(1, a, c);\n\
        rl_value shorter_result = rl_compare(0, a, d);\n\
        int valid = same_result.tag == 1 && same_result.value == 1\n\
          && other_result.tag == 1 && other_result.value == 1\n\
          && shorter_result.tag == 1 && shorter_result.value == 0\n\
          && rl_compare(0, rl_string(left, 0), rl_string(other, 0)).value == 1;\n\
        printf(\"%s\\n\", valid ? \"true\" : \"false\");\nreturn 0;\n}}\n"
    );
    let fixture = Fixture::new();
    fixture.run_c(&typed_c, "true", "distinct arrays typed C");
    fixture.run_c(&reference_c, "true", "distinct arrays reference C");
}
