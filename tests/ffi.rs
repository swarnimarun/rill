use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn calls_scalar_c_functions_and_preserves_argument_order() {
    let source = r#"
extern fn rill_fixture_step(x: i32) -> i32;
extern fn rill_fixture_combine(x: i32, y: i32) -> i32;
extern fn rill_fixture_record(x: i32) -> unit;
extern fn rill_fixture_read() -> i32;
let main = let ignored = rill_fixture_record(0) in rill_fixture_combine(rill_fixture_step(1), rill_fixture_step(2)) + rill_fixture_read()
"#;
    let c = rill::compile(source).unwrap();
    let program = rill::parser::parse(source).unwrap();
    assert!(rill::interpreter::run(&program)
        .unwrap_err()
        .contains("E-INTERP-FFI"));
    let path = std::env::temp_dir().join(format!(
        "rill-ffi-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let generated = path.join("generated.c");
    let fixture = path.join("fixture.c");
    let executable = path.join("program");
    fs::write(&generated, c).unwrap();
    fs::write(
        &fixture,
        r#"
#include <stdint.h>
static int32_t state;
int32_t rill_fixture_step(int32_t x) { state = state * 10 + x; return x; }
int32_t rill_fixture_combine(int32_t x, int32_t y) { return x * 10 + y; }
void rill_fixture_record(int32_t x) { state = x; }
int32_t rill_fixture_read(void) { return state; }
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&generated)
            .arg(&fixture)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"24\n");
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn passes_static_utf8_c_string_bytes_to_c() {
    let source = r#"
extern fn rill_fixture_capture(x: cstr) -> unit;
extern fn rill_fixture_check() -> i32;
let main = let ignored = rill_fixture_capture(c"hé\n\"\\A") in rill_fixture_check()
"#;
    let c = rill::compile(source).unwrap();
    let path = std::env::temp_dir().join(format!(
        "rill-cstr-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let generated = path.join("generated.c");
    let fixture = path.join("fixture.c");
    let executable = path.join("program");
    fs::write(&generated, c).unwrap();
    fs::write(
        &fixture,
        r#"
#include <stdint.h>
#include <string.h>
static const char *saved;
void rill_fixture_capture(const char *value) { saved = value; }
int32_t rill_fixture_check(void) {
    static const unsigned char expected[] = {104, 195, 169, 10, 34, 92, 65, 0};
    return memcmp(saved, expected, sizeof(expected)) == 0 ? 1 : 0;
}
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&generated)
            .arg(&fixture)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"1\n");
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn while_loop_rechecks_foreign_condition_without_recursion() {
    let source = r#"
extern fn rill_loop_get() -> i32;
extern fn rill_loop_tick() -> unit;
let main = let ignored = while rill_loop_get() < 100000 do rill_loop_tick() in rill_loop_get()
"#;
    let c = rill::compile(source).unwrap();
    let path = std::env::temp_dir().join(format!(
        "rill-loop-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let generated = path.join("generated.c");
    let fixture = path.join("fixture.c");
    let executable = path.join("program");
    fs::write(&generated, c).unwrap();
    fs::write(&fixture, "#include <stdint.h>\nstatic int32_t n; int32_t rill_loop_get(void) { return n; } void rill_loop_tick(void) { ++n; }\n").unwrap();
    let output = Command::new("cc")
        .args(["-std=c11", "-O2"])
        .arg(&generated)
        .arg(&fixture)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"100000\n");
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn round_trips_c_booleans_and_preserves_argument_order() {
    let source = r#"
extern fn rill_bool_step(x: bool) -> bool;
extern fn rill_bool_combine(x: bool, y: bool) -> bool;
extern fn rill_bool_state() -> i32;
extern fn rill_bool_true() -> bool;
extern fn rill_bool_false() -> bool;
let wrap x = rill_bool_step(x)
let main = if rill_bool_combine(wrap(rill_bool_false()), wrap(rill_bool_true())) then rill_bool_state() else 0
"#;
    let c = rill::compile(source).unwrap();
    let program = rill::parser::parse(source).unwrap();
    assert!(rill::interpreter::run(&program)
        .unwrap_err()
        .contains("E-INTERP-FFI"));
    let path = std::env::temp_dir().join(format!(
        "rill-bool-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let generated = path.join("generated.c");
    let fixture = path.join("fixture.c");
    let executable = path.join("program");
    let header = path.join("fixture.h");
    fs::write(
        &header,
        r#"
#include <stdint.h>
#include <stdbool.h>
bool rill_bool_step(bool);
bool rill_bool_combine(bool, bool);
int32_t rill_bool_state(void);
bool rill_bool_true(void);
bool rill_bool_false(void);
"#,
    )
    .unwrap();
    fs::write(&generated, c).unwrap();
    fs::write(
        &fixture,
        r#"
#include <stdint.h>
#include <stdbool.h>
static int32_t state;
bool rill_bool_step(bool x) { state = state * 10 + (x ? 2 : 1); return !x; }
bool rill_bool_combine(bool x, bool y) { return x && !y; }
int32_t rill_bool_state(void) { return state; }
bool rill_bool_true(void) { return -257; }
bool rill_bool_false(void) { return 0; }
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2", "-O3"] {
        let output = Command::new("cc")
            .args(["-std=c11", opt, "-Werror", "-include"])
            .arg(&header)
            .arg(&generated)
            .arg(&fixture)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"12\n");
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn rejects_integer_boolean_ffi_mismatches() {
    for source in [
        "extern fn f(x: bool) -> unit; let main = f(1)",
        "extern fn f(x: i32) -> unit; let main = f(true)",
        "extern fn f() -> bool; let main = f() + 1",
        "extern fn f() -> i32; let main = if f() then 1 else 0",
    ] {
        assert!(rill::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn foreign_boolean_result_survives_rill_return_and_main() {
    for value in ["true", "false"] {
        let source =
            "extern fn rill_bool() -> bool; let forward = rill_bool()\nlet main = forward()";
        let c = rill::compile(source).unwrap();
        let path = std::env::temp_dir().join(format!(
            "rill-bool-result-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let generated = path.join("generated.c");
        let fixture = path.join("fixture.c");
        let executable = path.join("program");
        fs::write(&generated, c).unwrap();
        fs::write(
            &fixture,
            format!("#include <stdbool.h>\nbool rill_bool(void) {{ return {value}; }}"),
        )
        .unwrap();
        let output = Command::new("cc")
            .args(["-std=c11", "-O2"])
            .arg(&generated)
            .arg(&fixture)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, format!("{value}\n").as_bytes());
        fs::remove_dir_all(path).unwrap();
    }
}
