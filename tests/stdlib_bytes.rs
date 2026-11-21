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
            "rill-stdlib-bytes-{}-{}",
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

#[test]
fn copy_within_matches_rust_memmove_for_every_small_range_and_extremes() {
    let mut expected = String::new();
    let mut valid = String::new();
    let mut cases = Vec::new();
    for source in 0_i32..=8 {
        for destination in 0_i32..=8 {
            for count in 0_i32..=8 {
                cases.push((source, destination, count));
            }
        }
    }
    for value in [-1, i32::MIN, i32::MAX] {
        cases.extend([(value, 0, 1), (0, value, 1), (0, 0, value)]);
    }
    for &(source, destination, count) in &cases {
        let mut bytes = *b"ABCDEFGH";
        let range = usize::try_from(source)
            .ok()
            .zip(usize::try_from(destination).ok())
            .zip(usize::try_from(count).ok())
            .and_then(|((source, destination), count)| {
                source
                    .checked_add(count)
                    .zip(destination.checked_add(count))
                    .filter(|&(source_end, destination_end)| {
                        source_end <= 8 && destination_end <= 8
                    })
                    .map(|(source_end, _)| (source..source_end, destination))
            });
        if let Some((range, destination)) = range {
            bytes.copy_within(range, destination);
            valid.push('1');
        } else {
            valid.push('0');
        }
        expected.push_str(std::str::from_utf8(&bytes).unwrap());
    }
    let mut source = format!(
        r#"
import std.bytes as bytes;
import std.result as result;
fn check(source: i32, destination: i32, count: i32, accepted: bool, expected: str, at: i32) -> bool {{
    let mut buffer = system_bytes_zeroed_abort(8);
    let mut pos = 0;
    while pos < 8 {{
        owned_bytes_set(inout buffer, pos, 65 + pos);
        pos <- pos + 1;
    }};
    let outcome = bytes.copy_within(inout buffer, source, destination, count);
    let status = match outcome {{ result.Ok(n) => accepted && n == count | result.Err(bytes.InvalidRange) => !accepted | result.Err(bytes.InvalidByte) => false }};
    pos <- 0;
    let mut same = true;
    while pos < 8 {{
        same <- same && owned_bytes_get(buffer, pos) == byte_at(expected, at + pos);
        pos <- pos + 1;
    }};
    status && same
}}
fn main() {{
    let expected = "{expected}";
    let valid = "{valid}";
    let mut source = 0;
    let mut index = 0;
    let mut good = true;
    while source <= 8 {{
        let mut destination = 0;
        while destination <= 8 {{
            let mut count = 0;
            while count <= 8 {{
                good <- check(source, destination, count, byte_at(valid, index) == 49, expected, index * 8) && good;
                index <- index + 1;
                count <- count + 1;
            }};
            destination <- destination + 1;
        }};
        source <- source + 1;
    }};
"#
    );
    for (index, &(from, to, count)) in cases.iter().enumerate().skip(729) {
        let int = |value: i32| {
            if value == i32::MIN {
                "(-2147483647 - 1)".to_string()
            } else {
                value.to_string()
            }
        };
        source.push_str(&format!(
            "good <- check({}, {}, {}, false, expected, {}) && good;\n",
            int(from),
            int(to),
            int(count),
            index * 8
        ));
    }
    source.push_str("good\n}\n");
    compare(&source, cases.len());
}

#[test]
fn checked_writes_preserve_poison_and_copy_between_distinct_roots() {
    compare(
        r#"
import std.bytes as bytes;
import std.result as result;
import std.option as option;
fn main() {
    let mut destination = system_bytes_zeroed_abort(12);
    let mut source = system_bytes_zeroed_abort(4);
    let initial = bytes.fill(inout destination, 0, 12, 165);
    let copied_text = bytes.write_str(inout source, 0, "A\0é");
    let copied = bytes.copy(inout destination, 3, source, 0, 4);
    let first = bytes.find(destination, 0);
    let absent = bytes.find(destination, 255);
    let invalid = bytes.find(destination, 256);
    let bad_range = bytes.fill(inout destination, 2147483647, 1, -1);
    let bad_value = bytes.fill(inout destination, 0, 0, -1);
    let bad_copy = bytes.copy(inout destination, 0, source, 0, 5);
    let bad_text = bytes.write_str(inout destination, 12, "x");
    let empty_text = bytes.write_str(inout destination, 12, "");
    let empty_copy = bytes.copy(inout destination, 12, source, 4, 0);
    let mut index = 0;
    let mut same = true;
    while index < 12 {
        let expected = if index >= 3 && index < 7 { byte_at("A\0é", index - 3) } else { 165 };
        same <- same && owned_bytes_get(destination, index) == expected;
        index <- index + 1;
    };
    same && bytes.equal(source, source) && !bytes.equal(source, destination) &&
    (match initial { result.Ok(n) => n == 12 | _ => false }) &&
    (match copied_text { result.Ok(n) => n == 4 | _ => false }) &&
    (match copied { result.Ok(n) => n == 4 | _ => false }) &&
    (match first { result.Ok(option.Some(n)) => n == 4 | _ => false }) &&
    (match absent { result.Ok(option.None) => true | _ => false }) &&
    (match invalid { result.Err(bytes.InvalidByte) => true | _ => false }) &&
    (match bad_range { result.Err(bytes.InvalidRange) => true | _ => false }) &&
    (match bad_value { result.Err(bytes.InvalidByte) => true | _ => false }) &&
    (match bad_copy { result.Err(bytes.InvalidRange) => true | _ => false }) &&
    (match bad_text { result.Err(bytes.InvalidRange) => true | _ => false }) &&
    (match empty_text { result.Ok(n) => n == 0 | _ => false }) &&
    (match empty_copy { result.Ok(n) => n == 0 | _ => false }) &&
    bytes.valid_range(destination, 12, 0) && !bytes.valid_range(destination, 0, -1) &&
    !bytes.valid_range(destination, -2147483647 - 1, 2147483647)
}
"#,
        2,
    );
}

#[test]
fn distinct_buffer_copy_keeps_source_loan_exclusion() {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, "import std.bytes as bytes; fn main() { let mut buffer = system_bytes_zeroed_abort(4); bytes.copy(inout buffer, 0, buffer, 0, 4) }").unwrap();
    let error = rill::modules::compile::load_program(&entry).unwrap_err();
    assert!(
        error.to_string().contains("loan") || error.to_string().contains("borrow"),
        "{error}"
    );
}

#[test]
fn empty_buffers_and_byte_boundaries_have_explicit_results() {
    compare(
        r#"
import std.bytes as bytes;
import std.result as result;
import std.option as option;
fn main() {
    let mut empty = system_bytes_zeroed_abort(0);
    let other = system_bytes_zeroed_abort(0);
    let filled = bytes.fill(inout empty, 0, 0, 255);
    let text = bytes.write_str(inout empty, 0, "");
    let copied = bytes.copy(inout empty, 0, other, 0, 0);
    let moved = bytes.copy_within(inout empty, 0, 0, 0);
    let bad_empty = bytes.fill(inout empty, 0, 0, 256);
    let found = bytes.find(empty, 255);
    let mut full = system_bytes_zeroed_abort(2);
    let upper = bytes.fill(inout full, 0, 2, 255);
    let bad_value = bytes.fill(inout full, 0, 2, 2147483647);
    let bad_count = bytes.copy(inout full, 0, other, 0, -1);
    let bad_find = bytes.find(full, -2147483647 - 1);
    bytes.equal(empty, other) && bytes.valid_range(empty, 0, 0) &&
    !bytes.valid_range(empty, 1, 0) && owned_bytes_get(full, 0) == 255 &&
    owned_bytes_get(full, 1) == 255 &&
    (match filled { result.Ok(n) => n == 0 | _ => false }) &&
    (match text { result.Ok(n) => n == 0 | _ => false }) &&
    (match copied { result.Ok(n) => n == 0 | _ => false }) &&
    (match moved { result.Ok(n) => n == 0 | _ => false }) &&
    (match bad_empty { result.Err(bytes.InvalidByte) => true | _ => false }) &&
    (match found { result.Ok(option.None) => true | _ => false }) &&
    (match upper { result.Ok(n) => n == 2 | _ => false }) &&
    (match bad_value { result.Err(bytes.InvalidByte) => true | _ => false }) &&
    (match bad_count { result.Err(bytes.InvalidRange) => true | _ => false }) &&
    (match bad_find { result.Err(bytes.InvalidByte) => true | _ => false })
}
"#,
        3,
    );
}
