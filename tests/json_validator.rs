use rill::interpreter::Value;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn source(input: &str) -> String {
    let base = include_str!("../examples/json/validator.rill");
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
            other => literal.push(other),
        }
    }
    literal.push('"');
    format!("{functions}\nlet main = json_valid({literal})\n")
}

fn interpret(input: &str) -> bool {
    let p = rill::parser::parse(&source(input)).unwrap();
    rill::check::check(&p).unwrap();
    match rill::interpreter::run(&p).unwrap() {
        Value::Bool(result) => result,
        value => panic!("unexpected result: {value}"),
    }
}

#[test]
fn accepts_and_rejects_json_syntax() {
    let valid = [
        "null",
        " true \n",
        "-0",
        "123.45e-9",
        "[0,-0,-123,1.0,1e-2,1E+2]",
        "[]",
        "{}",
        r#""\uD800""#,
        "[true,null,1]",
        r#"{"nested":[{},[false]],"s":"é\\u0041"}"#,
        r#""escape: \" \\ \/ \b \f \n \r \t \u0041""#,
    ];
    let invalid = [
        "",
        "nul",
        "true false",
        "01",
        "-01",
        "-",
        "1.",
        "1.e2",
        "1e",
        "1e+",
        "[1,]",
        "[,1]",
        "[truefalse]",
        "{\"a\":}",
        "{\"a\":1,}",
        "{1:2}",
        "\"raw\nline\"",
        "\"a\0b\"",
        r#""bad\q""#,
        r#""bad\u12xz""#,
        r#"["\u000"]"#,
        "[",
        "{",
    ];
    for input in valid {
        assert!(interpret(input), "should accept {input:?}");
    }
    for input in invalid {
        assert!(!interpret(input), "should reject {input:?}");
    }
    assert!(interpret(&format!("{}0{}", "[".repeat(8), "]".repeat(8))));
    assert!(!interpret(&format!("{}0{}", "[".repeat(9), "]".repeat(9))));
}

#[test]
fn representative_cases_match_generated_c() {
    for (input, expected) in [
        (r#"{"a":[1,true,"é"]}"#, true),
        ("[1,]", false),
        ("-1.2e+3", true),
        ("1e+", false),
    ] {
        let program = source(input);
        assert_eq!(interpret(input), expected);
        let c = rill::compile(&program).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "rill-json-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let c_file = dir.join("validator.c");
        let executable = dir.join("validator");
        fs::write(&c_file, c).unwrap();
        for opt in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&c_file)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                result.stdout,
                format!("{expected}\n").as_bytes(),
                "{input:?} {opt}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
