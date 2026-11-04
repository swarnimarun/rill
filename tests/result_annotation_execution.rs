//! Explicit annotation execution matrix. All new fixtures and diagnostics are retained.
use rill::{backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
const O: &str = "type O<a> = None|Some(a)\n";
struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-annotation-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("main.rill"), source).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    fn output(&self, command: &mut Command, label: &str, seconds: u64) -> Output {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let out = self.0.join(format!("{serial}-{label}.out"));
        let err = self.0.join(format!("{serial}-{label}.err"));
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&out).unwrap())
            .stderr(fs::File::create(&err).unwrap());
        let mut child = command.spawn().unwrap_or_else(|e| panic!("{label}: {e}"));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{label}: subprocess timeout is not a language trap");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Output {
            status,
            stdout: fs::read(out).unwrap(),
            stderr: fs::read(err).unwrap(),
        }
    }
    fn guard(&self) {
        if let Some(guard) = std::env::var_os("K4_RESOURCE_GUARD") {
            let out = self.output(Command::new("bash").arg(guard), "resource-guard", 5);
            assert!(
                out.status.success(),
                "resource guard: {} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    fn run(
        &self,
        label: &str,
        c: &str,
        expected: Option<&str>,
        events: &str,
        extra: &[PathBuf],
        definitions: &[&str],
    ) {
        let source = self.write(&format!("{label}.c"), c);
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        for opt in ["-O0", "-O2"] {
            let binary = self.0.join(format!("{label}-{}", &opt[1..]));
            self.guard();
            let built = self.output(
                Command::new(&compiler)
                    .args(["-std=c11", "-pedantic-errors", opt, "-I"])
                    .arg(&self.0)
                    .args(definitions)
                    .arg(&source)
                    .args(extra)
                    .arg("-o")
                    .arg(&binary),
                "compile",
                30,
            );
            assert!(
                built.status.success(),
                "{label} {opt}: {}",
                String::from_utf8_lossy(&built.stderr)
            );
            if !built.stderr.is_empty() {
                eprintln!("{label} {opt}: {}", String::from_utf8_lossy(&built.stderr));
            }
            let output = self.output(&mut Command::new(&binary), "execute", 10);
            assert_eq!(
                output.status.code(),
                Some(if expected.is_some() { 0 } else { 1 }),
                "{label} {opt}"
            );
            let stdout = expected.map(|s| format!("{s}\n")).unwrap_or_default();
            let stderr = format!(
                "{events}{}",
                if expected.is_none() {
                    "rill: runtime trap\n"
                } else {
                    ""
                }
            );
            assert_eq!(output.stdout, stdout.as_bytes(), "{label} {opt}");
            assert_eq!(output.stderr, stderr.as_bytes(), "{label} {opt}");
            eprintln!(
                "ANNOTATION_C_PASS {} {label} {opt}",
                compiler.to_string_lossy()
            );
        }
    }
}
fn lower(source: &str) -> (rill::ast::Program, typed::TypedMain, String) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{e}\n{source}"));
    typed::verify(&lowered).unwrap();
    let c = typed::emit_c(&lowered).unwrap();
    assert_eq!(backend::emit(&program).unwrap(), c);
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
    (program, lowered, c)
}
fn pure(label: &str, source: &str, expected: Option<&str>, reference_c: bool) -> String {
    let fixture = Fixture::new(label, source);
    let (program, lowered, c) = lower(source);
    let expected_result = expected
        .map(str::to_owned)
        .ok_or_else(|| "runtime trap".to_owned());
    for actual in [
        typed::evaluate(&lowered),
        interpreter::run(&program),
        interpreter::run_reference(&program),
    ] {
        assert_eq!(actual.map(|v| v.to_string()), expected_result, "{source}");
    }
    fixture.run("typed", &c, expected, "", &[], &[]);
    let reference = backend::emit_reference(&program);
    if reference_c {
        fixture.run("reference", &reference.unwrap(), expected, "", &[], &[]);
    } else {
        fixture.write(
            "reference-classification.txt",
            &match reference {
                Ok(_) => "supported, not executed in this contextual case".into(),
                Err(e) => e,
            },
        );
    }
    c
}
fn frames(c: &str) -> String {
    let enter =
        "static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }";
    let leave = "static void rl_leave(void) { --rl_call_depth; }";
    assert_eq!(c.matches(enter).count(), 1);
    assert_eq!(c.matches(leave).count(), 1);
    c.replace(enter, "static void rl_enter(void) { fprintf(stderr, \"E%u;\", rl_call_depth+1); if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }")
        .replace(leave, "static void rl_leave(void) { fprintf(stderr, \"L%u;\", rl_call_depth); --rl_call_depth; }")
}
#[test]
fn closed_pure_results_execute_on_both_c_backends() {
    let cases = [
        ("absent", format!("{O}fn absent()->O<i32>{{None}} fn main()->O<i32>{{absent()}}"), "None"),
        ("observe", format!("{O}fn absent()->O<i32>{{None}} fn main()->i32{{match absent(){{None=>42|Some(x)=>x}}}}"), "42"),
        ("mutable", format!("{O}fn absent()->O<i32>{{None}} fn main()->i32{{let mut x=absent();x<-Some(42);match x{{None=>0|Some(n)=>n}}}}"), "42"),
        ("nested", format!("{O}type I={{v:O<i32>}} type P={{inner:I;flag:bool}} fn make()->P{{record P{{flag=true;inner=record I{{v=Some(42)}}}}}} fn main()->i32{{let p=make();match p.inner.v{{None=>0|Some(n)=>n}}}}"), "42"),
        ("static-text", "fn text()->str{\"héllo\"} fn main()->i32{byte_len(text())}".into(), "6"),
        ("unit", "fn work()->unit{42;} fn main()->unit{work()}".into(), "()"),
        ("boolean", "fn yes()->bool{true} fn main()->bool{yes()}".into(), "true"),
        ("vector", "fn lanes()->u8x16{u8x16_load(bytes_new(16),0)} fn main()->i32{u8x16_lane(lanes(),15)}".into(), "0"),
        ("vector-enum", "type V=Pack(u8x16) fn lanes()->V{Pack(u8x16_load(bytes_new(16),0))} fn main()->i32{match lanes(){Pack(v)=>u8x16_lane(v,0)}}".into(), "0"),
        ("generic", "fn count(x)->i32{21} fn main()->i32{count(true)+count(0)}".into(), "42"),
        ("identity", "fn identity(x)->i32{x} fn main()->i32{identity(42)}".into(), "42"),
        ("lowercase-nominal", "type a=Lower fn make()->a{Lower} fn main()->i32{match make(){Lower=>42}}".into(), "42"),
        ("field-loans", "type P={x:i32;y:i32} fn make()->P{record P{x=20;y=21}} fn bump(inout n)->unit{n<-n+1} fn inspect(read p:P)->i32{p.x+p.y} fn main()->i32{let mut p=make();bump(inout p.x);inspect(p)}".into(), "42"),
        ("written-order", "type P={a:i32;b:i32} fn mark(inout n,d)->i32{n<-n*10+d;n} fn make(inout n)->P{record P{b=mark(inout n,2);a=mark(inout n,1)}} fn main()->i32{let mut n=0;let p=make(inout n);n}".into(), "21"),
        ("three-scc", "type P={x:i32} fn f(n)->P{g(n)} fn g(n){h(n)} fn h(n)->P{if n==0{record P{x=42}}else{f(n-1)}} fn main()->i32{let p=g(2);p.x}".into(), "42"),
        ("dead-trap", "fn f()->i32{if true{42}else{1/0}} fn main()->i32{f()}".into(), "42"),
    ];
    for (label, source, value) in cases {
        pure(label, &source, Some(value), true);
    }
}
#[test]
fn contextual_results_keep_exact_closed_types_and_existing_control() {
    for (label, source) in [
        ("context-marker", format!("{O}type Marker<a> =Mark fn absent(){{None}} fn mark()->Marker<i32>{{Mark}} fn main()->i32{{let ignored=absent();let observed=mark();match observed{{Mark=>42}}}}")),
        ("context-record", format!("{O}type R<a> ={{hole:O<a>;n:i32}} type P={{x:i32}} fn discard(){{record R{{n=1;hole=None}}}} fn point()->P{{record P{{x=42}}}} fn main()->i32{{let ignored=discard();let p=point();p.x}}")),
    ] { pure(label, &source, Some("42"), false); }
    pure(
        "interrupted",
        "fn spin(){spin()} fn sink(a,b)->i32{b} fn main()->i32{sink(spin(),42)}",
        None,
        false,
    );
}
#[test]
fn annotations_preserve_exact_return_discard_and_depth_events() {
    let source = format!(
        "{O}fn step(inout n)->O<i32>{{n<-n+1;None}} fn main()->i32{{let mut n=41;step(inout n);n}}"
    );
    let c = pure("discard-return", &source, Some("42"), true);
    Fixture::new("discard-events", &source).run(
        "frames",
        &frames(&c),
        Some("42"),
        "E1;E2;L2;L1;",
        &[],
        &[],
    );
    let source = "fn spin()->i32{spin()} fn main()->i32{spin()}";
    let c = pure("annotated-spin", source, None, true);
    let events: String = (1..=33).map(|depth| format!("E{depth};")).collect();
    Fixture::new("spin-events", source).run("frames", &frames(&c), None, &events, &[], &[]);
    let source = "fn explode()->i32{1/0} fn main()->i32{explode();42}";
    let c = pure("discard-trap", source, None, true);
    Fixture::new("trap-events", source).run("frames", &frames(&c), None, "E1;E2;", &[], &[]);
}

