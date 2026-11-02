use rill::interpreter::Value;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn program(input: &str) -> String {
    let base = include_str!("../examples/json/string_serializer.rill");
    let (functions, _) = base.rsplit_once("\nlet main =").unwrap();
    let mut literal = String::from("\"");
    for ch in input.chars() {
        match ch {
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            '\0' => literal.push_str("\\0"),
            ch => literal.push(ch),
        }
    }
    literal.push('"');
    format!("{functions}\nlet main = json_write_string({literal})\n")
}

fn evaluate(input: &str) -> Value {
    let p = rill::parser::parse(&program(input)).unwrap();
    rill::check::check(&p).unwrap();
    rill::interpreter::run(&p).unwrap()
}

fn written(value: Value) -> Vec<u8> {
    let Value::Adt(name, mut fields) = value else {
        panic!("expected result")
    };
    assert_eq!(name, "JsonWritten");
    assert_eq!(fields.len(), 1);
    let Value::Bytes(bytes) = fields.remove(0) else {
        panic!("expected byte buffer")
    };
    assert_eq!(bytes.len(), 256);
    let len = bytes[0] as usize;
    assert!((2..=255).contains(&len));
    assert!(bytes[len + 1..].iter().all(|&byte| byte == 0));
    bytes[1..=len].to_vec()
}

// Independent decoder for the JSON string forms emitted by this example.
fn decode(encoded: &[u8]) -> Vec<u8> {
    assert_eq!(encoded.first(), Some(&b'"'));
    assert_eq!(encoded.last(), Some(&b'"'));
    let mut output = Vec::new();
    let mut i = 1;
    while i < encoded.len() - 1 {
        if encoded[i] != b'\\' {
            assert!(encoded[i] >= 32 && encoded[i] != b'"');
            output.push(encoded[i]);
            i += 1;
        } else {
            i += 1;
            match encoded[i] {
                b'"' | b'\\' => {
                    output.push(encoded[i]);
                    i += 1;
                }
                b'u' => {
                    assert_eq!(&encoded[i + 1..i + 3], b"00");
                    let hex = std::str::from_utf8(&encoded[i + 3..i + 5]).unwrap();
                    output.push(u8::from_str_radix(hex, 16).unwrap());
                    i += 5;
                }
                other => panic!("unexpected escape {other}"),
            }
        }
    }
    assert_eq!(i, encoded.len() - 1);
    output
}

#[test]
fn string_fixtures_round_trip_and_capacity_is_checked() {
    for (input, expected) in [
        ("", "\"\""),
        ("hello", "\"hello\""),
        ("a\"b\\c", "\"a\\\"b\\\\c\""),
        ("\0\n\r\t", "\"\\u0000\\u000a\\u000d\\u0009\""),
        ("\u{1f}", "\"\\u001f\""),
        ("é 😀", "\"é 😀\""),
    ] {
        let encoded = written(evaluate(input));
        assert_eq!(encoded, expected.as_bytes());
        assert_eq!(decode(&encoded), input.as_bytes());
    }
    let controls: String = (0..32).map(|c| char::from_u32(c).unwrap()).collect();
    assert_eq!(decode(&written(evaluate(&controls))), controls.as_bytes());
    for input in [
        "a".repeat(253),
        format!("{}a", "é".repeat(126)),
        format!("{}a", "\0".repeat(42)),
        format!("{}aaaaa", "😀".repeat(62)),
    ] {
        let encoded = written(evaluate(&input));
        assert_eq!(encoded.len(), 255);
        assert_eq!(decode(&encoded), input.as_bytes());
    }
    for input in [
        "a".repeat(254),
        format!("{}aa", "é".repeat(126)),
        format!("{}aa", "\0".repeat(42)),
        format!("{}aaaaaa", "😀".repeat(62)),
    ] {
        assert_eq!(evaluate(&input), Value::Adt("JsonTooLong".into(), vec![]));
    }
}

#[test]
fn strings_match_generated_c() {
    for input in ["a\"b\\c\n\0é".to_string(), "a".repeat(253), "a".repeat(254)] {
        let source = program(&input);
        let expected = evaluate(&input).to_string();
        let dir = std::env::temp_dir().join(format!(
            "rill-json-string-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let c_file = dir.join("string.c");
        let executable = dir.join("string");
        fs::write(&c_file, rill::compile(&source).unwrap()).unwrap();
        for opt in ["-O0", "-O2"] {
            let build = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&c_file)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, format!("{expected}\n").as_bytes());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
