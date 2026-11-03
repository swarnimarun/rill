//! F2 transports the existing Copy foreign scalars and raw pointers through
//! checked typed IR. The C fixture is a separate translation unit; no host FFI
//! evaluator or C-callable Rill exports are implied by these round trips.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const HEADER: &str = r#"
#ifndef TYPED_RAW_FIXTURE_H
#define TYPED_RAW_FIXTURE_H
#include <stdint.h>
typedef struct RFHandle RFHandle;
typedef struct RFOther RFOther;
typedef RFHandle RFAlias;
typedef RFHandle _RFHandle;
uint8_t rf_u8(uint8_t);
uint32_t rf_u32(uint32_t);
uint64_t rf_u64(uint64_t);
float rf_f32(float);
uint32_t rf_word(void);
uint64_t rf_wide(void);
float rf_float(int32_t);
RFHandle *rf_handle(int32_t);
const RFHandle *rf_const_handle(int32_t);
int32_t rf_id(RFHandle *);
int32_t rf_const_id(const RFHandle *);
_RFHandle *rf_underscore(int32_t);
const _RFHandle *rf_underscore_const(int32_t);
int32_t rf_underscore_id(_RFHandle *);
int32_t rf_underscore_const_id(const _RFHandle *);
int32_t rf_pair(RFHandle *, int32_t);
int32_t rf_change(RFHandle *, int32_t);
int32_t rf_contents(RFHandle *);
void *rf_void(void);
const void *rf_const_void(void);
int32_t *rf_i32_pointer(void);
uint8_t *rf_u8_pointer(void);
uint32_t *rf_u32_pointer(void);
uint64_t *rf_u64_pointer(void);
float *rf_f32_pointer(void);
_Bool *rf_bool_pointer(void);
int32_t rf_mark(int32_t);
int32_t rf_order(int32_t, uint8_t, int32_t);
int32_t rf_mode(void);
void rf_set(_Bool);
RFHandle *rf_read_pointer(void);
float rf_read_float(void);
_Bool rf_read_flag(void);
int32_t rf_reads(void);
#define RF_BYTE UINT8_C(255)
#define RF_WORD UINT32_C(4294967295)
#define RF_WIDE UINT64_C(18446744073709551615)
#define RF_POINTER (rf_read_pointer())
#define RF_FLOAT (rf_read_float())
#define RF_COMMA_FLOAT rf_read_float(), ((float)2)
#define RF_COMMA_POINTER rf_read_pointer(), ((RFHandle *)0)
#define RF_FLAG (rf_read_flag())
#define RF_NULL ((RFHandle *)0)
#define RF_CONST_NULL ((const RFHandle *)0)
#endif
"#;

const IMPLEMENTATION: &str = r#"
#include "typed_raw_fixture.h"
#include <inttypes.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
struct RFHandle { int32_t id; int32_t value; };
static RFHandle handles[2] = {{1, 10}, {2, 20}};
static int32_t count;
static _Bool flag = 1;
static void event(const char *s) { fputs(s, stderr); fflush(stderr); }
static void number(const char *s, int32_t n) {
    fprintf(stderr, "%s=%" PRId32 ";", s, n); fflush(stderr);
}
uint8_t rf_u8(uint8_t x) { event("u8;"); return x; }
uint32_t rf_u32(uint32_t x) { event("u32;"); return x; }
uint64_t rf_u64(uint64_t x) { event("u64;"); return x; }
float rf_f32(float x) { event("f32;"); return x; }
uint32_t rf_word(void) { event("word;"); return UINT32_MAX; }
uint64_t rf_wide(void) { event("wide;"); return UINT64_MAX; }
float rf_float(int32_t which) {
    number("float", which);
    switch (which) {
    case 0: return NAN;
    case 1: return INFINITY;
    case 2: return -0.0f;
    default: return 0.0f;
    }
}
RFHandle *rf_handle(int32_t id) {
    number("handle", id);
    return id == 1 ? &handles[0] : id == 2 ? &handles[1] : NULL;
}
const RFHandle *rf_const_handle(int32_t id) {
    number("const", id);
    return id == 1 ? &handles[0] : id == 2 ? &handles[1] : NULL;
}
int32_t rf_id(RFHandle *p) { int32_t n = p ? p->id : 0; number("id", n); return n; }
int32_t rf_const_id(const RFHandle *p) {
    int32_t n = p ? p->id : 0; number("cid", n); return n;
}
_RFHandle *rf_underscore(int32_t id) { return rf_handle(id); }
const _RFHandle *rf_underscore_const(int32_t id) { return rf_const_handle(id); }
int32_t rf_underscore_id(_RFHandle *p) { return rf_id(p); }
int32_t rf_underscore_const_id(const _RFHandle *p) { return rf_const_id(p); }
int32_t rf_pair(RFHandle *p, int32_t n) {
    int32_t id = p ? p->id : 0;
    fprintf(stderr, "pair=%" PRId32 ":%" PRId32 ";", id, n); fflush(stderr);
    return id * 100 + n;
}
int32_t rf_change(RFHandle *p, int32_t n) {
    number("change", n); p->value = n; return 0;
}
int32_t rf_contents(RFHandle *p) { number("contents", p->value); return p->value; }
void *rf_void(void) { return &handles[0]; }
const void *rf_const_void(void) { return &handles[0]; }
int32_t *rf_i32_pointer(void) { static int32_t x; return &x; }
uint8_t *rf_u8_pointer(void) { static uint8_t x; return &x; }
uint32_t *rf_u32_pointer(void) { static uint32_t x; return &x; }
uint64_t *rf_u64_pointer(void) { static uint64_t x; return &x; }
float *rf_f32_pointer(void) { static float x; return &x; }
_Bool *rf_bool_pointer(void) { static _Bool x; return &x; }
int32_t rf_mark(int32_t n) { number("mark", n); return n; }
int32_t rf_order(int32_t a, uint8_t b, int32_t c) {
    event("order;"); return a + b + c;
}
int32_t rf_mode(void) {
    const char *value = getenv("RILL_RAW_CASE"); return value ? (int32_t)atoi(value) : 0;
}
void rf_set(_Bool value) { flag = value; event(value ? "set=1;" : "set=0;"); }
RFHandle *rf_read_pointer(void) {
    event("pointer;"); ++count; return &handles[flag ? 0 : 1];
}
float rf_read_float(void) { event("real;"); ++count; return flag ? 1.25f : 2.5f; }
_Bool rf_read_flag(void) { event("flag;"); ++count; return flag; }
int32_t rf_reads(void) { event("reads;"); return count; }
"#;

