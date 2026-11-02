use std::{fs, process::Command};

#[test]
fn every_writer_preserves_bytes_outside_its_checked_range() {
    let cases = [
        (
            "write_i32(inout output, -2147483647 - 1, offset)",
            "-2147483648",
        ),
        ("write_i32(inout output, 2147483647, offset)", "2147483647"),
        ("write_i32(inout output, 0, offset)", "0"),
        ("write_i32(inout output, -1, offset)", "-1"),
        ("write_i32(inout output, 10, offset)", "10"),
        ("write_i32(inout output, -100, offset)", "-100"),
        ("write_bool(inout output, true, offset)", "true"),
        ("write_bool(inout output, false, offset)", "false"),
        ("write_null(inout output, offset)", "null"),
        (
            r#"write_string(inout output, "\0\"\\", offset)"#,
            r#""\u0000\"\\""#,
        ),
        (r#"write_string(inout output, "é", offset)"#, "\"é\""),
    ];
    let mut source = String::from("import json_output;\n");
    for (index, (call, expected)) in cases.iter().enumerate() {
        let count = expected.len();
        let expected_literal = format!("{expected:?}");
        source.push_str(&format!(
            "let check{index} capacity offset =
                let mut output = system_bytes_zeroed_abort(capacity) in
                let mut pos = 0 in
                let fill = while pos < capacity do
                    let byte = owned_bytes_set(inout output, pos, 165) in pos <- pos + 1 in
                let written = json_output.{call} in
                let valid = offset >= 0 && offset <= capacity && {count} <= capacity - offset in
                let restart = pos <- 0 in let mut same = true in
                let scan = while pos < capacity do
                    let expected = if valid && pos >= offset && pos - offset < {count}
                        then byte_at({expected_literal}, pos - offset) else 165 in
                    let byte = same <- same && owned_bytes_get(output, pos) == expected in
                    pos <- pos + 1 in
                same && written == (if valid then {count} else -1)\n"
        ));
    }
    source.push_str("let main = let mut capacity = 0 in let mut good = true in\n");
    source.push_str("let sizes = while capacity <= 15 do\nlet mut offset = -1 in\nlet positions = while offset <= 17 do\n");
    for index in 0..cases.len() {
        source.push_str(&format!(
            "let check{index} = good <- good && check{index}(capacity, offset) in\n"
        ));
    }
    source.push_str("offset <- offset + 1 in\n");
    for index in 0..cases.len() {
        source.push_str(&format!(
            "let extremes{index} = good <- good && check{index}(capacity, -2147483647 - 1) && check{index}(capacity, 2147483647) in\n"
        ));
    }
    source.push_str("capacity <- capacity + 1 in good\n");
    let dir = std::env::temp_dir().join(format!("rill-json-adversarial-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/json_output.rill"),
        dir.join("json_output.rill"),
    )
    .unwrap();
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    let generated = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&generated, rill::backend::emit(&program).unwrap()).unwrap();
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&generated)
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
        assert_eq!(run.stdout, b"true\n");
    }
    fs::remove_dir_all(dir).unwrap();
}
