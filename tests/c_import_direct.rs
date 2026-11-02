use std::{
    ffi::OsStr,
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rill-direct-ffi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("api.h"),
            r#"
#include <stdint.h>
typedef struct Window { int value; } Window;
typedef struct Renderer { int value; } Renderer;
#define LARGE_FLAGS UINT64_C(18446744073709551615)
#define SMALL_FLAGS 128
#define TOO_WIDE (((__uint128_t)1) << 100)
static Window singleton = { 7 };
static Window *get_window(void) { return &singleton; }
static int32_t use_window(Window *p) { return p ? p->value : 3; }
static void use_renderer(Renderer *p) { (void)p; }
static uint64_t roundtrip(uint64_t x) { return x; }
static uint32_t unsigned_result(void) { return UINT32_C(4294967295); }
static _Bool exact_bool(uint32_t flags) { return flags == 128; }
static float scalar_float(float x) { return x + 0.5f; }
static uint8_t byte_value(uint8_t x) { return x; }
"#,
        )
        .unwrap();
        Self(dir)
    }
    fn source(&self, body: &str) -> String {
        format!("@include(\"{}\");\n{body}", self.0.join("api.h").display())
    }
    fn compile_and_run(&self, body: &str, expected: &[u8]) {
        let c = rill::compile(&self.source(body)).unwrap();
        rill::c_import::validate_c(&c, OsStr::new("cc"), false, &[]).unwrap();
        let source = self.0.join("generated.c");
        let executable = self.0.join("program");
        fs::write(&source, c).unwrap();
        for opt in ["-O0", "-O2", "-O3"] {
            let output = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&source)
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
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected);
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preserves_full_width_unsigned_float_and_nominal_pointer_values() {
    let f = Fixture::new();
    f.compile_and_run("extern const LARGE_FLAGS: u64; extern fn roundtrip(x: u64) -> u64; fn main() { roundtrip(LARGE_FLAGS) }", b"18446744073709551615\n");
    f.compile_and_run(
        "extern fn unsigned_result() -> u32; fn main() { unsigned_result() }",
        b"4294967295\n",
    );
    f.compile_and_run(
        "extern fn scalar_float(x: f32) -> f32; fn main() { scalar_float(f32(2)) }",
        b"2.5\n",
    );
    f.compile_and_run("extern fn get_window() -> ptr<Window>; extern fn use_window(w: ptr<Window>) -> i32; fn main() { let mut p = c_null(); p <- get_window(); use_window(p) }", b"7\n");
    f.compile_and_run("extern fn get_window() -> ptr<Window>; extern fn use_window(w: ptr<Window>) -> i32; fn main() { use_window(if false { get_window() } else { c_null() }) }", b"3\n");
    f.compile_and_run("extern const SMALL_FLAGS: u32; extern fn exact_bool(flags: u32) -> bool; fn main() { exact_bool(SMALL_FLAGS) }", b"true\n");
}

#[test]
fn c_values_survive_rill_record_storage() {
    let f = Fixture::new();
    f.compile_and_run("extern const LARGE_FLAGS: u64; extern fn roundtrip(x: u64) -> u64; type State = { flags: u64 } fn main() { let state = record State { flags = LARGE_FLAGS }; roundtrip(state.flags) }", b"18446744073709551615\n");
    f.compile_and_run("extern fn get_window() -> ptr<Window>; extern fn use_window(w: ptr<Window>) -> i32; type State = { window: ptr<Window> } fn main() { let state = record State { window = get_window() }; use_window(state.window) }", b"7\n");
}

#[test]
fn header_rejects_scalar_bool_and_pointee_mismatches() {
    let f = Fixture::new();
    for body in [
        "extern fn exact_bool(x: u32) -> i32; fn main() { 0 }",
        "extern fn exact_bool(x: i32) -> bool; fn main() { 0 }",
        "extern fn get_window() -> ptr<Renderer>; fn main() { 0 }",
        "extern fn use_window(w: const_ptr<Window>) -> i32; fn main() { 0 }",
        "extern const LARGE_FLAGS: u32; fn main() { 0 }",
        "extern const TOO_WIDE: u64; fn main() { 0 }",
    ] {
        let c = rill::compile(&f.source(body)).unwrap();
        let error = rill::c_import::validate_c(&c, OsStr::new("cc"), false, &[]).unwrap_err();
        assert!(error.contains("E-FFI: incompatible header"), "{error}");
    }
}

#[test]
fn null_inference_cannot_erase_pointer_identity_or_constness() {
    let declarations = "extern fn get_window() -> ptr<Window>; extern fn use_renderer(r: ptr<Renderer>) -> unit; extern fn use_window(w: ptr<Window>) -> i32;";
    for body in [
        "let mut p = c_null(); p <- get_window(); use_renderer(p)",
        "let p = if true { c_null() } else { get_window() }; use_renderer(p)",
        "let mut p = c_null(); use_renderer(p); p <- get_window()",
        "let p = c_null(); use_renderer(p); use_window(p)",
    ] {
        let source = format!("{declarations} fn main() {{ {body} }}");
        let program = rill::parser::parse(&source).unwrap();
        assert!(rill::check::check(&program)
            .unwrap_err()
            .contains("incompatible types"));
    }
    for body in ["c_null()", "is_null(0)", "let unused = c_null(); 0"] {
        let program = rill::parser::parse(&format!("fn main() {{ {body} }}")).unwrap();
        assert!(rill::check::check(&program).is_err());
    }
}

#[test]
fn checked_numeric_conversions_trap_out_of_range() {
    let f = Fixture::new();
    for body in ["u8(256)", "u32(-1)", "u64(-1)"] {
        let c = rill::compile(&f.source(&format!("fn main() {{ {body} }}"))).unwrap();
        let source = f.0.join("generated.c");
        let executable = f.0.join("program");
        fs::write(&source, c).unwrap();
        assert!(Command::new("cc")
            .args(["-std=c11", "-O2"])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success());
        let output = Command::new(&executable).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("runtime trap"));
    }
}