const SCALARS: &str = r#"
extern fn rf_u8(value: u8) -> u8;
extern fn rf_u32(value: u32) -> u32;
extern fn rf_u64(value: u64) -> u64;
extern fn rf_f32(value: f32) -> f32;
extern fn rf_word() -> u32;
extern fn rf_wide() -> u64;
extern fn rf_float(which: i32) -> f32;
extern const RF_BYTE: u8;
extern const RF_WORD: u32;
extern const RF_WIDE: u64;
"#;

const POINTERS: &str = r#"
extern fn rf_handle(id: i32) -> ptr<RFHandle>;
extern fn rf_const_handle(id: i32) -> const_ptr<RFHandle>;
extern fn rf_id(value: ptr<RFHandle>) -> i32;
extern fn rf_const_id(value: const_ptr<RFHandle>) -> i32;
extern fn rf_pair(value: ptr<RFHandle>, mark: i32) -> i32;
extern fn rf_mark(value: i32) -> i32;
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
    assert!(
        !c.contains("restrict"),
        "raw pointer slots imply no pointee noalias"
    );
    assert_eq!(program, snapshot, "lowering changed the source program");
    (program, c)
}

fn executable_identity(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find_map(|path| fs::canonicalize(path).ok())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn compilers(all: bool) -> &'static [String] {
    static COMPILERS: OnceLock<Vec<String>> = OnceLock::new();
    let available = COMPILERS.get_or_init(|| {
        let mut seen = BTreeSet::new();
        let mut available = Vec::new();
        for name in ["cc", "gcc", "clang"] {
            let version = Command::new(name).arg("--version").output();
            if version.is_ok_and(|output| output.status.success()) {
                if seen.insert(executable_identity(name)) {
                    available.push(name.to_string());
                }
            } else if name == "cc" {
                panic!("typed raw native tests require a working C11 cc compiler");
            } else {
                eprintln!("SKIP optional typed raw ABI compiler: {name} unavailable");
            }
        }
        available
    });
    if all {
        available
    } else {
        &available[..1]
    }
}

struct Fixture {
    directory: PathBuf,
    objects: RefCell<BTreeMap<(String, String), PathBuf>>,
}

