//! Scalar/static-cstr native contracts, retained alongside F2 raw transport.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const HEADER: &str = r#"
#ifndef TYPED_FOREIGN_FIXTURE_H
#define TYPED_FOREIGN_FIXTURE_H
#include <stdint.h>
typedef struct TFHandle TFHandle;
typedef struct TFOther TFOther;
int32_t tf_step(int32_t);
int32_t tf_combine(int32_t, int32_t);
void tf_record(int32_t);
int32_t tf_read(void);
_Bool tf_true(void);
_Bool tf_false(void);
_Bool tf_flip(_Bool);
_Bool tf_bools(_Bool, _Bool);
_Bool tf_before(int32_t);
void tf_tick(void);
void tf_set(_Bool);
_Bool tf_read_flag(void);
_Bool tf_read_second(void);
int32_t tf_reads(void);
void tf_capture(int32_t, const char *, int32_t);
int32_t tf_check_text(void);
uint8_t tf_unused_u8(uint8_t);
uint32_t tf_unused_u32(uint32_t);
uint64_t tf_unused_u64(uint64_t);
float tf_unused_f32(float);
TFHandle *tf_unused_pointer(TFHandle *, const TFHandle *);
const TFHandle *tf_unused_const_pointer(const TFHandle *);
void *tf_unused_void(void *);
#define TF_NUMBER 7
#define TF_FLAG (tf_read_flag())
#define TF_COMMA_FLAG tf_read_flag(), (_Bool)1
#define TF_SECOND (tf_read_second())
#define TF_BYTE UINT8_C(255)
#define TF_WORD UINT32_C(4294967295)
#define TF_WIDE UINT64_C(18446744073709551615)
#define TF_FLOAT ((float)1.25)
#define TF_POINTER ((const TFHandle *)0)
#endif
"#;

// This is a separate translation unit for every generated program and the
// handwritten ABI oracle. The header is the only shared type authority.
const IMPLEMENTATION: &str = r#"
#include "typed_foreign_fixture.h"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
static int32_t state;
static int32_t reads;
static _Bool flag = 1;
static const char *saved[2];
static void event(const char *name) { fputs(name, stderr); fflush(stderr); }
int32_t tf_step(int32_t n) {
    fprintf(stderr, "step=%" PRId32 ";", n); fflush(stderr);
    state = state * 10 + n; return n;
}
int32_t tf_combine(int32_t a, int32_t b) {
    fprintf(stderr, "combine=%" PRId32 ":%" PRId32 ";", a, b); fflush(stderr);
    return a * 10 + b;
}
void tf_record(int32_t n) {
    fprintf(stderr, "record=%" PRId32 ";", n); fflush(stderr); state = n;
}
int32_t tf_read(void) { event("read;"); return state; }
_Bool tf_true(void) { event("true;"); return -257; }
_Bool tf_false(void) { event("false;"); return 0; }
_Bool tf_flip(_Bool value) { event(value ? "flip=1;" : "flip=0;"); return !value; }
_Bool tf_bools(_Bool a, _Bool b) { event("bools;"); return a && !b; }
_Bool tf_before(int32_t limit) { event("before;"); return state < limit; }
void tf_tick(void) { event("tick;"); ++state; }
void tf_set(_Bool value) { event(value ? "set=1;" : "set=0;"); flag = value; }
_Bool tf_read_flag(void) { event("flag;"); ++reads; return flag; }
_Bool tf_read_second(void) { event("second;"); ++reads; return flag; }
int32_t tf_reads(void) { event("reads;"); return reads; }
void tf_capture(int32_t slot, const char *text, int32_t mark) {
    fprintf(stderr, "capture=%" PRId32 ":%" PRId32 ";", slot, mark); fflush(stderr);
    if (slot >= 0 && slot < 2) saved[slot] = text;
}
int32_t tf_check_text(void) {
    static const unsigned char expected[] = {
        104, 195, 169, 10, 13, 9, 34, 92, 65, 57, 240, 159, 152, 128, 0
    };
    event("text;");
    return saved[0] && saved[1]
        && memcmp(saved[0], expected, sizeof(expected)) == 0 && saved[1][0] == 0;
}
"#;

