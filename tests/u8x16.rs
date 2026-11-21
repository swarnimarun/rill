use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn buffers(a: &[u8; 16], b: &[u8; 16]) -> String {
    let mut src = String::from("let mut a = bytes_new(16) in ");
    for (i, byte) in a.iter().enumerate() {
        if *byte != 0 {
            src.push_str(&format!("let sa{i} = a <- bytes_set(a, {i}, {byte}) in "));
        }
    }
    src.push_str("let mut b = bytes_new(16) in ");
    for (i, byte) in b.iter().enumerate() {
        if *byte != 0 {
            src.push_str(&format!("let sb{i} = b <- bytes_set(b, {i}, {byte}) in "));
        }
    }
    src.push_str("let va = u8x16_load(a, 0) in let vb = u8x16_load(b, 0) in ");
    src
}
fn binary_program(op: &str, a: &[u8; 16], b: &[u8; 16], expected: &[u8; 16]) -> String {
    let checks = expected
        .iter()
        .enumerate()
        .map(|(i, byte)| format!("u8x16_lane(out, {i}) == {byte}"))
        .collect::<Vec<_>>()
        .join(" && ");
    format!(
        "let main = {} let out = {op}(va, vb) in {checks}",
        buffers(a, b)
    )
}
fn interpret(source: &str) -> Result<rill::interpreter::Value, String> {
    let p = rill::parser::parse(source)?;
    rill::check::check(&p)?;
    rill::interpreter::run(&p)
}
fn compare(source: &str) {
    let expected = interpret(source);
    let c = rill::compile(source).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "rill-u8x16-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("test.c");
    let binary = dir.join("test");
    fs::write(&cfile, c).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&cfile)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        match &expected {
            Ok(value) => {
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(output.stdout, format!("{value}\n").as_bytes());
            }
            Err(error) => {
                assert_eq!(error, "runtime trap");
                assert!(!output.status.success());
                assert_eq!(output.stderr, b"rill: runtime trap\n");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_byte_and_lane_matches_independent_reference() {
    for round in 0..16 {
        let a = std::array::from_fn(|i| (round * 16 + i) as u8);
        let b = std::array::from_fn(|i| 255 - ((round * 16 + (i * 7) % 16) as u8));
        for (name, expected) in [
            ("u8x16_and", std::array::from_fn(|i| a[i] & b[i])),
            ("u8x16_or", std::array::from_fn(|i| a[i] | b[i])),
            ("u8x16_xor", std::array::from_fn(|i| a[i] ^ b[i])),
            (
                "u8x16_eq",
                std::array::from_fn(|i| if a[i] == b[i] { 255 } else { 0 }),
            ),
        ] {
            let src = binary_program(name, &a, &b, &expected);
            assert_eq!(
                interpret(&src),
                Ok(rill::interpreter::Value::Bool(true)),
                "{name} round {round}"
            );
            if round == 4 {
                compare(&format!("let main = {} {name}(va, vb)", buffers(&a, &b)));
            }
        }
    }
}

#[test]
fn mask_reductions_and_call_arguments_match_c() {
    let a = [0u8; 16];
    let mut b = [0u8; 16];
    b[3] = 4;
    let src=format!("let check v = u8x16_any(v) && u8x16_all(u8x16_eq(v, v))\nlet main = {} let zeros = u8x16_any(va) in let same = u8x16_eq(va, vb) in zeros == false && check(vb) && u8x16_any(same) && u8x16_all(same) == false", buffers(&a,&b));
    assert_eq!(interpret(&src), Ok(rill::interpreter::Value::Bool(true)));
    compare(&src);
}

#[test]
fn bounds_are_checked_and_invalid_operations_trap() {
    for source in [
        "let main = u8x16_load(bytes_new(15), 0)",
        "let main = u8x16_load(bytes_new(16), -1)",
        "let main = u8x16_load(bytes_new(16), 1)",
        "let main = u8x16_lane(u8x16_load(bytes_new(16), 0), -1)",
        "let main = u8x16_lane(u8x16_load(bytes_new(16), 0), 16)",
    ] {
        assert_eq!(interpret(source).unwrap_err(), "runtime trap");
        compare(source);
    }
    let source = "let main = u8x16_lane(u8x16_load(bytes_set(bytes_new(17), 16, 37), 1), 15)";
    assert_eq!(interpret(source), Ok(rill::interpreter::Value::Int(37)));
    compare(source);
}