struct Case<'a> {
    body: &'a str,
    driver: &'a str,
    // None leaves target-specific float/null text to the same-target C oracle.
    stdout: Option<&'a str>,
    trace: &'a str,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rill-typed-raw-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("typed_raw_fixture.h"), HEADER).unwrap();
        fs::write(directory.join("fixture.c"), IMPLEMENTATION).unwrap();
        Self {
            directory,
            objects: RefCell::new(BTreeMap::new()),
        }
    }

    fn source(&self, body: &str) -> String {
        format!("@include(\"typed_raw_fixture.h\");\n{body}")
    }

    fn compiler(&self, compiler: &str, optimization: &str) -> Command {
        let mut command = Command::new(compiler);
        command
            .args([
                "-std=c11",
                "-pedantic-errors",
                "-Werror",
                "-Werror=incompatible-pointer-types",
                optimization,
                "-I",
            ])
            .arg(&self.directory);
        command
    }

    fn object(&self, compiler: &str, optimization: &str) -> PathBuf {
        let key = (compiler.to_string(), optimization.to_string());
        if let Some(path) = self.objects.borrow().get(&key) {
            return path.clone();
        }
        let path = self
            .directory
            .join(format!("fixture-{compiler}-{optimization}.o"));
        let output = self
            .compiler(compiler, optimization)
            .arg("-c")
            .arg(self.directory.join("fixture.c"))
            .arg("-o")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture {compiler} {optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.objects.borrow_mut().insert(key, path.clone());
        path
    }

    fn build(&self, name: &str, c: &str, compiler: &str, optimization: &str) -> PathBuf {
        let source = self.directory.join(format!("{name}.c"));
        let executable = self
            .directory
            .join(format!("{name}-{compiler}-{optimization}"));
        fs::write(&source, c).unwrap();
        let output = self
            .compiler(compiler, optimization)
            .arg(&source)
            .arg(self.object(compiler, optimization))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name} {compiler} {optimization}: {}\n{c}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    }

    fn compare(&self, case: Case<'_>, all_compilers: bool, generalized: bool) {
        let source = self.source(case.body);
        let (program, typed) = typed_source(&source);
        // The independent uniform emitter cannot instantiate unresolved local
        // schemes. Only these explicitly marked generalized-shape cases omit
        // that oracle; the handwritten C driver remains mandatory for them.
        let reference = (!generalized).then(|| {
            let c = rill::backend::emit_reference(&program).unwrap();
            assert!(c.contains("rl_value"));
            c
        });
        let driver = format!(
            "#include \"typed_raw_fixture.h\"\n#include <inttypes.h>\n#include <stdio.h>\nint main(void) {{ {} return 0; }}\n",
            case.driver
        );
        for compiler in compilers(all_compilers) {
            for optimization in ["-O0", "-O2"] {
                let label = format!("{compiler} {optimization}: {source}");
                let actual = run(&self.build("typed", &typed, compiler, optimization), None);
                assert_eq!(actual.status.code(), Some(0), "{label}: {actual:?}");
                assert_eq!(actual.stderr, case.trace.as_bytes(), "{label}");
                if let Some(expected) = case.stdout {
                    assert_eq!(actual.stdout, format!("{expected}\n").as_bytes(), "{label}");
                }
                let baseline = run(&self.build("oracle", &driver, compiler, optimization), None);
                same_output(&actual, &baseline, &label);
                if let Some(reference) = &reference {
                    let reference = run(
                        &self.build("reference", reference, compiler, optimization),
                        None,
                    );
                    same_output(&actual, &reference, &label);
                }
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn run(executable: &Path, mode: Option<i32>) -> Output {
    let mut command = Command::new(executable);
    command.env_remove("RILL_RAW_CASE");
    if let Some(mode) = mode {
        command.env("RILL_RAW_CASE", mode.to_string());
    }
    command.output().unwrap()
}

fn same_output(actual: &Output, expected: &Output, label: &str) {
    assert_eq!(actual.status, expected.status, "{label}");
    assert_eq!(actual.stdout, expected.stdout, "{label}");
    assert_eq!(actual.stderr, expected.stderr, "{label}");
}

#[test]
fn scalar_conversions_full_width_abi_generics_and_nested_printers_match_target_c() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{SCALARS}
type Wrap<a> = Wrap(a)
type Numbers = {{ zero: u8; byte: u8; word: u32; wide: u64; signed_word: u32; signed_wide: u64; real: f32; negative: f32; rounded: f32; nested: Wrap<Wrap<u64>> }}
fn id(value) {{ value }}
fn main() {{
    let z = rf_u8(u8(0));
    let b = rf_u8(id(RF_BYTE));
    let w = rf_u32(id(rf_word()));
    let d = rf_u64(id(rf_wide()));
    let sw = rf_u32(u32(2147483647));
    let sd = rf_u64(u64(2147483647));
    let f = rf_f32(f32(0));
    let n = rf_f32(id(f32(-1)));
    let r = rf_f32(f32(16777217));
    record Numbers {{ zero = z; byte = b; word = w; wide = d; signed_word = sw; signed_wide = sd; real = f; negative = n; rounded = r; nested = Wrap(Wrap(RF_WIDE)) }}
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: r#"uint8_t z = rf_u8((uint8_t)0); uint8_t b = rf_u8(RF_BYTE); uint32_t w = rf_word(); w = rf_u32(w); uint64_t d = rf_wide(); d = rf_u64(d); uint32_t sw = rf_u32((uint32_t)INT32_MAX); uint64_t sd = rf_u64((uint64_t)INT32_MAX); float f = rf_f32((float)0); float n = rf_f32((float)-1); float r = rf_f32((float)16777217); printf("Numbers { zero = %u; byte = %u; word = %" PRIu32 "; wide = %" PRIu64 "; signed_word = %" PRIu32 "; signed_wide = %" PRIu64 "; real = %.9g; negative = %.9g; rounded = %.9g; nested = Wrap(Wrap(%" PRIu64 ")) }\n", (unsigned)z, (unsigned)b, w, d, sw, sd, (double)f, (double)n, (double)r, (uint64_t)RF_WIDE);"#,
        stdout: None,
        trace: "u8;u8;word;u32;wide;u64;u32;u64;f32;f32;f32;",
    }, true, false);
}

#[test]
fn conversion_children_collect_bytes_vectors_strings_and_nested_rill_calls() {
    let fixture = Fixture::new();
    fixture.compare(Case {
        body: r#"
extern fn rf_mark(value: i32) -> i32;
type RawChildren = { bytes: u32; lane: u8; text: u64; call: f32 }
fn id(x) { x }
fn increment(n: i32) { n + 1 }
fn main() {
    record RawChildren {
        bytes = u32(bytes_len(bytes_new(9)));
        lane = u8(u8x16_lane(u8x16_load(bytes_set(bytes_new(16), 3, 255), 0), 3));
        text = u64(byte_len("é"));
        call = f32(increment(id(rf_mark(41))))
    }
}
"#,
        driver: "int32_t n = rf_mark(41); printf(\"RawChildren { bytes = %\" PRIu32 \"; lane = %u; text = %\" PRIu64 \"; call = %.9g }\\n\", (uint32_t)9, (unsigned)(uint8_t)255, (uint64_t)2, (double)(float)(n + 1));",
        stdout: Some("RawChildren { bytes = 9; lane = 255; text = 2; call = 42 }"),
        trace: "mark=41;",
    }, false, false);
}

#[test]
fn full_width_unsigned_and_float_results_use_the_entry_result_printer() {
    let fixture = Fixture::new();
    for (body, driver, stdout, trace) in [
        (
            "extern fn rf_word() -> u32; fn wrap() { rf_word() } fn main() { wrap() }",
            "printf(\"%\" PRIu32 \"\\n\", rf_word());",
            Some("4294967295"),
            "word;",
        ),
        (
            "extern const RF_WIDE: u64; fn id(x) { x } fn main() { id(RF_WIDE) }",
            "printf(\"%\" PRIu64 \"\\n\", (uint64_t)RF_WIDE);",
            Some("18446744073709551615"),
            "",
        ),
        (
            "fn main() { u8(255) }",
            "printf(\"%u\\n\", (unsigned)(uint8_t)255);",
            Some("255"),
            "",
        ),
        (
            "fn main() { f32(16777217) }",
            "printf(\"%.9g\\n\", (double)(float)16777217);",
            None,
            "",
        ),
    ] {
        fixture.compare(
            Case {
                body,
                driver,
                stdout,
                trace,
            },
            false,
            false,
        );
    }
}

#[test]
fn raw_equality_matches_c_nan_infinity_signed_zero_and_unsigned_edges() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{SCALARS}
fn main() {{
    let nan = rf_float(0); let inf = rf_float(1); let negative_zero = rf_float(2);
    u8(0) != RF_BYTE && u8(255) == RF_BYTE && RF_WORD == rf_word()
    && u32(0) != RF_WORD && RF_WIDE == rf_wide() && u64(0) != RF_WIDE
    && nan != nan && (nan == nan) == false && inf == inf && inf != f32(0)
    && negative_zero == f32(0) && (negative_zero != f32(0)) == false
    && f32(-1) == f32(-1) && f32(-1) != f32(1)
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "float nan = rf_float(0); float inf = rf_float(1); float nz = rf_float(2); float nan_copy = nan; float inf_copy = inf; float minus_one = (float)-1; float minus_copy = minus_one; _Bool ok = (uint8_t)0 != RF_BYTE && (uint8_t)255 == RF_BYTE && RF_WORD == rf_word() && (uint32_t)0 != RF_WORD && RF_WIDE == rf_wide() && (uint64_t)0 != RF_WIDE && nan != nan_copy && !(nan == nan_copy) && inf == inf_copy && inf != (float)0 && nz == (float)0 && !(nz != (float)0) && minus_one == minus_copy && minus_one != (float)1; puts(ok ? \"true\" : \"false\");",
        stdout: Some("true"),
        trace: "float=0;float=1;float=2;word;wide;",
    }, true, false);
}

#[test]
fn nullable_pointer_kinds_keep_exact_identity_through_generic_returns() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{POINTERS}
extern fn rf_void() -> ptr<void>;
extern fn rf_const_void() -> const_ptr<void>;
extern fn rf_i32_pointer() -> ptr<i32>;
extern fn rf_u8_pointer() -> ptr<u8>;
extern fn rf_u32_pointer() -> ptr<u32>;
extern fn rf_u64_pointer() -> ptr<u64>;
extern fn rf_f32_pointer() -> ptr<f32>;
extern fn rf_bool_pointer() -> ptr<bool>;
fn id(x) {{ x }}
fn same(read x: ptr<RFHandle>, y: ptr<RFHandle>) {{ x == y }}
fn same_void(read x: ptr<void>, y: ptr<void>) {{ x == y }}
fn main() {{
    let mut p = c_null(); let absent = is_null(p); p <- id(rf_handle(1));
    let q = id(rf_handle(2)); let cp = id(rf_const_handle(1));
    let nil = if false {{ q }} else {{ c_null() }};
    absent && is_null(nil) && p == p && p != q && same(p, p)
    && cp == id(rf_const_handle(1)) && cp != c_null()
    && is_null(id(rf_void())) == false && same_void(rf_void(), id(rf_void()))
    && is_null(id(rf_const_void())) == false
    && is_null(id(rf_i32_pointer())) == false && is_null(id(rf_u8_pointer())) == false
    && is_null(id(rf_u32_pointer())) == false && is_null(id(rf_u64_pointer())) == false
    && is_null(id(rf_f32_pointer())) == false && is_null(id(rf_bool_pointer())) == false
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "RFHandle *p = NULL; _Bool absent = p == NULL; p = rf_handle(1); RFHandle *q = rf_handle(2); const RFHandle *cp = rf_const_handle(1); RFHandle *nil = NULL; RFHandle *same_p = p; _Bool ok = absent && nil == NULL && p == same_p && p != q && p == same_p && cp == rf_const_handle(1) && cp != NULL && rf_void() != NULL && rf_void() == rf_void() && rf_const_void() != NULL && rf_i32_pointer() != NULL && rf_u8_pointer() != NULL && rf_u32_pointer() != NULL && rf_u64_pointer() != NULL && rf_f32_pointer() != NULL && rf_bool_pointer() != NULL; puts(ok ? \"true\" : \"false\");",
        stdout: Some("true"),
        trace: "handle=1;handle=2;const=1;const=1;",
    }, true, false);
}

