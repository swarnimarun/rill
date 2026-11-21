use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn literal(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fixture(input: &str, expected: &str, capacity: i32, offset: i32, fits: bool) -> String {
    let written = if fits { expected.len() as i32 } else { -1 };
    // Poison the whole destination. On failure every byte must retain the poison;
    // on success only the exact destination range may change.
    format!(
        "import json_output;\nlet main =
        let mut output = system_bytes_zeroed_abort({capacity}) in
        let mut i = 0 in
        let fill = while i < owned_bytes_len(output) do
            let done = owned_bytes_set(inout output, i, 165) in i <- i + 1 in
        let written = json_output.write_string(inout output, {}, {offset}) in
        let reset = i <- 0 in
        let mut good = written == {written} in
        let verify = while i < owned_bytes_len(output) do
            let expected = if written >= 0 && i >= {offset} && i - {offset} < written
                then byte_at({}, i - {offset}) else 165 in
            let compare = good <- good && owned_bytes_get(output, i) == expected in
            i <- i + 1 in good",
        literal(input),
        literal(expected),
    )
}

fn compare(source: &str) {
    let dir = std::env::temp_dir().join(format!(
        "rill-owned-json-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join("json_output.rill"),
        fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/json_output.rill"),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let p = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(rill::interpreter::run(&p).unwrap().to_string(), "true");
    let c = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&c)
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
        assert_eq!(result.stdout, b"true\n");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn escaped_strings_preserve_destination_prefix_and_suffix() {
    for (input, expected) in [
        ("", "\"\""),
        ("hello", "\"hello\""),
        ("a\"b\\c", "\"a\\\"b\\\\c\""),
        ("\0\n\r\t", "\"\\u0000\\u000a\\u000d\\u0009\""),
        ("é 😀", "\"é 😀\""),
    ] {
        compare(&fixture(
            input,
            expected,
            expected.len() as i32 + 7,
            3,
            true,
        ));
    }
    let input: String = (0u8..32).map(char::from).collect();
    let expected = format!(
        "\"{}\"",
        (0u8..32)
            .map(|b| format!("\\u00{b:02x}"))
            .collect::<String>()
    );
    compare(&fixture(&input, &expected, expected.len() as i32, 0, true));
    let input = "x".repeat(300);
    let expected = format!("\"{input}\"");
    compare(&fixture(&input, &expected, 302, 0, true));
}

#[test]
fn invalid_offsets_and_exhaustion_leave_every_byte_unchanged() {
    for (input, expected, capacity, offset) in [
        ("", "\"\"", 0, 0),
        ("", "\"\"", 1, 0),
        ("", "\"\"", 2, 1),
        ("", "\"\"", 2, 2),
        ("x", "\"x\"", 8, -1),
        ("x", "\"x\"", 8, 9),
        ("x", "\"x\"", 8, i32::MAX),
        ("x", "\"x\"", 8, i32::MIN),
        ("ab\0", "\"ab\\u0000\"", 9, 0),
        ("é", "\"é\"", 3, 0),
    ] {
        compare(&fixture(input, expected, capacity, offset, false));
    }
}

#[test]
fn repeated_writes_compose_and_cli_example_runs() {
    compare(
        "import json_output;
        let main = let mut output = system_bytes_zeroed_abort(7) in
            let first = json_output.write_string(inout output, \"a\", 0) in
            let second = json_output.write_string(inout output, \"bc\", first) in
            let failed = json_output.write_string(inout output, \"\", first + second) in
            first == 3 && second == 4 && failed == -1 &&
            owned_bytes_get(output, 0) == 34 && owned_bytes_get(output, 1) == 97 &&
            owned_bytes_get(output, 3) == 34 && owned_bytes_get(output, 4) == 98 &&
            owned_bytes_get(output, 5) == 99 && owned_bytes_get(output, 6) == 34",
    );
    let run = Command::new(env!("CARGO_BIN_EXE_rill"))
        .args(["run", "examples/owned/json.rill"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"15\n");
}
