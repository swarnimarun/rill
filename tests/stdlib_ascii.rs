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
        let path = std::env::temp_dir().join(format!(
            "rill-stdlib-ascii-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn literal(value: i32) -> String {
    if value == i32::MIN {
        "(-2147483647 - 1)".into()
    } else {
        value.to_string()
    }
}

fn optional(operation: &str, value: Option<i32>) -> String {
    match value {
        Some(value) => format!(
            "(match {operation} {{ option.Some(value) => value == {value} | option.None => false }})"
        ),
        None => format!(
            "(match {operation} {{ option.Some(value) => false | option.None => true }})"
        ),
    }
}

fn compare(cases: &[String]) {
    let mut source = "import std.ascii as ascii; import std.option as option;\n".to_string();
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
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, &source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    let c = rill::backend::emit(&program).unwrap();
    assert!(!c.contains("malloc("));
    fs::write(fixture.0.join("program.c"), c).unwrap();
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
        assert_eq!(output.stdout, b"true\n", "{optimization}");
        assert!(output.stderr.is_empty(), "{optimization}");
    }
}

#[test]
fn every_byte_and_extreme_integer_matches_independent_ascii_expectations() {
    let mut codes = (0..=255).collect::<Vec<i32>>();
    codes.extend([i32::MIN, -256, -128, -1, 256, 257, 1024, i32::MAX]);
    let mut cases = Vec::new();
    for code in codes {
        let byte = u8::try_from(code).ok();
        let expected = [
            ("is_ascii", byte.is_some_and(|b| b.is_ascii())),
            ("is_control", byte.is_some_and(|b| b.is_ascii_control())),
            ("is_digit", byte.is_some_and(|b| b.is_ascii_digit())),
            ("is_lower", byte.is_some_and(|b| b.is_ascii_lowercase())),
            ("is_upper", byte.is_some_and(|b| b.is_ascii_uppercase())),
            ("is_alpha", byte.is_some_and(|b| b.is_ascii_alphabetic())),
            (
                "is_alphanumeric",
                byte.is_some_and(|b| b.is_ascii_alphanumeric()),
            ),
            // The Rill contract includes all six C/ASCII whitespace bytes,
            // including vertical tab, unlike Rust's five-byte ASCII helper.
            ("is_whitespace", matches!(code, 9..=13 | 32)),
            ("is_blank", matches!(code, 9 | 32)),
            ("is_graphic", byte.is_some_and(|b| b.is_ascii_graphic())),
            ("is_printable", matches!(code, 32..=126)),
            (
                "is_punctuation",
                byte.is_some_and(|b| b.is_ascii_punctuation()),
            ),
            ("is_hex_digit", byte.is_some_and(|b| b.is_ascii_hexdigit())),
        ];
        let value = literal(code);
        let mut checks = expected
            .iter()
            .map(|(name, expected)| format!("ascii.{name}({value}) == {expected}"))
            .collect::<Vec<_>>();
        let lower = byte.map_or(code, |b| i32::from(b.to_ascii_lowercase()));
        let upper = byte.map_or(code, |b| i32::from(b.to_ascii_uppercase()));
        checks.push(format!("ascii.to_lower({value}) == {}", literal(lower)));
        checks.push(format!("ascii.to_upper({value}) == {}", literal(upper)));
        let decimal = byte.filter(u8::is_ascii_digit).map(|b| i32::from(b - b'0'));
        let hex = byte.and_then(|b| char::from(b).to_digit(16).map(|d| d as i32));
        checks.push(optional(&format!("ascii.digit_value({value})"), decimal));
        checks.push(optional(&format!("ascii.hex_value({value})"), hex));
        cases.push(checks.join(" && "));
    }
    compare(&cases);
}