#[test]
fn header_provided_underscore_pointees_preserve_spelling_and_constness() {
    let fixture = Fixture::new();
    let body = r#"
extern fn rf_underscore(id: i32) -> ptr<_RFHandle>;
extern fn rf_underscore_const(id: i32) -> const_ptr<_RFHandle>;
extern fn rf_underscore_id(value: ptr<_RFHandle>) -> i32;
extern fn rf_underscore_const_id(value: const_ptr<_RFHandle>) -> i32;
fn id(x) { x }
fn main() {
    let mut p = c_null(); p <- id(rf_underscore(1));
    let q = id(rf_underscore_const(2));
    let absent = if false { p } else { c_null() };
    if is_null(absent) && p == id(p) && q == id(q) {
        rf_underscore_id(p) * 10 + rf_underscore_const_id(q)
    } else { 0 }
}
"#;
    let (_, c) = typed_source(&fixture.source(body));
    assert!(c.contains("typedef _RFHandle * rl_cptr_"));
    assert!(c.contains("typedef const _RFHandle * rl_cptr_"));
    fixture.compare(Case {
        body,
        driver: "_RFHandle *p = rf_underscore(1); const _RFHandle *q = rf_underscore_const(2); _RFHandle *absent = NULL; _RFHandle *saved_p = p; const _RFHandle *saved_q = q; int32_t result = 0; if (absent == NULL && p == saved_p && q == saved_q) { result = rf_underscore_id(p) * 10; result += rf_underscore_const_id(q); } printf(\"%\" PRId32 \"\\n\", result);",
        stdout: Some("12"), trace: "handle=1;const=2;id=1;cid=2;",
    }, true, false);
}

#[test]
fn null_pointer_main_and_nested_printers_match_target_formatting() {
    let fixture = Fixture::new();
    for (body, driver) in [
        ("extern const RF_NULL: ptr<RFHandle>; fn main() { RF_NULL }", "printf(\"%p\\n\", (void *)RF_NULL);"),
        ("extern const RF_CONST_NULL: const_ptr<RFHandle>; type Boxed<a> = Boxed(a) type R = { p: Boxed<const_ptr<RFHandle>> } fn main() { record R { p = Boxed(RF_CONST_NULL) } }", "printf(\"R { p = Boxed(%p) }\\n\", (void *)RF_CONST_NULL);"),
    ] {
        fixture.compare(Case { body, driver, stdout: None, trace: "" }, false, false);
    }
}

