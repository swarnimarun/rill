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
            "rill-stdlib-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn compare(source: &str, expected: &str) {
    let tree = Tree::new();
    let entry = tree.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected
    );
    let source = tree.0.join("program.c");
    fs::write(&source, rill::backend::emit(&program).unwrap()).unwrap();
    let binary = tree.0.join("program");
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&source)
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
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
    }
}

fn source_with_cases(cases: &[String]) -> String {
    let mut source = String::from("import std.cli as cli; import std.result as result;\n");
    for (index, body) in cases.iter().enumerate() {
        source.push_str(&format!("fn case{index}() {{ {body} }}\n"));
    }
    source.push_str("fn main() { ");
    source.push_str(
        &(0..cases.len())
            .map(|index| format!("case{index}()"))
            .collect::<Vec<_>>()
            .join(" && "),
    );
    source.push_str(" }\n");
    source
}

#[test]
fn classification_and_byte_spans_match_in_interpreter_and_c() {
    let mut cases = Vec::new();
    for (text, expected) in [
        ("", "cli.Positional(s) => s.start == 0 && s.end == 0"),
        ("-", "cli.Positional(s) => s.start == 0 && s.end == 1"),
        ("input.txt", "cli.Positional(s) => s.start == 0 && s.end == 9"),
        ("é.txt", "cli.Positional(s) => s.start == 0 && s.end == 6"),
        ("-42", "cli.Positional(s) => s.start == 0 && s.end == 3"),
        ("-0", "cli.Positional(s) => s.start == 0 && s.end == 2"),
        ("--", "cli.EndOptions => true"),
        ("-h", "cli.ShortOption(c) => c == 104"),
        ("-Z", "cli.ShortOption(c) => c == 90"),
        ("--help", "cli.LongOption(s) => s.start == 2 && s.end == 6 && cli.span_equal(text, s, \"help\")"),
        ("--foo-bar_2", "cli.LongOption(s) => cli.span_equal(text, s, \"foo-bar_2\")"),
        ("--count=42", "cli.LongValue(n, v) => n.start == 2 && n.end == 7 && v.start == 8 && v.end == 10 && cli.span_equal(text, n, \"count\") && cli.span_equal(text, v, \"42\")"),
        ("--name=", "cli.LongValue(n, v) => cli.span_equal(text, n, \"name\") && v.start == 7 && v.end == 7 && cli.span_equal(text, v, \"\")"),
        ("--name=a=b", "cli.LongValue(n, v) => cli.span_equal(text, v, \"a=b\")"),
        ("--name=é", "cli.LongValue(n, v) => v.start == 7 && v.end == 9 && cli.span_equal(text, v, \"é\")"),
        ("--=bad", "cli.InvalidOption(s) => s.start == 0 && s.end == 6"),
        ("---bad", "cli.InvalidOption(s) => s.end == 6"),
        ("--1bad", "cli.InvalidOption(s) => s.end == 6"),
        ("--a.b", "cli.InvalidOption(s) => s.end == 5"),
        ("--é", "cli.InvalidOption(s) => s.end == 4"),
        ("-abc", "cli.InvalidOption(s) => s.end == 4"),
        ("-h=yes", "cli.InvalidOption(s) => s.end == 6"),
        ("-?", "cli.InvalidOption(s) => s.end == 2"),
    ] {
        let literal = serde_json::to_string(text).unwrap();
        cases.push(format!(
            "let text = {literal}; match cli.classify(text, true) {{ {expected} | _ => false }}"
        ));
        cases.push(format!(
            "let text = {literal}; match cli.classify(text, false) {{ cli.Positional(s) => s.start == 0 && s.end == byte_len(text) | _ => false }}"
        ));
    }
    for (start, end, expected, result) in [
        (0, 3, "abc", true),
        (1, 3, "bc", true),
        (3, 3, "", true),
        (0, 3, "abd", false),
        (0, 2, "abc", false),
        (-1, 2, "ab", false),
        (2, 1, "", false),
        (0, 4, "abc", false),
        (i32::MAX, i32::MAX, "", false),
    ] {
        cases.push(format!(
            "cli.span_equal(\"abc\", record cli.Span {{ start = {start}; end = {end} }}, \"{expected}\") == {result}"
        ));
    }
    compare(&source_with_cases(&cases), "true");
}

#[test]
fn integer_values_and_exact_error_offsets_match_in_interpreter_and_c() {
    let mut cases = Vec::new();
    for text in [
        "0",
        "-0",
        "+0",
        "42",
        "-42",
        "+42",
        "00042",
        "2147483647",
        "-2147483648",
        "+2147483647",
        "-0002147483648",
    ] {
        let expected = text.parse::<i32>().unwrap();
        let expected = if expected == i32::MIN {
            "(-2147483647 - 1)".to_string()
        } else {
            expected.to_string()
        };
        cases.push(format!(
            "let text = \"{text}\"; match cli.parse_i32(text, record cli.Span {{ start = 0; end = byte_len(text) }}) {{ result.Ok(value) => value == {expected} | result.Err(error) => false }}"
        ));
    }
    for (text, start, end, expected) in [
        ("", 0, 0, "cli.ExpectedDigit(pos) => pos == 0"),
        ("-", 0, 1, "cli.ExpectedDigit(pos) => pos == 1"),
        ("+", 0, 1, "cli.ExpectedDigit(pos) => pos == 1"),
        ("abc", 0, 3, "cli.InvalidDigit(pos) => pos == 0"),
        ("12x", 0, 3, "cli.InvalidDigit(pos) => pos == 2"),
        (" 12", 0, 3, "cli.InvalidDigit(pos) => pos == 0"),
        ("12 ", 0, 3, "cli.InvalidDigit(pos) => pos == 2"),
        ("--1", 0, 3, "cli.InvalidDigit(pos) => pos == 1"),
        ("1.5", 0, 3, "cli.InvalidDigit(pos) => pos == 1"),
        ("2147483648", 0, 10, "cli.IntegerOverflow(pos) => pos == 9"),
        (
            "-2147483649",
            0,
            11,
            "cli.IntegerOverflow(pos) => pos == 10",
        ),
        (
            "+2147483648",
            0,
            11,
            "cli.IntegerOverflow(pos) => pos == 10",
        ),
        (
            "999999999999999",
            0,
            15,
            "cli.IntegerOverflow(pos) => pos == 9",
        ),
        (
            "--n=2147483648",
            4,
            14,
            "cli.IntegerOverflow(pos) => pos == 13",
        ),
        ("42", -1, 2, "cli.InvalidSpan => true"),
        ("42", 2, 1, "cli.InvalidSpan => true"),
        ("42", 0, 3, "cli.InvalidSpan => true"),
        ("42", i32::MAX, i32::MAX, "cli.InvalidSpan => true"),
    ] {
        cases.push(format!(
            "match cli.parse_i32(\"{text}\", record cli.Span {{ start = {start}; end = {end} }}) {{ result.Ok(value) => false | result.Err(error) => match error {{ {expected} | _ => false }} }}"
        ));
    }
    cases.push("match cli.parse_i32(\"--n=-42\", record cli.Span { start = 4; end = 7 }) { result.Ok(value) => value == -42 | result.Err(error) => false }".into());
    compare(&source_with_cases(&cases), "true");
}

#[test]
fn bundled_cli_example_runs_without_local_library_files() {
    compare(include_str!("../examples/cli/main.rill"), "42");
}
