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
            "rill-stdlib-binary-{}-{}",
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

fn compare(source: &str, allocations: usize) {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    let generated = fixture.0.join("program.c");
    fs::write(&generated, rill::backend::emit(&program).unwrap()).unwrap();
    let observer = fixture.0.join("observer.c");
    fs::write(
        &observer,
        format!(
            r#"
#include <stdio.h>
#include <stdlib.h>
static size_t allocations, releases;
void *tracked_malloc(size_t count) {{ ++allocations; return malloc(count); }}
void tracked_free(void *pointer) {{ ++releases; free(pointer); }}
int rill_program_main(void);
int main(void) {{
    int status = rill_program_main();
    if (allocations != {allocations} || releases != {allocations}) {{
        fprintf(stderr, "allocation/cleanup mismatch: %zu %zu\n", allocations, releases);
        return 41;
    }}
    return status;
}}
"#
        ),
    )
    .unwrap();
    for optimization in ["-O0", "-O3"] {
        let object = fixture.0.join("program.o");
        let executable = fixture.0.join("program");
        let build = Command::new("cc")
            .args([
                "-std=c11",
                optimization,
                "-Dmain=rill_program_main",
                "-Dmalloc=tracked_malloc",
                "-Dfree=tracked_free",
                "-c",
            ])
            .arg(&generated)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let link = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&object)
            .arg(&observer)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"true\n", "{optimization}");
        assert!(run.stderr.is_empty(), "{optimization}");
    }
}

fn integer(value: i32) -> String {
    if value == i32::MIN {
        "(-2147483647 - 1)".into()
    } else {
        value.to_string()
    }
}

#[test]
fn endian_writers_match_independent_rust_bytes_for_signed_boundaries() {
    let values = [
        i32::MIN,
        i32::MIN + 1,
        -16_777_217,
        -16_777_216,
        -65_537,
        -65_536,
        -257,
        -256,
        -255,
        -129,
        -128,
        -1,
        0,
        1,
        127,
        128,
        255,
        256,
        257,
        65_535,
        65_536,
        16_777_215,
        16_777_216,
        i32::MAX - 1,
        i32::MAX,
    ];
    let mut source = String::from(
        r#"
import std.binary as binary;
import std.bytes as bytes;
import std.result as result;
fn check(value: i32, b0: i32, b1: i32, b2: i32, b3: i32) -> bool {
    let mut output = system_bytes_zeroed_abort(10);
    let initialized = bytes.fill(inout output, 0, 10, 165);
    let little = binary.write_i32_le(inout output, 1, value);
    let big = binary.write_i32_be(inout output, 5, value);
    (match initialized { result.Ok(n) => n == 10 | _ => false }) &&
    (match little { result.Ok(n) => n == 4 | _ => false }) &&
    (match big { result.Ok(n) => n == 4 | _ => false }) &&
    (match binary.read_i32_le(output, 1) { result.Ok(n) => n == value | _ => false }) &&
    (match binary.read_i32_be(output, 5) { result.Ok(n) => n == value | _ => false }) &&
    owned_bytes_get(output, 0) == 165 && owned_bytes_get(output, 9) == 165 &&
    owned_bytes_get(output, 1) == b0 && owned_bytes_get(output, 2) == b1 &&
    owned_bytes_get(output, 3) == b2 && owned_bytes_get(output, 4) == b3 &&
    owned_bytes_get(output, 5) == b3 && owned_bytes_get(output, 6) == b2 &&
    owned_bytes_get(output, 7) == b1 && owned_bytes_get(output, 8) == b0
}
fn main() {
    let mut good = true;
"#,
    );
    for value in values {
        let bytes = value.to_le_bytes();
        source.push_str(&format!(
            "good <- check({}, {}, {}, {}, {}) && good;\n",
            integer(value),
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3]
        ));
    }
    source.push_str("good\n}\n");
    compare(&source, values.len());
}