#[test]
fn pointer_root_field_read_inout_forwarding_preserves_both_const_levels() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{POINTERS}
type Pair = {{ left: ptr<RFHandle>; right: ptr<RFHandle>; shared: const_ptr<RFHandle> }}
type Outer = {{ pair: Pair; spare: ptr<RFHandle> }}
type Wrap<a> = Wrap(a)
fn swap(inout a, inout b) {{ let old = a; a <- b; b <- old }}
fn set(inout p: ptr<RFHandle>, q: ptr<RFHandle>) {{ p <- q }}
fn forward(inout p: Outer) {{ swap(inout p.pair.left, inout p.pair.right) }}
fn inspect(read p: ptr<RFHandle>) {{ rf_id(p) }}
fn inspect_const(read p: const_ptr<RFHandle>) {{ rf_const_id(p) }}
fn relay(read p: Pair) {{ inspect(p.left) + inspect_const(p.shared) }}
fn copy_to(read p: ptr<RFHandle>, inout q: ptr<RFHandle>) {{ q <- p }}
fn main() {{
    let mut p = record Outer {{ spare = c_null(); pair = record Pair {{ right = rf_handle(2); shared = rf_const_handle(1); left = rf_handle(1) }} }};
    forward(inout p); copy_to(p.pair.left, inout p.spare);
    let a = inspect(p.spare); let b = relay(p.pair);
    set(inout p.pair.right, p.spare);
    let nested = Wrap(Wrap(p.pair.right));
    let c = match nested {{ Wrap(Wrap(value)) => inspect(value) }};
    let ignored = match Wrap(p.pair.shared) {{ Wrap(_) => 0 }};
    a * 100 + b * 10 + c
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "RFHandle *right = rf_handle(2); const RFHandle *shared = rf_const_handle(1); RFHandle *left = rf_handle(1); RFHandle *old = left; left = right; right = old; RFHandle *spare = left; int32_t a = rf_id(spare); int32_t b = rf_id(left); b += rf_const_id(shared); right = spare; int32_t c = rf_id(right); printf(\"%\" PRId32 \"\\n\", a * 100 + b * 10 + c);",
        stdout: Some("232"),
        trace: "handle=2;const=1;handle=1;id=2;id=2;cid=1;id=2;",
    }, true, false);
}

#[test]
fn raw_scalar_root_field_loans_and_patterns_keep_concrete_values() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{SCALARS}
type Numbers = {{ byte: u8; word: u32; wide: u64; real: f32 }}
type Wrap<a> = Wrap(a)
fn copy(read x) {{ x }}
fn set(inout x, y) {{ x <- y }}
fn forward(inout x: Numbers) {{ set(inout x.byte, u8(255)); set(inout x.word, RF_WORD); set(inout x.wide, RF_WIDE); set(inout x.real, f32(-1)) }}
fn main() {{
    let mut x = record Numbers {{ byte = u8(0); word = u32(0); wide = u64(0); real = f32(0) }};
    let mut root = u64(0); set(inout root, RF_WIDE); forward(inout x);
    let nested = Wrap(Wrap(copy(x.wide)));
    let wide = match nested {{ Wrap(Wrap(value)) => value }};
    copy(x.byte) == RF_BYTE && copy(x.word) == RF_WORD && wide == root && copy(x.real) == f32(-1)
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "uint8_t b = (uint8_t)255; uint32_t w = RF_WORD; uint64_t d = RF_WIDE; float f = (float)-1; uint64_t root = RF_WIDE; puts(b == RF_BYTE && w == RF_WORD && d == root && f == (float)-1 ? \"true\" : \"false\");",
        stdout: Some("true"), trace: "",
    }, true, false);
}

#[test]
fn pointer_argument_snapshots_and_foreign_pointee_aliases_do_not_confuse_slot_loans() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{POINTERS}
extern fn rf_change(value: ptr<RFHandle>, n: i32) -> i32;
extern fn rf_contents(value: ptr<RFHandle>) -> i32;
type Cell = {{ pointer: ptr<RFHandle> }}
fn observe(read slot: ptr<RFHandle>, ignored: i32) {{ rf_contents(slot) }}
fn main() {{
    let mut p = rf_handle(1);
    let a = rf_pair(p, {{ p <- rf_handle(2); rf_mark(3) }});
    let mut cell = record Cell {{ pointer = p }};
    let b = rf_pair(cell.pointer, {{ cell.pointer <- rf_handle(1); rf_mark(4) }});
    let different = p != {{ p <- cell.pointer; p }};
    let alias = p;
    let c = observe(p, rf_change(alias, 9));
    if different {{ a + b + c }} else {{ 0 }}
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "RFHandle *p = rf_handle(1); RFHandle *first = p; p = rf_handle(2); int32_t mark = rf_mark(3); int32_t a = rf_pair(first, mark); RFHandle *cell = p; first = cell; cell = rf_handle(1); mark = rf_mark(4); int32_t b = rf_pair(first, mark); first = p; p = cell; _Bool different = first != p; RFHandle *alias = p; (void)rf_change(alias, 9); int32_t c = rf_contents(p); printf(\"%\" PRId32 \"\\n\", different ? a + b + c : 0);",
        stdout: Some("316"),
        trace: "handle=1;handle=2;mark=3;pair=1:3;handle=1;mark=4;pair=2:4;change=9;contents=9;",
    }, true, false);
}

#[test]
fn scalar_value_and_equality_operands_snapshot_before_later_assignment() {
    let fixture = Fixture::new();
    fixture.compare(Case {
        body: r#"extern fn rf_u64(value: u64) -> u64; extern const RF_WIDE: u64;
type Cell = { n: u64 }
fn first(a, b) { a }
fn main() {
    let mut root = RF_WIDE;
    let a = first(root, { root <- u64(0); rf_u64(root) });
    let mut cell = record Cell { n = RF_WIDE };
    let b = first(cell.n, { cell.n <- u64(0); rf_u64(cell.n) });
    let neq = root != { root <- RF_WIDE; root };
    a == RF_WIDE && b == RF_WIDE && neq && cell.n == u64(0)
}"#,
        driver: "uint64_t root = RF_WIDE; uint64_t a = root; root = 0; (void)rf_u64(root); uint64_t cell = RF_WIDE; uint64_t b = cell; cell = 0; (void)rf_u64(cell); uint64_t before = root; root = RF_WIDE; _Bool neq = before != root; puts(a == RF_WIDE && b == RF_WIDE && neq && cell == 0 ? \"true\" : \"false\");",
        stdout: Some("true"), trace: "u64;u64;",
    }, false, false);
}