const NATIVE_HEADER: &str = r#"
#ifndef ANNOTATION_FIXTURE_H
#define ANNOTATION_FIXTURE_H
#include <stdint.h>
typedef struct AnnHandle AnnHandle;
int32_t ann_step(int32_t);
uint8_t ann_u8(uint8_t);
uint32_t ann_u32(uint32_t);
uint64_t ann_u64(uint64_t);
float ann_f32(float);
AnnHandle *ann_handle(int32_t);
const AnnHandle *ann_const_handle(int32_t);
int32_t ann_id(AnnHandle *);
int32_t ann_const_id(const AnnHandle *);
void ann_tick(void);
#endif
"#;
const NATIVE_IMPLEMENTATION: &str = r#"
#include "annotation_fixture.h"
#include <stdio.h>
struct AnnHandle { int32_t id; };
int32_t ann_step(int32_t n) { fprintf(stderr,"S%d;",(int)n); return n; }
uint8_t ann_u8(uint8_t n) { fputs("U8;",stderr); return n; }
uint32_t ann_u32(uint32_t n) { fputs("U32;",stderr); return n; }
uint64_t ann_u64(uint64_t n) { fputs("U64;",stderr); return n; }
float ann_f32(float n) { fputs("F32;",stderr); return n; }
AnnHandle *ann_handle(int32_t n) { static AnnHandle h; h.id=n; fprintf(stderr,"H%d;",(int)n); return &h; }
const AnnHandle *ann_const_handle(int32_t n) { static AnnHandle h; h.id=n; fprintf(stderr,"CH%d;",(int)n); return &h; }
int32_t ann_id(AnnHandle *h) { fprintf(stderr,"ID%d;",(int)h->id); return h->id; }
int32_t ann_const_id(const AnnHandle *h) { fprintf(stderr,"CID%d;",(int)h->id); return h->id; }
void ann_tick(void) { fputs("T;",stderr); }
"#;
#[test]
fn native_result_annotations_preserve_exact_abi_and_call_events() {
    let cases = [
        ("native-i32", "extern fn ann_step(n:i32)->i32; fn wrap()->i32{ann_step(42)} fn main()->i32{wrap()}", "42", "S42;"),
        ("native-u8", "extern fn ann_u8(n:u8)->u8; fn wrap()->u8{ann_u8(u8(7))} fn main()->bool{wrap()==u8(7)}", "true", "U8;"),
        ("native-u32", "extern fn ann_u32(n:u32)->u32; fn wrap()->u32{ann_u32(u32(7))} fn main()->bool{wrap()==u32(7)}", "true", "U32;"),
        ("native-u64", "extern fn ann_u64(n:u64)->u64; fn wrap()->u64{ann_u64(u64(7))} fn main()->bool{wrap()==u64(7)}", "true", "U64;"),
        ("native-f32", "extern fn ann_f32(n:f32)->f32; fn wrap()->f32{ann_f32(f32(7))} fn main()->bool{wrap()==f32(7)}", "true", "F32;"),
        ("native-pointer", "extern fn ann_handle(n:i32)->ptr<AnnHandle>; extern fn ann_id(p:ptr<AnnHandle>)->i32; fn wrap()->ptr<AnnHandle>{ann_handle(42)} fn main()->i32{ann_id(wrap())}", "42", "H42;ID42;"),
        ("native-const-pointer", "extern fn ann_const_handle(n:i32)->const_ptr<AnnHandle>; extern fn ann_const_id(p:const_ptr<AnnHandle>)->i32; fn wrap()->const_ptr<AnnHandle>{ann_const_handle(42)} fn main()->i32{ann_const_id(wrap())}", "42", "CH42;CID42;"),
        ("native-null", "fn wrap()->const_ptr<AnnHandle>{c_null()} fn main()->bool{is_null(wrap())}", "true", ""),
        ("native-unit", "extern fn ann_tick()->unit; fn wrap()->unit{ann_tick()} fn main()->i32{wrap();42}", "42", "T;"),
    ];
    for (label, body, expected, events) in cases {
        let source = format!("@include(\"annotation_fixture.h\");\n{body}");
        let fixture = Fixture::new(label, &source);
        fixture.write("annotation_fixture.h", NATIVE_HEADER);
        let support = fixture.write("fixture.c", NATIVE_IMPLEMENTATION);
        let (program, lowered, c) = lower(&source);
        for error in [
            typed::evaluate(&lowered).unwrap_err(),
            interpreter::run(&program).unwrap_err(),
            interpreter::run_reference(&program).unwrap_err(),
        ] {
            assert!(
                error.starts_with("E-INTERP-FFI:") || error.starts_with("E-FFI:"),
                "{error}"
            );
        }
        assert!(!c.contains("restrict"));
        fixture.run(
            "typed",
            &c,
            Some(expected),
            events,
            std::slice::from_ref(&support),
            &[],
        );
        fixture.run(
            "reference",
            &backend::emit_reference(&program).unwrap(),
            Some(expected),
            events,
            &[support],
            &[],
        );
    }
}

