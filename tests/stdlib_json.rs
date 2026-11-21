use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str) {
    let dir = std::env::temp_dir().join(format!(
        "rill-stdlib-json-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    for module in ["json_output", "json_input"] {
        fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("stdlib/{module}.rill")),
            dir.join(format!("{module}.rill")),
        )
        .unwrap();
    }
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
fn signed_integer_round_trips_runtime_owned_bytes() {
    for value in [
        i32::MIN,
        -1000000000,
        -101,
        -10,
        -1,
        0,
        1,
        10,
        101,
        1000000000,
        i32::MAX,
    ] {
        let text = value.to_string();
        let expression = if value == i32::MIN {
            "(-2147483647 - 1)".to_string()
        } else {
            text.clone()
        };
        compare(&format!(
            "import json_output; import json_input;
            let main = let mut output = system_bytes_zeroed_abort({}) in
            let written = json_output.write_i32(inout output, {expression}, 0) in
            let mut i = 0 in let mut same = true in
            let verify = while i < owned_bytes_len(output) do
                let check = same <- same && owned_bytes_get(output, i) == byte_at(\"{text}\", i) in
                i <- i + 1 in
            same && written == {} && json_input.i32_valid(output) &&
            json_input.parse_i32_or(output, 73) == {expression}",
            text.len(),
            text.len()
        ));
    }
}

#[test]
fn runtime_input_rejects_syntax_overflow_and_arbitrary_bytes() {
    for (bytes, expected) in [
        (b" \t\r\n-0 ".as_slice(), Some(0)),
        (b" 42\n", Some(42)),
        (b"", None),
        (b"-", None),
        (b"+1", None),
        (b"01", None),
        (b"-01", None),
        (b"1.0", None),
        (b"1e0", None),
        (b"1 2", None),
        (b"2147483648", None),
        (b"-2147483649", None),
        (b"999999999999999999999999999999999", None),
        (b"1\0", None),
        (b"\xff", None),
        (b"true", None),
    ] {
        let mut writes = String::new();
        for (i, byte) in bytes.iter().enumerate() {
            writes.push_str(&format!(
                "let byte{i} = owned_bytes_set(inout input, {i}, {byte}) in "
            ));
        }
        compare(&format!(
            "import json_input; let main =
            let mut input = system_bytes_zeroed_abort({}) in {writes}
            json_input.i32_valid(input) == {} && json_input.parse_i32_or(input, 73) == {}",
            bytes.len(),
            expected.is_some(),
            expected.unwrap_or(73)
        ));
    }
}

#[test]
fn writers_are_atomic_and_compose() {
    compare("import json_output; let main =
        let mut output = system_bytes_zeroed_abort(12) in
        let a = json_output.write_bool(inout output, false, 0) in
        let b = json_output.write_null(inout output, a) in
        let c = json_output.write_i32(inout output, -42, a + b) in
        let failed = json_output.write_i32(inout output, 100, 10) in
        let bad = json_output.write_bool(inout output, true, -1) in
        let mut pos = 0 in let mut good = true in
        let scan = while pos < 12 do
            let check = good <- good && owned_bytes_get(output, pos) == byte_at(\"falsenull-42\", pos) in
            pos <- pos + 1 in
        good && a == 5 && b == 4 && c == 3 && failed == -1 && bad == -1");
}

#[test]
fn rejected_writes_preserve_poison_at_every_offset() {
    for (capacity, offset) in [(0, 0), (10, 0), (12, 2), (12, -1), (12, 13), (12, i32::MAX)] {
        compare(&format!(
            "import json_output; let main =
            let mut output = system_bytes_zeroed_abort({capacity}) in
            let mut i = 0 in
            let fill = while i < owned_bytes_len(output) do
                let byte = owned_bytes_set(inout output, i, 165) in i <- i + 1 in
            let failed = json_output.write_i32(inout output, -2147483647 - 1, {offset}) in
            let reset = i <- 0 in let mut same = true in
            let scan = while i < owned_bytes_len(output) do
                let check = same <- same && owned_bytes_get(output, i) == 165 in i <- i + 1 in
            failed == -1 && same"
        ));
    }
}

#[test]
fn every_runtime_byte_class_matches_json_integer_grammar() {
    compare(
        "import json_input;
        let check byte =
            let digit = byte >= 48 && byte <= 57 in
            let space = byte == 32 || byte == 9 || byte == 10 || byte == 13 in
            let mut single = system_bytes_zeroed_abort(1) in
            let s = owned_bytes_set(inout single, 0, byte) in
            let single_ok = json_input.i32_valid(single) == digit &&
                json_input.parse_i32_or(single, 73) == (if digit then byte - 48 else 73) &&
                owned_bytes_get(single, 0) == byte in
            let mut positive = system_bytes_zeroed_abort(2) in
            let p0 = owned_bytes_set(inout positive, 0, 49) in
            let p1 = owned_bytes_set(inout positive, 1, byte) in
            let positive_ok = json_input.i32_valid(positive) == (digit || space) &&
                json_input.parse_i32_or(positive, 73) ==
                    (if digit then 10 + byte - 48 else if space then 1 else 73) &&
                owned_bytes_get(positive, 0) == 49 && owned_bytes_get(positive, 1) == byte in
            let mut zero = system_bytes_zeroed_abort(3) in
            let z0 = owned_bytes_set(inout zero, 0, 45) in
            let z1 = owned_bytes_set(inout zero, 1, 48) in
            let z2 = owned_bytes_set(inout zero, 2, byte) in
            let zero_ok = json_input.i32_valid(zero) == space &&
                json_input.parse_i32_or(zero, 73) == (if space then 0 else 73) &&
                owned_bytes_get(zero, 0) == 45 && owned_bytes_get(zero, 1) == 48 &&
                owned_bytes_get(zero, 2) == byte in
            single_ok && positive_ok && zero_ok
        let main = let mut byte = 0 in let mut good = true in
            let scan = while byte < 256 do
                let checked = good <- good && check(byte) in byte <- byte + 1 in good",
    );
}