#[test]
fn generalized_raw_constant_leaves_capture_once_in_written_order_even_when_unused() {
    let fixture = Fixture::new();
    fixture.compare(Case {
        body: r#"
extern const RF_POINTER: ptr<RFHandle>;
extern const RF_FLOAT: f32;
extern const RF_FLAG: bool;
extern const RF_BYTE: u8;
extern const RF_WIDE: u64;
extern fn rf_set(value: bool) -> unit;
extern fn rf_reads() -> i32;
extern fn rf_id(value: ptr<RFHandle>) -> i32;
type Phantom<a> = Phantom
type Snapshot<a> = { pointer: ptr<RFHandle>; real: f32; flag: bool; byte: u8; wide: u64; marker: Phantom<a> }
type Unused<a> = Unused(ptr<RFHandle>, f32)
type Saved<a> = Saved(ptr<RFHandle>, f32)
fn saved_int(x: Saved<i32>) { match x { Saved(p, f) => rf_id(p) == 1 && f != f32(0) } }
fn saved_bool(x: Saved<bool>) { match x { Saved(p, f) => rf_id(p) == 1 && f != f32(0) } }
fn first(x: Snapshot<ptr<RFHandle>>) { x.flag && rf_id(x.pointer) == 1 && x.real != f32(0) && x.byte == u8(255) }
fn second(x: Snapshot<const_ptr<RFHandle>>) { x.flag && rf_id(x.pointer) == 1 && x.wide == RF_WIDE }
fn main() {
    let p = record Snapshot { real = RF_FLOAT; byte = RF_BYTE; pointer = RF_POINTER; marker = Phantom; wide = RF_WIDE; flag = RF_FLAG };
    let alias = p;
    let saved = Saved(RF_POINTER, RF_FLOAT); let saved_alias = saved;
    let unused = Unused(RF_POINTER, RF_FLOAT);
    rf_set(false);
    if first(alias) && second(p) && saved_int(saved_alias) && saved_bool(saved) { rf_reads() + 35 } else { 0 }
}
"#,
        driver: "float real = RF_FLOAT; uint8_t byte = RF_BYTE; RFHandle *pointer = RF_POINTER; uint64_t wide = RF_WIDE; _Bool flag = RF_FLAG; RFHandle *saved_pointer = RF_POINTER; float saved_real = RF_FLOAT; RFHandle *unused_pointer = RF_POINTER; float unused_real = RF_FLOAT; (void)unused_pointer; (void)unused_real; rf_set(0); _Bool first = flag && rf_id(pointer) == 1 && real != (float)0 && byte == (uint8_t)255; _Bool second = first && flag && rf_id(pointer) == 1 && wide == RF_WIDE; _Bool saved = second && rf_id(saved_pointer) == 1 && saved_real != (float)0 && rf_id(saved_pointer) == 1 && saved_real != (float)0; int32_t n = saved ? rf_reads() + 35 : 0; printf(\"%\" PRId32 \"\\n\", n);",
        stdout: Some("42"),
        trace: "real;pointer;flag;pointer;real;pointer;real;set=0;id=1;id=1;id=1;id=1;reads;",
    }, true, true);
}

#[test]
fn raw_constant_reads_remain_positioned_snapshots_and_shadowed_names_are_locals() {
    let fixture = Fixture::new();
    fixture.compare(Case {
        body: r#"
extern const RF_POINTER: ptr<RFHandle>;
extern const RF_FLOAT: f32;
extern const RF_COMMA_FLOAT: f32;
extern const RF_COMMA_POINTER: ptr<RFHandle>;
extern fn rf_set(value: bool) -> unit;
extern fn rf_reads() -> i32;
extern fn rf_id(value: ptr<RFHandle>) -> i32;
fn main() {
    let p = RF_POINTER; let real = RF_FLOAT; let comma = RF_COMMA_FLOAT; let null = RF_COMMA_POINTER;
    rf_set(false);
    let q = RF_POINTER; let later = RF_FLOAT;
    let RF_POINTER = p; let RF_FLOAT = real;
    if rf_id(RF_POINTER) == 1 && rf_id(q) == 2 && RF_FLOAT != later && comma == f32(2) && is_null(null) { rf_reads() + 36 } else { 0 }
}
"#,
        driver: "RFHandle *p = RF_POINTER; float real = RF_FLOAT; float comma = (RF_COMMA_FLOAT); RFHandle *null = (RF_COMMA_POINTER); rf_set(0); RFHandle *q = RF_POINTER; float later = RF_FLOAT; int32_t n = rf_id(p) == 1 && rf_id(q) == 2 && real != later && comma == (float)2 && null == NULL ? rf_reads() + 36 : 0; printf(\"%\" PRId32 \"\\n\", n);",
        stdout: Some("42"),
        trace: "pointer;real;real;pointer;set=0;pointer;real;id=1;id=2;reads;",
    }, false, false);
}