#[test]
fn endian_readers_decode_every_sign_byte_from_independent_input() {
    let mut source = String::from("import std.binary as binary; import std.result as result;\n");
    for (name, little) in [("expected_le", true), ("expected_be", false)] {
        source.push_str(&format!("fn {name}(high: i32) -> i32 {{ match high {{\n"));
        for high in 0..=255_u8 {
            let bytes = [7, 128, 255, high];
            let value = if little {
                i32::from_le_bytes(bytes)
            } else {
                i32::from_be_bytes(bytes)
            };
            source.push_str(&format!("{high} => {} |\n", integer(value)));
        }
        source.push_str("_ => 0 } }\n");
    }
    source.push_str(
        r#"
fn check(high: i32, little: i32, big: i32) -> bool {
    let mut input = system_bytes_zeroed_abort(6);
    owned_bytes_set(inout input, 0, 165);
    owned_bytes_set(inout input, 1, 7);
    owned_bytes_set(inout input, 2, 128);
    owned_bytes_set(inout input, 3, 255);
    owned_bytes_set(inout input, 4, high);
    owned_bytes_set(inout input, 5, 165);
    (match binary.read_i32_le(input, 1) { result.Ok(n) => n == little | _ => false }) &&
    (match binary.read_i32_be(input, 1) { result.Ok(n) => n == big | _ => false }) &&
    owned_bytes_get(input, 0) == 165 && owned_bytes_get(input, 1) == 7 &&
    owned_bytes_get(input, 2) == 128 && owned_bytes_get(input, 3) == 255 &&
    owned_bytes_get(input, 4) == high && owned_bytes_get(input, 5) == 165
}
fn main() {
    let mut high = 0;
    let mut good = true;
    while high <= 255 {
        good <- check(high, expected_le(high), expected_be(high)) && good;
        high <- high + 1;
    };
    good
}
"#,
    );
    compare(&source, 256);
}

#[test]
fn complete_range_preflight_preserves_invalid_buffers_and_unaligned_reads() {
    compare(
        r#"
import std.binary as binary;
import std.bytes as bytes;
import std.result as result;
fn check(capacity: i32, offset: i32, accepted: bool) -> bool {
    let mut buffer = system_bytes_zeroed_abort(capacity);
    let initialized = bytes.fill(inout buffer, 0, capacity, 165);
    let little = binary.write_i32_le(inout buffer, offset, 305419896);
    let big = binary.write_i32_be(inout buffer, offset, 305419896);
    let read_little = binary.read_i32_le(buffer, offset);
    let read_big = binary.read_i32_be(buffer, offset);
    let status =
        (match initialized { result.Ok(n) => n == capacity | _ => false }) &&
        (match little { result.Ok(n) => accepted && n == 4 | result.Err(bytes.InvalidRange) => !accepted | _ => false }) &&
        (match big { result.Ok(n) => accepted && n == 4 | result.Err(bytes.InvalidRange) => !accepted | _ => false }) &&
        (match read_little { result.Ok(n) => accepted && n == 2018915346 | result.Err(bytes.InvalidRange) => !accepted | _ => false }) &&
        (match read_big { result.Ok(n) => accepted && n == 305419896 | result.Err(bytes.InvalidRange) => !accepted | _ => false });
    let mut pos = 0;
    let mut same = true;
    while pos < capacity {
        let expected = if accepted && pos >= offset && pos - offset < 4 {
            match pos - offset { 0 => 18 | 1 => 52 | 2 => 86 | _ => 120 }
        } else { 165 };
        same <- same && owned_bytes_get(buffer, pos) == expected;
        pos <- pos + 1;
    };
    status && same
}
fn main() {
    let mut capacity = 0;
    let mut good = true;
    while capacity <= 8 {
        let mut offset = -1;
        while offset <= 10 {
            good <- check(capacity, offset, offset >= 0 && offset + 4 <= capacity) && good;
            offset <- offset + 1;
        };
        good <- check(capacity, -2147483647 - 1, false) && good;
        good <- check(capacity, 2147483647, false) && good;
        capacity <- capacity + 1;
    };
    good
}
"#,
        126,
    );
}
