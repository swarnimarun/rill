use std::{fs, process::Command};

#[test]
fn typed_json_results_match_in_interpreter_and_c() {
    let mut cases: Vec<(Vec<u8>, &str, i32)> = vec![
        (b"".to_vec(), "ExpectedDigit", 0),
        (b" \t\r\n".to_vec(), "ExpectedDigit", 4),
        (b"-".to_vec(), "ExpectedDigit", 1),
        (b" - ".to_vec(), "ExpectedDigit", 2),
        (b"+1".to_vec(), "ExpectedDigit", 0),
        (b"01".to_vec(), "LeadingZero", 1),
        (b"-01".to_vec(), "LeadingZero", 2),
        (b" 0000000000000000000000".to_vec(), "LeadingZero", 2),
        (b"2147483648".to_vec(), "IntegerOverflow", 9),
        (b"-2147483649".to_vec(), "IntegerOverflow", 10),
        (b"999999999999999999999999.0".to_vec(), "IntegerOverflow", 9),
        (b"1.0".to_vec(), "TrailingData", 1),
        (b"1e0".to_vec(), "TrailingData", 1),
        (b"1 2".to_vec(), "TrailingData", 2),
        (b"1\0".to_vec(), "TrailingData", 1),
        (b"1 \xff".to_vec(), "TrailingData", 2),
        (b"true".to_vec(), "ExpectedDigit", 0),
        (b"2147483647".to_vec(), "Ok", 2147483647),
        (b"-2147483648".to_vec(), "Ok", -2147483648),
        (b" \t\r\n-0 ".to_vec(), "Ok", 0),
        (b" 42\n".to_vec(), "Ok", 42),
        (b"-101".to_vec(), "Ok", -101),
    ];
    for byte in 0u8..=255 {
        let digit = byte.is_ascii_digit();
        let space = matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
        cases.push((
            vec![byte],
            if digit { "Ok" } else { "ExpectedDigit" },
            if digit {
                i32::from(byte - b'0')
            } else if space || byte == b'-' {
                1
            } else {
                0
            },
        ));
        cases.push((
            vec![b'1', byte],
            if digit || space { "Ok" } else { "TrailingData" },
            if digit {
                10 + i32::from(byte - b'0')
            } else {
                1
            },
        ));
        cases.push((
            vec![b'-', b'0', byte],
            if digit {
                "LeadingZero"
            } else if space {
                "Ok"
            } else {
                "TrailingData"
            },
            if space { 0 } else { 2 },
        ));
    }
    let mut source = String::from("import std.json_input as json; import std.result as result;\n");
    for (index, (bytes, variant, payload)) in cases.iter().enumerate() {
        source.push_str(&format!(
            "let case{index} (ignored: i32) = let mut input = system_bytes_zeroed_abort({}) in ",
            bytes.len()
        ));
        for (offset, byte) in bytes.iter().enumerate() {
            source.push_str(&format!(
                "let b{offset} = owned_bytes_set(inout input, {offset}, {byte}) in "
            ));
        }
        let expected = if *payload == i32::MIN {
            "(-2147483647 - 1)".to_string()
        } else {
            payload.to_string()
        };
        let success = if *variant == "Ok" {
            format!("value == {expected}")
        } else {
            "false".to_string()
        };
        let mut errors = Vec::new();
        for name in [
            "ExpectedDigit",
            "LeadingZero",
            "IntegerOverflow",
            "TrailingData",
        ] {
            let check = if *variant == name {
                format!("offset == {expected}")
            } else {
                "false".to_string()
            };
            errors.push(format!("json.{name}(offset) => {check}"));
        }
        source.push_str(&format!("let parsed = match json.parse_i32(input) {{ result.Ok(value) => {success} | result.Err(error) => match error {{ {} }} }} in ", errors.join(" | ")));
        source.push_str(&format!(
            "parsed && json.i32_valid(input) == {} && json.parse_i32_or(input, 73) == {}",
            *variant == "Ok",
            if *variant == "Ok" {
                expected
            } else {
                "73".to_string()
            }
        ));
        for (offset, byte) in bytes.iter().enumerate() {
            source.push_str(&format!(" && owned_bytes_get(input, {offset}) == {byte}"));
        }
        source.push('\n');
    }
    for (group, chunk) in (0..cases.len()).collect::<Vec<_>>().chunks(32).enumerate() {
        source.push_str(&format!("let group{group} (ignored: i32) = "));
        source.push_str(
            &chunk
                .iter()
                .map(|index| format!("case{index}(0)"))
                .collect::<Vec<_>>()
                .join(" && "),
        );
        source.push('\n');
    }
    source.push_str("let main = ");
    source.push_str(
        &(0..cases.len().div_ceil(32))
            .map(|group| format!("group{group}(0)"))
            .collect::<Vec<_>>()
            .join(" && "),
    );
    let dir = std::env::temp_dir().join(format!("rill-json-result-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    let c = dir.join("program.c");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    let binary = dir.join("program");
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
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