#[test]
fn conversion_traps_keep_earlier_effects_and_suppress_later_operands() {
    let fixture = Fixture::new();
    let source = fixture.source(
        r#"
extern fn rf_mode() -> i32;
extern fn rf_mark(value: i32) -> i32;
extern fn rf_order(first: i32, byte: u8, last: i32) -> i32;
fn main() {
    let mode = rf_mode();
    if mode == 0 { rf_order(1 / 0, u8(0), rf_mark(2)) }
    else { if mode == 1 { rf_order(rf_mark(1), u8(256), rf_mark(2)) }
    else {
        rf_mark(1);
        if mode == 2 { let bad = u8(-1); () }
        else { if mode == 3 { let bad = u32(-1); () }
        else { if mode == 4 { let bad = u64(-1); () }
        else { if mode == 5 { let bad = u8(1 / 0); () }
        else { if mode == 6 { let bad = u32(2147483647 + 1); () }
        else { if mode == 7 { let bad = f32(1 / 0); () }
        else { let bad = u64((-2147483647 - 1) / -1); () } } } } } };
        rf_mark(2)
    } }
}
"#,
    );
    let (program, typed) = typed_source(&source);
    let reference = rill::backend::emit_reference(&program).unwrap();
    for compiler in compilers(false) {
        for optimization in ["-O0", "-O2"] {
            let typed = fixture.build("traps", &typed, compiler, optimization);
            let reference = fixture.build("reference-traps", &reference, compiler, optimization);
            // One compiled program covers all conversion/argument trap cases.
            for mode in 0..=8 {
                let actual = run(&typed, Some(mode));
                let expected = run(&reference, Some(mode));
                let label = format!("mode {mode}, {compiler} {optimization}");
                same_output(&actual, &expected, &label);
                assert_eq!(actual.status.code(), Some(1), "{label}");
                assert!(actual.stdout.is_empty(), "{label}");
                let trace = if mode == 0 {
                    "rill: runtime trap\n"
                } else {
                    "mark=1;rill: runtime trap\n"
                };
                assert_eq!(actual.stderr, trace.as_bytes(), "{label}");
            }
        }
    }
}

#[test]
fn short_circuit_null_branches_and_repeated_raw_loop_conditions_are_lazy() {
    let fixture = Fixture::new();
    let body = format!(
        r#"{POINTERS}
fn main() {{
    let ignored = false && u8(256) == u8(0);
    let ignored = true || is_null(rf_handle(99));
    let mut index = 0;
    while is_null(rf_handle(index)) {{ index <- index + 1 }};
    let pointer = if true {{ c_null() }} else {{ rf_handle(1 / 0) }};
    if is_null(pointer) {{ index + 41 }} else {{ 0 }}
}}
"#
    );
    fixture.compare(Case {
        body: &body,
        driver: "int32_t index = 0; while (rf_handle(index) == NULL) ++index; RFHandle *p = NULL; printf(\"%\" PRId32 \"\\n\", p == NULL ? index + 41 : 0);",
        stdout: Some("42"), trace: "handle=0;handle=1;",
    }, false, false);
}

