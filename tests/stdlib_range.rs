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
            "rill-stdlib-range-{}-{}",
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

fn compare(source: &str) {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    assert_eq!(
        rill::interpreter::run_reference(&program)
            .unwrap()
            .to_string(),
        "true"
    );
    let c = fixture.0.join("main.c");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    let executable = fixture.0.join("main");
    for optimization in ["-O0", "-O3"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&c)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"true\n", "{optimization}");
        assert!(run.stderr.is_empty(), "{optimization}");
    }
}

#[test]
fn half_open_cursors_preserve_bounds_and_alternate_from_both_ends() {
    compare(
        r#"
import std.range as range;
import std.option as option;
import std.result as result;
import std.int as integer;

fn some(actual: option.Option<i32>, expected: i32) -> bool {
    match actual { option.Some(value) => value == expected | option.None => false }
}
fn none(actual: option.Option<i32>) -> bool {
    match actual { option.Some(_) => false | option.None => true }
}
fn length(read cursor: range.Range, expected: i32) -> bool {
    match range.len_checked(cursor) { result.Ok(value) => value == expected | result.Err(_) => false }
}
fn main() -> bool {
    let mut cursor = range.new(-2, 3);
    let saved = cursor;
    let initial = cursor.start == -2 && cursor.end == 3 && !range.is_empty(cursor)
        && range.contains(cursor, -2) && range.contains(cursor, 2)
        && !range.contains(cursor, -3) && !range.contains(cursor, 3) && length(cursor, 5);
    let first = some(range.next(inout cursor), -2) && length(cursor, 4);
    let last = some(range.next_back(inout cursor), 2) && length(cursor, 3);
    let middle = some(range.next_back(inout cursor), 1)
        && some(range.next(inout cursor), -1) && some(range.next(inout cursor), 0);
    let exhausted = range.is_empty(cursor) && length(cursor, 0)
        && none(range.next(inout cursor)) && none(range.next_back(inout cursor));
    let stable = cursor.start == 1 && cursor.end == 1 && saved.start == -2 && saved.end == 3;
    let mut reversed = range.new(7, -4);
    let empty = range.is_empty(reversed) && length(reversed, 0) && !range.contains(reversed, 0)
        && none(range.next(inout reversed)) && none(range.next_back(inout reversed))
        && reversed.start == 7 && reversed.end == -4;
    initial && first && last && middle && exhausted && stable && empty
}
"#,
    );
}

#[test]
fn extreme_endpoints_and_unrepresentable_lengths_do_not_wrap_or_trap() {
    compare(
        r#"
import std.range as range;
import std.option as option;
import std.result as result;
import std.int as integer;

fn some(actual: option.Option<i32>, expected: i32) -> bool {
    match actual { option.Some(value) => value == expected | option.None => false }
}
fn length(read cursor: range.Range, expected: i32) -> bool {
    match range.len_checked(cursor) { result.Ok(value) => value == expected | result.Err(_) => false }
}
fn too_long(read cursor: range.Range) -> bool {
    match range.len_checked(cursor) {
        result.Ok(_) => false | result.Err(error) => match error {
            integer.Overflow => true | integer.DivisionByZero => false | integer.InvalidBounds => false
        }
    }
}
fn main() -> bool {
    let minimum = integer.min_value();
    let maximum = integer.max_value();
    let mut low = range.new(minimum, minimum + 1);
    let mut high = range.new(maximum - 1, maximum);
    let edges = some(range.next(inout low), minimum) && range.is_empty(low)
        && low.start == minimum + 1 && low.end == minimum + 1
        && some(range.next_back(inout high), maximum - 1) && range.is_empty(high)
        && high.start == maximum - 1 && high.end == maximum - 1;
    let mut huge = range.new(minimum, maximum);
    let wide = too_long(huge) && range.contains(huge, minimum)
        && !range.contains(huge, maximum)
        && some(range.next(inout huge), minimum)
        && some(range.next_back(inout huge), maximum - 1) && too_long(huge);
    let fits = length(range.new(minimum, -1), maximum)
        && too_long(range.new(minimum, 0))
        && length(range.new(0, maximum), maximum)
        && length(range.new(maximum, maximum), 0)
        && length(range.new(maximum, minimum), 0);
    edges && wide && fits
}
"#,
    );
}