const SCALARS: &str = r#"
extern fn tf_step(value: i32) -> i32;
extern fn tf_combine(left: i32, right: i32) -> i32;
extern fn tf_record(value: i32) -> unit;
extern fn tf_read() -> i32;
"#;

const FLAGS: &str = r#"
extern const TF_FLAG: bool;
extern const TF_SECOND: bool;
extern fn tf_set(value: bool) -> unit;
extern fn tf_reads() -> i32;
"#;

fn typed_source(source: &str) -> (rill::ast::Program, String) {
    let program = rill::parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let snapshot = program.clone();
    let checked = rill::check::analyze(&program).unwrap_or_else(|e| panic!("{source}: {e}"));
    let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    rill::typed::verify(&typed).unwrap_or_else(|e| panic!("{source}: {e}"));
    let c = rill::typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(&program).unwrap(), c, "{source}");
    assert_eq!(rill::compile(source).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert_eq!(program, snapshot, "lowering changed the source program");
    (program, c)
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rill-typed-foreign-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("typed_foreign_fixture.h"), HEADER).unwrap();
        fs::write(directory.join("fixture.c"), IMPLEMENTATION).unwrap();
        Self(directory)
    }

    fn source(&self, body: &str) -> String {
        format!("@include(\"typed_foreign_fixture.h\");\n{body}")
    }

    fn run_c(&self, name: &str, c: &str, optimization: &str) -> Output {
        let source = self.0.join(format!("{name}.c"));
        let executable = self.0.join(name);
        fs::write(&source, c).unwrap();
        let build = Command::new("cc")
            .args([
                "-std=c11",
                "-pedantic-errors",
                "-Werror",
                optimization,
                "-I",
            ])
            .arg(&self.0)
            .arg(&source)
            .arg(self.0.join("fixture.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{name} {optimization}: {}\n{c}",
            String::from_utf8_lossy(&build.stderr)
        );
        Command::new(executable).output().unwrap()
    }

    fn compare(&self, body: &str, driver: Option<&str>, expected: Option<&str>, trace: &str) {
        let source = self.source(body);
        let (program, typed_c) = typed_source(&source);
        let reference_c = rill::backend::emit_reference(&program).unwrap();
        assert!(reference_c.contains("rl_value"));
        for optimization in ["-O0", "-O2"] {
            let actual = self.run_c("typed", &typed_c, optimization);
            let reference = self.run_c("reference", &reference_c, optimization);
            assert_eq!(actual.status, reference.status, "{source}: {optimization}");
            assert_eq!(actual.stdout, reference.stdout, "{source}: {optimization}");
            assert_eq!(actual.stderr, reference.stderr, "{source}: {optimization}");
            assert_eq!(actual.stderr, trace.as_bytes(), "{source}: {optimization}");
            if let Some(expected) = expected {
                assert!(actual.status.success(), "{source}: {optimization}");
                assert_eq!(
                    actual.stdout,
                    format!("{expected}\n").as_bytes(),
                    "{source}"
                );
            } else {
                assert!(!actual.status.success(), "{source}: {optimization}");
                assert!(actual.stdout.is_empty(), "{source}: {optimization}");
            }
            if let Some(driver) = driver {
                let driver = format!(
                    "#include \"typed_foreign_fixture.h\"\n#include <inttypes.h>\n#include <stdio.h>\nint main(void) {{ {driver} return 0; }}\n"
                );
                let baseline = self.run_c("abi-oracle", &driver, optimization);
                assert_eq!(actual.status, baseline.status, "{source}: {optimization}");
                assert_eq!(actual.stdout, baseline.stdout, "{source}: {optimization}");
                assert_eq!(actual.stderr, baseline.stderr, "{source}: {optimization}");
            }
        }
    }

    fn compare_generalized(&self, body: &str, driver: &str, expected: &str, trace: &str) {
        // The uniform oracle cannot lay out unresolved generalized local
        // shapes, even when the typed path specializes every concrete use.
        // Only the explicitly selected shape cases use this handwritten C
        // oracle instead. Ordinary native programs retain both C oracles.
        let source = self.source(body);
        let (_, typed_c) = typed_source(&source);
        let driver = format!(
            "#include \"typed_foreign_fixture.h\"\n#include <inttypes.h>\n#include <stdio.h>\nint main(void) {{ {driver} return 0; }}\n"
        );
        for optimization in ["-O0", "-O2"] {
            let actual = self.run_c("typed", &typed_c, optimization);
            let baseline = self.run_c("abi-oracle", &driver, optimization);
            assert!(actual.status.success(), "{source}: {optimization}");
            assert_eq!(
                actual.stdout,
                format!("{expected}\n").as_bytes(),
                "{source}"
            );
            assert_eq!(actual.stderr, trace.as_bytes(), "{source}: {optimization}");
            assert_eq!(actual.status, baseline.status, "{source}: {optimization}");
            assert_eq!(actual.stdout, baseline.stdout, "{source}: {optimization}");
            assert_eq!(actual.stderr, baseline.stderr, "{source}: {optimization}");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn scalar_nested_calls_and_generic_wrappers_keep_source_order() {
    let fixture = Fixture::new();
    fixture.compare(
        &format!(
            "{SCALARS}\nfn id(value) {{ value }}\nfn main() {{ tf_record(0); let n = tf_combine(id(tf_step(1)), tf_step(id(tf_step(2)))); n + tf_read() }}"
        ),
        Some("tf_record(0); int32_t a = tf_step(1); int32_t b = tf_step(2); b = tf_step(b); int32_t n = tf_combine(a, b); int32_t state = tf_read(); printf(\"%\" PRId32 \"\\n\", n + state);"),
        Some("134"),
        "record=0;step=1;step=2;step=2;combine=1:2;read;",
    );
}

#[test]
fn scalar_arguments_snapshot_locals_and_fields_before_later_mutations() {
    let fixture = Fixture::new();
    for setup_and_call in [
        "let mut x = 1; let n = tf_combine(x, { x <- 2; tf_step(x) }); n + x * 100",
        "let mut x = record Cell { value = 1 }; let n = tf_combine(x.value, { x.value <- 2; tf_step(x.value) }); n + x.value * 100",
    ] {
        fixture.compare(
            &format!("{SCALARS} type Cell = {{ value: i32 }} fn main() {{ {setup_and_call} }}"),
            Some("int32_t x = 1; int32_t a = x; x = 2; int32_t b = tf_step(x); int32_t n = tf_combine(a, b); printf(\"%\" PRId32 \"\\n\", n + x * 100);"),
            Some("212"),
            "step=2;combine=1:2;",
        );
    }
    fixture.compare(
        &format!("{SCALARS} type Pair = {{ left: i32; right: i32 }} fn main() {{ let p = record Pair {{ right = tf_step(2); left = tf_step(1) }}; p.left + p.right + tf_read() }}"),
        Some("int32_t right = tf_step(2); int32_t left = tf_step(1); int32_t state = tf_read(); printf(\"%\" PRId32 \"\\n\", left + right + state);"),
        Some("24"),
        "step=2;step=1;read;",
    );
}

#[test]
fn exact_bool_abi_round_trips_and_unit_calls_have_one_effect() {
    let fixture = Fixture::new();
    fixture.compare(
        r#"
extern fn tf_true() -> bool;
extern fn tf_false() -> bool;
extern fn tf_flip(value: bool) -> bool;
extern fn tf_bools(left: bool, right: bool) -> bool;
fn wrap(value) { tf_flip(value) }
fn main() { tf_bools(wrap(tf_false()), wrap(tf_true())) }
"#,
        Some("_Bool a = tf_false(); a = tf_flip(a); _Bool b = tf_true(); b = tf_flip(b); puts(tf_bools(a, b) ? \"true\" : \"false\");"),
        Some("true"),
        "false;flip=0;true;flip=1;bools;",
    );
    for (body, driver, expected, trace) in [
        ("extern fn tf_true() -> bool; fn wrap() { tf_true() } fn main() { wrap() }", "puts(tf_true() ? \"true\" : \"false\");", "true", "true;"),
        ("extern fn tf_false() -> bool; fn wrap() { tf_false() } fn main() { wrap() }", "puts(tf_false() ? \"true\" : \"false\");", "false", "false;"),
        ("extern fn tf_record(value: i32) -> unit; fn wrap() { tf_record(7) } fn main() { wrap() }", "tf_record(7); puts(\"()\");", "()", "record=7;"),
    ] {
        fixture.compare(body, Some(driver), Some(expected), trace);
    }
}

#[test]
fn short_circuit_and_while_conditions_preserve_reached_calls() {
    let fixture = Fixture::new();
    fixture.compare(
        &format!(
            r#"{SCALARS}
extern fn tf_true() -> bool;
extern fn tf_false() -> bool;
extern fn tf_before(limit: i32) -> bool;
extern fn tf_tick() -> unit;
fn main() {{
    let ignored = tf_false() && tf_true();
    let ignored = true || tf_true();
    while tf_before(3) {{ tf_tick() }};
    if false {{ tf_step(1 / 0) }} else {{ tf_read() }}
}}
"#
        ),
        Some("(void)(tf_false() && tf_true()); (void)(1 || tf_true()); while (tf_before(3)) tf_tick(); int32_t n = tf_read(); printf(\"%\" PRId32 \"\\n\", n);"),
        Some("3"),
        "false;before;tick;before;tick;before;tick;before;read;",
    );
}

#[test]
fn static_cstr_retains_exact_utf8_escapes_empty_string_and_terminators() {
    let fixture = Fixture::new();
    fixture.compare(
        &format!(
            r#"{SCALARS}
extern fn tf_capture(slot: i32, text: cstr, mark: i32) -> unit;
extern fn tf_check_text() -> i32;
fn main() {{
    tf_capture(tf_step(0), c"hé\n\r\t\"\\A9😀", tf_step(1));
    tf_record(9);
    tf_capture(tf_step(1), c"", tf_step(2));
    let other = bytes_set(bytes_new(8), 0, 255);
    tf_check_text() + bytes_get(other, 0) - 255
}}
"#
        ),
        Some(r#"static const char text[] = {104, (char)195, (char)169, 10, 13, 9, 34, 92, 65, 57, (char)240, (char)159, (char)152, (char)128, 0}; int32_t slot = tf_step(0); int32_t mark = tf_step(1); tf_capture(slot, text, mark); tf_record(9); slot = tf_step(1); mark = tf_step(2); tf_capture(slot, "", mark); printf("%" PRId32 "\n", tf_check_text());"#),
        Some("1"),
        "step=0;step=1;capture=0:1;record=9;step=1;step=2;capture=1:2;text;",
    );
}

#[test]
fn native_constant_reads_snapshot_once_in_generalized_constructors_and_aliases() {
    let fixture = Fixture::new();
    fixture.compare_generalized(
        &format!(
            "{FLAGS} type Mark<a> = Mark(bool) fn ints(x: Mark<i32>) {{ match x {{ Mark(flag) => flag }} }} fn bools(x: Mark<bool>) {{ match x {{ Mark(flag) => flag }} }} fn main() {{ let p = Mark(TF_FLAG); let alias = p; tf_set(false); if ints(alias) && bools(p) {{ tf_reads() + 40 }} else {{ 0 }} }}"
        ),
        "_Bool saved = TF_FLAG; tf_set(0); int32_t n = saved && saved ? tf_reads() + 40 : 0; printf(\"%\" PRId32 \"\\n\", n);",
        "41",
        "flag;set=0;reads;",
    );
}

#[test]
fn generalized_record_captures_follow_written_order_even_when_unused() {
    let fixture = Fixture::new();
    fixture.compare_generalized(
        &format!(
            r#"{FLAGS}
type Phantom<a> = Phantom
type Snapshot<a> = {{ first: bool; second: bool; marker: Phantom<a> }}
type Unused<a> = Unused(bool)
fn ints(x: Snapshot<i32>) {{ x.first && x.second }}
fn bools(x: Snapshot<bool>) {{ x.first && x.second }}
fn main() {{
    let p = record Snapshot {{ second = TF_SECOND; marker = Phantom; first = TF_FLAG }};
    let alias = p;
    let unused = Unused(TF_FLAG);
    tf_set(false);
    if ints(alias) && bools(p) {{ tf_reads() + 39 }} else {{ 0 }}
}}
"#
        ),
        "_Bool second = TF_SECOND; _Bool first = TF_FLAG; _Bool unused = TF_FLAG; (void)unused; tf_set(0); int32_t n = first && second ? tf_reads() + 39 : 0; printf(\"%\" PRId32 \"\\n\", n);",
        "42",
        "second;flag;flag;set=0;reads;",
    );
}

#[test]
fn ordinary_constant_reads_are_not_hoisted_and_local_shadowing_wins() {
    let fixture = Fixture::new();
    fixture.compare(
        &format!("{FLAGS} extern const TF_NUMBER: i32; fn main() {{ let before = TF_FLAG; tf_set(false); let after = TF_FLAG; if before && after == false {{ tf_reads() + TF_NUMBER }} else {{ 0 }} }}"),
        Some("_Bool before = TF_FLAG; tf_set(0); _Bool after = TF_FLAG; int32_t n = before && !after ? tf_reads() + TF_NUMBER : 0; printf(\"%\" PRId32 \"\\n\", n);"),
        Some("9"),
        "flag;set=0;flag;reads;",
    );
    fixture.compare_generalized(
        &format!("{FLAGS} type Mark<a> = Mark(bool) fn main() {{ let TF_FLAG = false; let unused = Mark(TF_FLAG); tf_reads() }}"),
        "printf(\"%\" PRId32 \"\\n\", tf_reads());",
        "0",
        "reads;",
    );
}

#[test]
fn unused_full_abi_manifest_keeps_header_assertions_in_source_order() {
    let fixture = Fixture::new();
    let body = r#"
extern fn tf_unused_u8(value: u8) -> u8;
extern fn tf_unused_u32(value: u32) -> u32;
extern fn tf_unused_u64(value: u64) -> u64;
extern fn tf_unused_f32(value: f32) -> f32;
extern fn tf_unused_pointer(value: ptr<TFHandle>, other: const_ptr<TFHandle>) -> ptr<TFHandle>;
extern fn tf_unused_const_pointer(value: const_ptr<TFHandle>) -> const_ptr<TFHandle>;
extern fn tf_unused_void(value: ptr<void>) -> ptr<void>;
extern fn tf_capture(slot: i32, text: cstr, mark: i32) -> unit;
extern const TF_BYTE: u8;
extern const TF_WORD: u32;
extern const TF_WIDE: u64;
extern const TF_FLOAT: f32;
extern const TF_POINTER: const_ptr<TFHandle>;
extern const TF_NUMBER: i32;
extern const TF_FLAG: bool;
fn main() { 42 }
"#;
    let source = format!(
        "@include(\"stdint.h\"); @include(\"typed_foreign_fixture.h\"); @include(\"stdint.h\"); @link(\"z\"); @link(\"a\"); @pkg_config(\"second\"); @pkg_config(\"first\");\n{body}"
    );
    let (program, c) = typed_source(&source);
    assert!(c.starts_with(
        "#include <stdint.h>\n#include <typed_foreign_fixture.h>\n#include <stdint.h>\n"
    ));
    let reference = rill::backend::emit_reference(&program).unwrap();
    let assertions = |text: &str| {
        text.lines()
            .filter(|line| line.contains("E-FFI:"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(assertions(&c), assertions(&reference));
    assert_eq!(assertions(&c).len(), 15);
    // The pure main must link without definitions of these unused functions.
    fixture.compare(body, Some("puts(\"42\");"), Some("42"), "");
    fixture.compare(
        "extern fn tf_unused_u64(value: u64) -> u64; extern const TF_WIDE: u64; fn main() { () }",
        Some("puts(\"()\");"),
        Some("()"),
        "",
    );
}

#[test]
fn unused_manual_prototypes_preserve_exact_abi_without_headers() {
    let (_, c) = typed_source(
        "extern fn keep_bool(value: bool) -> bool; extern fn keep_text(value: cstr) -> unit; extern fn keep_raw(value: const_ptr<TFHandle>) -> u64; fn main() { 42 }",
    );
    assert!(c.contains("extern _Bool keep_bool(_Bool);"));
    assert!(c.contains("extern void keep_text(const char *);"));
    assert!(c.contains("extern uint64_t keep_raw(const TFHandle *);"));
}

#[test]
fn cli_check_and_emit_c_still_reject_unused_header_mismatches() {
    let fixture = Fixture::new();
    for (declaration, diagnostic) in [
        ("extern fn tf_true() -> i32;", "incompatible header prototype for tf_true"),
        ("extern fn tf_step(value: bool) -> i32;", "incompatible header prototype for tf_step"),
        ("extern fn tf_unused_u32(value: i32) -> u32;", "incompatible header prototype for tf_unused_u32"),
        ("extern fn tf_unused_pointer(value: ptr<TFOther>, other: const_ptr<TFHandle>) -> ptr<TFHandle>;", "incompatible header prototype for tf_unused_pointer"),
        ("extern fn tf_unused_const_pointer(value: ptr<TFHandle>) -> const_ptr<TFHandle>;", "incompatible header prototype for tf_unused_const_pointer"),
        ("extern const TF_WORD: i32;", "incompatible header constant TF_WORD"),
        ("extern const TF_NUMBER: bool;", "incompatible header constant TF_NUMBER"),
    ] {
        let source = fixture.source(&format!("{declaration} fn main() {{ () }}"));
        // Library emission retains assertions, but must not pretend to have
        // consulted the target compiler already.
        typed_source(&source);
        let path = fixture.0.join("main.rill");
        fs::write(&path, source).unwrap();
        for operation in ["check", "emit-c"] {
            let output = Command::new(env!("CARGO_BIN_EXE_rill"))
                .arg(operation)
                .arg(&path)
                .args(["--cc", "cc", "--c-arg", "-I", "--c-arg"])
                .arg(&fixture.0)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{operation}: {declaration}");
            assert!(output.stdout.is_empty(), "{operation}: {declaration}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(diagnostic),
                "{operation}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn native_calls_do_not_consume_rill_recursion_frames() {
    let fixture = Fixture::new();
    for (depth, expected, trace) in [
        (30, Some("0"), "step=0;"),
        (31, None, "rill: runtime trap\n"),
    ] {
        fixture.compare(
            &format!("extern fn tf_step(value: i32) -> i32; fn recur(n: i32) {{ if n == 0 {{ tf_step(0) }} else {{ recur(n - 1) }} }} fn main() {{ recur({depth}) }}"),
            None,
            expected,
            trace,
        );
    }
}

#[test]
fn traps_preserve_earlier_native_effects_and_suppress_later_arguments() {
    let fixture = Fixture::new();
    fixture.compare(
        &format!(
            "{SCALARS} fn main() {{ tf_combine(tf_step(1), {{ let bad = 1 / 0; tf_step(bad) }}) }}"
        ),
        None,
        None,
        "step=1;rill: runtime trap\n",
    );
    fixture.compare(
        &format!("{SCALARS} fn main() {{ tf_combine(1 / 0, tf_step(2)) }}"),
        None,
        None,
        "rill: runtime trap\n",
    );
}

#[test]
fn typed_evaluator_keeps_pure_paths_and_reached_native_error_precedence() {
    for (source, expected) in [
        ("@include(\"stdint.h\"); fn main() { 42 }", Ok("42")),
        ("extern fn f(value: u64) -> ptr<TFHandle>; fn main() { 42 }", Ok("42")),
        ("extern fn f() -> i32; fn main() { if false { f() } else { 42 } }", Ok("42")),
        ("extern fn f() -> bool; fn main() { false && f() }", Ok("false")),
        ("extern fn f() -> bool; fn main() { true || f() }", Ok("true")),
        ("@include(\"typed_foreign_fixture.h\"); extern const TF_FLAG: bool; fn main() { if false { TF_FLAG } else { true } }", Ok("true")),
        ("extern fn f() -> i32; fn main() { f() }", Err("E-INTERP-FFI: cannot call C function f in the interpreter")),
        ("extern fn f(value: i32) -> i32; fn main() { f(1 / 0) }", Err("runtime trap")),
        ("extern fn f(text: cstr, value: i32) -> i32; fn main() { f(c\"literal\", 1 / 0) }", Err("runtime trap")),
        ("extern fn f(value: i32) -> i32; extern fn g() -> i32; fn main() { f(g()) }", Err("E-INTERP-FFI: cannot call C function g in the interpreter")),
        ("extern fn f(a: i32, b: i32) -> i32; extern fn g() -> i32; fn main() { f(g(), 1 / 0) }", Err("E-INTERP-FFI: cannot call C function g in the interpreter")),
        ("@include(\"typed_foreign_fixture.h\"); extern const TF_NUMBER: i32; fn main() { TF_NUMBER }", Err("E-FFI: imported C constants require native C emission")),
        ("@include(\"typed_foreign_fixture.h\"); extern const TF_FLAG: bool; type M<a> = M(bool) fn main() { let unused = M(TF_FLAG); 42 }", Err("E-FFI: imported C constants require native C emission")),
    ] {
        let (program, _) = typed_source(source);
        let checked = rill::check::analyze(&program).unwrap();
        let typed = rill::typed::lower_main(&checked).unwrap();
        let reference = rill::interpreter::run_reference(&program);
        match expected {
            Ok(value) => assert_eq!(reference.as_ref().unwrap().to_string(), value, "{source}"),
            Err(error) => assert_eq!(reference.as_ref().unwrap_err(), error, "{source}"),
        }
        assert_eq!(rill::typed::evaluate(&typed), reference, "{source}");
        assert_eq!(rill::interpreter::run(&program), reference, "{source}");
    }
}

#[test]
fn runtime_raw_values_use_checked_typed_transport() {
    for source in [
        "fn main() { u8(1) }",
        "fn main() { u32(1) }",
        "fn main() { u64(1) }",
        "fn main() { f32(1) }",
        "extern fn raw() -> u64; fn main() { raw() }",
        "extern fn pointer() -> ptr<TFHandle>; fn main() { is_null(pointer()) }",
        "extern fn use_pointer(value: ptr<TFHandle>) -> i32; fn main() { use_pointer(c_null()) }",
        "@include(\"stdint.h\"); extern const UINT64_MAX: u64; fn main() { UINT64_MAX }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        let ir = rill::typed::lower_main(&checked).unwrap();
        rill::typed::verify(&ir).unwrap();
        let c = rill::backend::emit(&program).unwrap();
        assert!(!c.contains("rl_value"), "{source}");
        assert_eq!(c, rill::typed::emit_c(&ir).unwrap(), "{source}");
        assert!(rill::backend::emit_reference(&program)
            .unwrap()
            .contains("rl_value"));
    }
    for source in [
        "fn main() { u8(1 / 0) }",
        "fn main() { u32(1 / 0) }",
        "fn main() { u64(1 / 0) }",
        "fn main() { f32(1 / 0) }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let expected = Err("E-FFI: raw C values require native C emission".to_string());
        assert_eq!(
            rill::interpreter::run_reference(&program),
            expected,
            "{source}"
        );
        assert_eq!(rill::interpreter::run(&program), expected, "{source}");
    }
}

#[test]
fn foreign_calls_keep_source_loan_exclusion_and_owner_boundary() {
    for source in [
        "extern fn f(value: i32) -> i32; fn keep(inout x: i32, y: i32) { y } fn main() { let mut x = 1; keep(inout x, f(x)) }",
        "extern fn f(value: i32) -> i32; fn keep(read x: i32, y: i32) { y } fn main() { let mut x = 1; keep(x, f({ x <- 2; x })) }",
        "extern fn f() -> i32; fn main() { let owner = system_box_i32_abort(42); box_read(owner) }",
        "@include(\"stdint.h\"); fn main() { let owner = system_box_i32_abort(42); box_read(owner) }",
        "@include(\"typed_foreign_fixture.h\"); extern const TF_NUMBER: i32; fn main() { let owner = system_box_i32_abort(42); box_read(owner) }",
        "extern fn f(value: i32) -> i32; fn main() { f({ let owner = system_box_i32_abort(42); box_read(owner) }) }",
        "extern fn f() -> i32; fn main() { if false { let owner = system_box_i32_abort(42); box_read(owner) } else { 42 } }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        if let Ok(checked) = rill::check::analyze(&program) {
            assert!(rill::typed::lower_main(&checked).is_err(), "{source}");
        }
        assert!(rill::backend::emit(&program).is_err(), "{source}");
        assert!(rill::interpreter::run(&program).is_err(), "{source}");
    }
}

#[test]
fn invalid_scalar_and_cstr_forms_still_fail_before_native_emission() {
    for source in [
        "extern fn f(value: bool) -> unit; fn main() { f(1) }",
        "extern fn f(value: i32) -> unit; fn main() { f(true) }",
        "extern fn f(value: i32) -> unit; fn main() { f() }",
        "extern fn f(value: unit) -> unit; fn main() { 42 }",
        "extern fn f() -> cstr; fn main() { 42 }",
        "extern fn f(value: cstr) -> unit; fn main() { f(\"ordinary\") }",
        "extern fn f(value: cstr) -> unit; fn main() { let text = c\"literal\"; f(text) }",
        "extern fn f(value: cstr) -> unit; fn main() { f(c\"embedded\\0nul\") }",
        "extern fn f(value: cstr) -> unit; fn main() { f(c\"embedded\0nul\") }",
    ] {
        assert!(rill::compile(source).is_err(), "accepted {source:?}");
    }
}

#[test]
fn unparenthesized_bool_comma_macros_remain_single_expression_reads() {
    let fixture = Fixture::new();
    let declarations = "extern const TF_COMMA_FLAG: bool; extern fn tf_reads() -> i32;";
    fixture.compare(
        &format!("{declarations} fn main() {{ let saved = TF_COMMA_FLAG; if saved {{ tf_reads() + 40 }} else {{ 0 }} }}"),
        Some("_Bool saved = (TF_COMMA_FLAG); int32_t n = saved ? tf_reads() + 40 : 0; printf(\"%\" PRId32 \"\\n\", n);"),
        Some("41"),
        "flag;reads;",
    );
    fixture.compare_generalized(
        &format!("{declarations} type Mark<a> = Mark(bool) fn ints(x: Mark<i32>) {{ match x {{ Mark(flag) => flag }} }} fn bools(x: Mark<bool>) {{ match x {{ Mark(flag) => flag }} }} fn main() {{ let saved = Mark(TF_COMMA_FLAG); if ints(saved) && bools(saved) {{ tf_reads() + 40 }} else {{ 0 }} }}"),
        "_Bool saved = (TF_COMMA_FLAG); int32_t n = saved && saved ? tf_reads() + 40 : 0; printf(\"%\" PRId32 \"\\n\", n);",
        "41",
        "flag;reads;",
    );
}