#[test]
fn typed_evaluator_matches_reference_reached_raw_operation_error_precedence() {
    const RAW: &str = "E-FFI: raw C values require native C emission";
    const CONSTANT: &str = "E-FFI: imported C constants require native C emission";
    for (source, expected) in [
        ("fn main() { u8(1 / 0) }", Err(RAW)),
        ("fn main() { u32(1 / 0) }", Err(RAW)),
        ("fn main() { u64(1 / 0) }", Err(RAW)),
        ("fn main() { f32(1 / 0) }", Err(RAW)),
        ("extern fn use_pointer(p: ptr<RFHandle>) -> i32; fn main() { use_pointer(c_null()) }", Err(RAW)),
        ("extern fn pointer(n: i32) -> ptr<RFHandle>; fn main() { is_null(pointer(1 / 0)) }", Err(RAW)),
        ("extern fn pointer() -> ptr<RFHandle>; fn main() { pointer() }", Err("E-INTERP-FFI: cannot call C function pointer in the interpreter")),
        ("extern fn pointer(n: i32) -> ptr<RFHandle>; fn main() { pointer(1 / 0) }", Err("runtime trap")),
        ("extern fn f(a: i32, b: u8) -> i32; fn main() { f(1 / 0, u8(0)) }", Err("runtime trap")),
        ("extern fn f(a: u8, b: i32) -> i32; fn main() { f(u8(0), 1 / 0) }", Err(RAW)),
        ("@include(\"typed_raw_fixture.h\"); extern const RF_WIDE: u64; fn main() { RF_WIDE }", Err(CONSTANT)),
        ("@include(\"typed_raw_fixture.h\"); extern const RF_POINTER: ptr<RFHandle>; type M<a> = M(ptr<RFHandle>) fn main() { let unused = M(RF_POINTER); 42 }", Err(CONSTANT)),
        ("extern fn raw() -> ptr<RFHandle>; fn main() { if false { is_null(raw()) } else { true } }", Ok("true")),
        ("fn main() { false && u8(256) == u8(0) }", Ok("false")),
        ("fn main() { true || f32(1 / 0) == f32(0) }", Ok("true")),
        ("extern fn pointer() -> ptr<RFHandle>; fn main() { 42 }", Ok("42")),
        ("@include(\"typed_raw_fixture.h\"); extern const RF_WIDE: u64; fn main() { if false { RF_WIDE == u64(0) } else { true } }", Ok("true")),
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
fn raw_source_rejections_preserve_nominal_types_constness_and_numeric_limits() {
    for source in [
        "fn main() { u8(true) }",
        "fn main() { u32(u8(0)) }",
        "fn main() { u64() }",
        "fn main() { f32(1, 2) }",
        "fn main() { u8(1) + u8(2) }",
        "fn main() { u32(1) < u32(2) }",
        "fn main() { u64(1) * u64(2) }",
        "fn main() { f32(1) <= f32(2) }",
        "fn main() { f32(1) / f32(2) }",
        "fn main() { u8(1) == u32(1) }",
        "fn main() { c_null() }",
        "fn main() { let unused = c_null(); 42 }",
        "fn main() { is_null(0) }",
        "extern fn take(p: ptr<RFHandle>) -> unit; fn main() { take(c_null(1)) }",
        "extern fn take(p: ptr<RFHandle>) -> unit; fn main() { take(0) }",
        "extern fn pointer() -> ptr<RFHandle>; extern fn other() -> ptr<RFOther>; fn main() { pointer() == other() }",
        "extern fn pointer() -> ptr<RFHandle>; extern fn other() -> ptr<RFAlias>; fn main() { pointer() == other() }",
        "extern fn pointer() -> ptr<RFHandle>; extern fn other() -> const_ptr<RFHandle>; fn main() { pointer() == other() }",
        "extern fn pointer() -> ptr<RFHandle>; extern fn take(p: const_ptr<RFHandle>) -> unit; fn main() { take(pointer()) }",
        "extern fn pointer() -> const_ptr<RFHandle>; extern fn take(p: ptr<RFHandle>) -> unit; fn main() { take(pointer()) }",
        "extern fn pointer() -> ptr<RFHandle>; fn main() { pointer() < pointer() }",
        "extern fn pointer() -> ptr<RFHandle>; fn main() { pointer() + 1 }",
        "extern fn a(p: ptr<RFHandle>) -> unit; extern fn b(p: ptr<RFOther>) -> unit; fn main() { let p = c_null(); a(p); b(p) }",
        "extern fn pointer() -> ptr<RFHandle>; extern fn take(p: ptr<RFOther>) -> unit; fn main() { let mut p = c_null(); take(p); p <- pointer() }",
    ] {
        let program = rill::parser::parse(source).unwrap_or_else(|e| panic!("invalid rejection fixture {source}: {e}"));
        assert!(rill::check::analyze(&program).is_err(), "accepted {source}");
        assert!(rill::backend::emit(&program).is_err(), "emitted {source}");
        assert!(rill::backend::emit_reference(&program).is_err(), "reference emitted {source}");
        assert!(rill::interpreter::run(&program).is_err(), "interpreted {source}");
    }
}

#[test]
fn nested_native_arguments_do_not_bypass_live_raw_slot_loans() {
    for body in [
        "exclusive(inout p.left, rf_id(p.left))",
        "shared(p.left, rf_id({ p.left <- p.right; p.left }))",
        "exclusive(inout p.left, rf_id({ let copy = p; copy.left }))",
        "exclusive(inout p.left, rf_id({ p <- record Pair { left = c_null(); right = c_null() }; p.right }))",
    ] {
        let source = format!(r#"{POINTERS}
type Pair = {{ left: ptr<RFHandle>; right: ptr<RFHandle> }}
fn exclusive(inout x: ptr<RFHandle>, n: i32) {{ n }}
fn shared(read x: ptr<RFHandle>, n: i32) {{ n }}
fn main() {{ let mut p = record Pair {{ left = rf_handle(1); right = rf_handle(2) }}; {body} }}
"#);
        let program = rill::parser::parse(&source).unwrap();
        assert!(rill::check::analyze(&program).is_err(), "accepted {source}");
        assert!(rill::backend::emit(&program).is_err(), "emitted {source}");
    }
    for source in [
        "extern fn pointer() -> ptr<RFHandle>; fn assign(inout x: ptr<RFHandle>) { x <- c_null() } fn forward(read x: ptr<RFHandle>) { assign(inout x) } fn main() { let p = pointer(); forward(p) }",
        "extern fn pointer() -> ptr<RFHandle>; fn assign(inout x: ptr<RFHandle>) { x <- c_null() } fn main() { let p = pointer(); assign(inout p) }",
    ] {
        assert!(rill::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn raw_values_remain_excluded_from_owner_programs_even_in_dead_or_generic_shapes() {
    for source in [
        "fn main() { let owner = system_box_i32_abort(42); let raw = u8(1); box_read(owner) }",
        "fn main() { let owner = system_box_i32_abort(42); if false { let raw = f32(0); 0 } else { box_read(owner) } }",
        "type R = { raw: u64 } fn main() { let owner = system_box_i32_abort(42); let r = record R { raw = u64(1) }; box_read(owner) }",
        "extern fn pointer() -> ptr<RFHandle>; fn id(x) { x } fn main() { let owner = system_box_i32_abort(42); let p = id(pointer()); box_read(owner) }",
        "@include(\"typed_raw_fixture.h\"); extern const RF_POINTER: ptr<RFHandle>; type Mark<a> = Mark(ptr<RFHandle>) fn main() { let owner = system_box_i32_abort(42); let unused = Mark(RF_POINTER); box_read(owner) }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        if let Ok(checked) = rill::check::analyze(&program) {
            assert!(rill::typed::lower_main(&checked).is_err(), "lowered {source}");
        }
        assert!(rill::backend::emit(&program).is_err(), "emitted {source}");
        assert!(rill::interpreter::run(&program).is_err(), "interpreted {source}");
    }
}

#[test]
fn unused_raw_header_mismatches_still_fail_cli_check_and_emit_c() {
    let fixture = Fixture::new();
    for (declaration, diagnostic) in [
        (
            "extern fn rf_u8(value: u32) -> u8;",
            "incompatible header prototype for rf_u8",
        ),
        (
            "extern fn rf_f32(value: i32) -> f32;",
            "incompatible header prototype for rf_f32",
        ),
        (
            "extern fn rf_handle(id: i32) -> const_ptr<RFHandle>;",
            "incompatible header prototype for rf_handle",
        ),
        (
            "extern fn rf_const_id(value: ptr<RFHandle>) -> i32;",
            "incompatible header prototype for rf_const_id",
        ),
        (
            "extern fn rf_handle(id: i32) -> ptr<RFOther>;",
            "incompatible header prototype for rf_handle",
        ),
        (
            "extern const RF_WIDE: u32;",
            "incompatible header constant RF_WIDE",
        ),
        (
            "extern const RF_CONST_NULL: ptr<RFHandle>;",
            "incompatible header constant RF_CONST_NULL",
        ),
        (
            "extern const RF_FLOAT: u32;",
            "incompatible header constant RF_FLOAT",
        ),
    ] {
        let source = fixture.source(&format!("{declaration} fn main() {{ 42 }}"));
        typed_source(&source);
        let path = fixture.directory.join("invalid.rill");
        fs::write(&path, source).unwrap();
        for operation in ["check", "emit-c"] {
            let output = Command::new(env!("CARGO_BIN_EXE_rill"))
                .arg(operation)
                .arg(&path)
                .args(["--cc", "cc", "--c-arg", "-I", "--c-arg"])
                .arg(&fixture.directory)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{operation}: {declaration}");
            assert!(output.stdout.is_empty(), "{operation}: {declaration}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(diagnostic),
                "{operation}: {declaration}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
