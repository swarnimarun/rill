use std::{
    cmp::Ordering,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-stdlib-text-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn string_literal(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\0' => result.push_str("\\0"),
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

fn integer_literal(value: i32) -> String {
    if value == i32::MIN {
        "(-2147483647 - 1)".into()
    } else {
        value.to_string()
    }
}

fn optional(operation: &str, expected: Option<usize>) -> String {
    match expected {
        Some(index) => format!(
            "(match {operation} {{ option.Some(index) => index == {index} | option.None => false }})"
        ),
        None => format!(
            "(match {operation} {{ option.Some(index) => false | option.None => true }})"
        ),
    }
}

fn source_with_cases(cases: &[String]) -> String {
    let mut source = "import std.text as text; import std.option as option;\n".to_string();
    for (index, case) in cases.iter().enumerate() {
        source.push_str(&format!("fn case{index}() {{ {case} }}\n"));
    }
    for (group, chunk) in cases.chunks(24).enumerate() {
        let calls = (0..chunk.len())
            .map(|offset| format!("case{}()", group * 24 + offset))
            .collect::<Vec<_>>()
            .join(" && ");
        source.push_str(&format!("fn group{group}() {{ {calls} }}\n"));
    }
    let calls = (0..cases.len().div_ceil(24))
        .map(|index| format!("group{index}()"))
        .collect::<Vec<_>>()
        .join(" && ");
    source.push_str(&format!("fn main() {{ {calls} }}\n"));
    source
}

fn compare(source: &str, expected: &[u8]) {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        format!("{}\n", rill::interpreter::run(&program).unwrap()).as_bytes(),
        expected
    );
    fs::write(
        fixture.0.join("program.c"),
        rill::backend::emit(&program).unwrap(),
    )
    .unwrap();
    let executable = fixture.0.join("program");
    for optimization in ["-O0", "-O3"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(fixture.0.join("program.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success(), "{optimization}");
        assert_eq!(output.stdout, expected, "{optimization}");
        assert!(output.stderr.is_empty(), "{optimization}");
    }
}

#[test]
fn string_pairs_match_independent_byte_search_and_comparison() {
    let corpus = [
        "", "a", "b", "aa", "aba", "ababa", "abcdef", "def", "é", "éé", "aé😀b", "😀", "\0",
        "a\0b", "a\0", "\\\"",
    ];
    let mut cases = Vec::new();
    for left in corpus {
        for right in corpus {
            let a = left.as_bytes();
            let b = right.as_bytes();
            let first = if b.is_empty() {
                Some(0)
            } else {
                a.windows(b.len()).position(|window| window == b)
            };
            let last = if b.is_empty() {
                Some(a.len())
            } else {
                a.windows(b.len()).rposition(|window| window == b)
            };
            let order = match a.cmp(b) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            };
            let left = string_literal(left);
            let right = string_literal(right);
            cases.push(format!(
                "text.equal({left}, {right}) == {} && text.compare({left}, {right}) == {order} && text.starts_with({left}, {right}) == {} && text.ends_with({left}, {right}) == {} && text.contains({left}, {right}) == {} && {} && {}",
                a == b,
                a.starts_with(b),
                a.ends_with(b),
                first.is_some(),
                optional(&format!("text.find({left}, {right})"), first),
                optional(&format!("text.rfind({left}, {right})"), last),
            ));
        }
    }
    compare(&source_with_cases(&cases), b"true\n");
}

#[test]
fn trimming_and_spans_preserve_byte_boundaries_and_reject_invalid_ranges() {
    let mut cases = Vec::new();
    for value in [
        "",
        " ",
        "\t\n\u{b}\u{c}\r ",
        "a",
        "  a\t",
        "\u{b}é\u{c}",
        "\t😀 é\r",
        "\0a\0",
        " \0 ",
        "\u{a0}é\u{a0}",
        "\u{2003}a\u{2003}",
    ] {
        let whitespace = |byte: &u8| matches!(byte, 9..=13 | 32);
        let bytes = value.as_bytes();
        let start = bytes.iter().take_while(|byte| whitespace(byte)).count();
        let end = bytes[start..]
            .iter()
            .rposition(|byte| !whitespace(byte))
            .map_or(start, |index| start + index + 1);
        let span_text = string_literal(&value[start..end]);
        let value = string_literal(value);
        cases.push(format!(
            "let span = text.trim_ascii({value}); span.start == {start} && span.end == {end} && text.valid_span({value}, span) && text.span_equal({value}, span, {span_text})"
        ));
    }
    let value = "aé\0b";
    for (start, end, expected) in [
        (0, 5, value),
        (1, 3, "é"),
        (3, 4, "\0"),
        (5, 5, ""),
        (0, 0, ""),
        (1, 2, "é"),
        (2, 3, "é"),
        (0, 5, "different"),
        (-1, 3, ""),
        (3, 1, ""),
        (0, 6, ""),
        (i32::MIN, i32::MAX, ""),
        (i32::MAX, i32::MAX, ""),
        (i32::MIN, i32::MIN, ""),
    ] {
        let valid = start >= 0 && end >= start && end as usize <= value.len();
        let equal = valid && &value.as_bytes()[start as usize..end as usize] == expected.as_bytes();
        cases.push(format!(
            "let span = record text.Span {{ start = {}; end = {} }}; text.valid_span({}, span) == {valid} && text.span_equal({}, span, {}) == {equal}",
            integer_literal(start),
            integer_literal(end),
            string_literal(value),
            string_literal(value),
            string_literal(expected),
        ));
    }
    compare(&source_with_cases(&cases), b"true\n");
}

#[test]
fn monomorphic_text_and_ascii_utilities_coexist_with_owners() {
    compare(
        "import std.text as text; import std.ascii as ascii; import std.option as option;\n\
         fn main() { let owner = system_box_i32_abort(7); \
             let good = text.contains(\"aéb\", \"é\") && \
                 (match ascii.hex_value(70) { option.Some(value) => value == 15 | option.None => false }); \
             if good { box_read(owner) + 35 } else { 0 } }",
        b"42\n",
    );
}

#[test]
fn matching_helper_stays_private() {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(
        &entry,
        "import std.text as text; fn main() { text.matches_at(\"a\", \"a\", 0) }",
    )
    .unwrap();
    let error = rill::modules::compile::load_program(&entry).unwrap_err();
    assert_eq!(error.code, "E-MODULE-PRIVATE");
}