const OWNER_SUPPORT: &str = r#"
#include <stdio.h>
#include <stdlib.h>
static void *live[16];
static int sequence;
void *annotation_malloc(size_t size) {
    void *p=malloc(size); if (!p) return NULL;
    int n=++sequence; if(n>=16) abort(); live[n]=p;
    fprintf(stderr,"A%d;",n); return p;
}
void annotation_free(void *p) {
    int n=1; while(n<=sequence && live[n]!=p) ++n;
    if(n>sequence) abort(); fprintf(stderr,"F%d;",n);
    live[n]=NULL; free(p);
}
"#;
#[test]
fn annotated_existing_owner_returns_keep_moves_cleanup_and_abort_policy() {
    let cases = [
        ("owner-box", "fn make(n)->SystemBoxI32{let out=system_box_i32_abort(n);let scratch=system_box_i32_abort(7);move out} fn main()->i32{let out=make(42);box_read(out)}", Some("42"), "A1;A2;F2;F1;"),
        ("owner-identity", "fn pass(owner)->SystemBoxI32{move owner} fn main()->i32{let owner=system_box_i32_abort(42);let returned=pass(move owner);box_read(returned)}", Some("42"), "A1;F1;"),
        ("owner-bytes", "fn make(n)->SystemBytes{let out=system_bytes_zeroed_abort(n);move out} fn relay(n)->SystemBytes{make(n)} fn main()->i32{let mut out=relay(2);owned_bytes_set(inout out,1,42);owned_bytes_get(out,1)}", Some("42"), "A1;F1;"),
        ("owner-enum", "type O<a> =None|Some(a) fn make()->O<SystemBoxI32>{let b=system_box_i32_abort(42);Some(move b)} fn main()->i32{let out=make();match move out{None=>0|Some(b)=>box_read(b)}}", Some("42"), "A1;F1;"),
        ("owner-inactive", "type O<a> =None|Some(a) fn absent()->O<SystemBoxI32>{None} fn main()->i32{let out=absent();match move out{None=>42|Some(b)=>box_read(b)}}", Some("42"), ""),
        ("owner-trap", "fn make()->SystemBoxI32{let out=system_box_i32_abort(42);move out} fn main()->i32{let out=make();1/0}", None, "A1;"),
    ];
    for (label, source, expected, events) in cases {
        let fixture = Fixture::new(label, source);
        let support = fixture.write("owner_fixture.c", OWNER_SUPPORT);
        let (program, lowered, c) = lower(source);
        let expected_result = expected
            .map(str::to_owned)
            .ok_or_else(|| "runtime trap".to_owned());
        assert_eq!(
            typed::evaluate(&lowered).map(|v| v.to_string()),
            expected_result
        );
        assert_eq!(
            interpreter::run(&program).map(|v| v.to_string()),
            expected_result
        );
        assert!(interpreter::run_reference(&program)
            .unwrap_err()
            .starts_with("E-REFERENCE"));
        assert!(backend::emit_reference(&program)
            .unwrap_err()
            .starts_with("E-REFERENCE"));
        assert_eq!(c.matches("= malloc;").count(), 1);
        assert_eq!(c.matches("= free;").count(), 1);
        let instrumented = format!("#include <stddef.h>\nvoid *annotation_malloc(size_t);\nvoid annotation_free(void *);\n{}", c.replace("= malloc;", "= annotation_malloc;").replace("= free;", "= annotation_free;"));
        fixture.run("owned", &instrumented, expected, events, &[support], &[]);
    }
}

#[test]
fn imported_closed_results_execute_with_resolved_nominal_identity() {
    let source = "import api; fn absent()->api.O<api.P>{api.absent()} fn main()->i32{match absent(){api.None=>42|api.Some(p)=>p.x}}";
    let fixture = Fixture::new("module", source);
    fixture.write(
        "api.rill",
        "export type O<a> = None|Some(a) export type P={x:i32} export fn absent()->O<P>{None}",
    );
    let program = rill::modules::compile::load_program(&fixture.0.join("main.rill")).unwrap();
    let checked = check::analyze(&program).unwrap();
    let ir = typed::lower_main(&checked).unwrap();
    assert_eq!(typed::evaluate(&ir).unwrap().to_string(), "42");
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        "42"
    );
    let c = typed::emit_c(&ir).unwrap();
    assert_eq!(backend::emit(&program).unwrap(), c);
    fixture.run("typed", &c, Some("42"), "", &[], &[]);
    fixture.run(
        "reference",
        &backend::emit_reference(&program).unwrap(),
        Some("42"),
        "",
        &[],
        &[],
    );
}
