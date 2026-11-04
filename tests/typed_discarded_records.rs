//! Author-focused direct discarded-record parity and retained boundaries.
//! Prepared source-only; execution evidence belongs to the verification record.
use rill::{
    check, interpreter, parser, typed,
    typed::provenance::{
        lower_main_with_function_origins, nodes::lower_main_with_copy_node_origins,
    },
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const O: &str = "type O<a> = None | Some(a)\n";
const R: &str = "type R<a> = { left: i32; hole: O<a>; right: i32 }\n";
const MARK: &str = "fn mark(inout n,d){n<-n*10+d;n}\n";
const RECORD: &str = "record R{right=mark(inout n,1);hole=None;left=mark(inout n,2)}";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-discarded-records-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn output(&self, command: &mut Command, seconds: u64, context: &str) -> Output {
        // Files avoid pipe-buffer deadlocks while polling a bounded subprocess.
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.0.join(format!("stdout-{serial}"));
        let stderr = self.0.join(format!("stderr-{serial}"));
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{context}: {error}"));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{context}: timed out, not an accepted runtime trap");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }

    fn run_c(&self, c: &str, expected: Option<&str>, trace: &str, source: &str) {
        let path = self.write("main.c", c);
        self.write("main.rill", source);
        let binary = self.0.join("main");
        // Run this suite under CC=gcc and CC=clang for cross-compiler evidence.
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        for optimization in ["-O0", "-O2"] {
            let context = format!("{} {optimization}\n{source}", compiler.to_string_lossy());
            if let Some(guard) = std::env::var_os("K4_RESOURCE_GUARD") {
                let guarded = self.output(
                    Command::new("bash").arg(guard),
                    5,
                    &format!("resource guard before {context}"),
                );
                assert!(
                    guarded.status.success(),
                    "{context}: resource guard failed\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&guarded.stdout),
                    String::from_utf8_lossy(&guarded.stderr)
                );
            }
            let build = self.output(
                Command::new(&compiler)
                    .args(["-std=c11", "-pedantic-errors", optimization])
                    .arg(&path)
                    .arg("-o")
                    .arg(&binary),
                30,
                &context,
            );
            assert!(
                build.status.success(),
                "{context}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            assert!(build.stdout.is_empty(), "{context}");
            if !build.stderr.is_empty() {
                eprintln!(
                    "{context}: C diagnostic: {}",
                    String::from_utf8_lossy(&build.stderr)
                );
            }
            let output = self.output(&mut Command::new(&binary), 10, &context);
            match expected {
                Some(value) => {
                    assert_eq!(output.status.code(), Some(0), "{context}");
                    assert_eq!(output.stdout, format!("{value}\n").as_bytes(), "{context}");
                    assert_eq!(output.stderr, trace.as_bytes(), "{context}");
                }
                None => {
                    // Signals and host-stack failure are not the language trap.
                    assert_eq!(output.status.code(), Some(1), "{context}");
                    assert!(output.stdout.is_empty(), "{context}");
                    assert_eq!(
                        output.stderr,
                        format!("{trace}rill: runtime trap\n").as_bytes(),
                        "{context}"
                    );
                }
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's fresh temporary fixture is removed.
        if std::env::var_os("RILL_RETAIN_TEST_ARTIFACTS").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn lower(source: &str) -> (rill::ast::Program, typed::TypedMain, String) {
    let program = parser::parse(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let snapshot = program.clone();
    let checked = check::analyze(&program).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let lowered = typed::lower_main(&checked).unwrap_or_else(|error| panic!("{error}\n{source}"));
    typed::verify(&lowered).unwrap();
    let c = typed::emit_c(&lowered).unwrap();
    assert_eq!(rill::backend::emit(&program).unwrap(), c, "{source}");
    assert_eq!(
        typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
        c,
        "repeated lowering changed emitted C: {source}"
    );
    assert_eq!(program, snapshot, "lowering mutated checked source");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert!(!c.contains("__builtin_unreachable") && !c.contains("_Noreturn static void rl_fn_"));
    for line in c
        .lines()
        .filter(|line| line.starts_with("static void rl_fn_"))
    {
        let name = line
            .strip_prefix("static void ")
            .unwrap()
            .split('(')
            .next()
            .unwrap();
        assert!(
            !c.contains(&format!("= {name}(")),
            "void call used as a value: {name}"
        );
    }
    (program, lowered, c)
}

fn compare(source: &str, expected: Option<&str>) -> String {
    let (program, lowered, c) = lower(source);
    let actual = typed::evaluate(&lowered);
    match expected {
        Some(value) => assert_eq!(actual.as_ref().unwrap().to_string(), value, "{source}"),
        None => assert_eq!(actual.as_ref().unwrap_err(), "runtime trap", "{source}"),
    }
    assert_eq!(interpreter::run(&program), actual, "{source}");
    assert_eq!(interpreter::run_reference(&program), actual, "{source}");
    let fixture = Fixture::new();
    // Uniform-C layout classification is separate from explicit typed support.
    let uniform = match rill::backend::emit_reference(&program) {
        Ok(_) => "SUPPORTED\n".to_string(),
        Err(error) => format!("REJECTED: {error}\n"),
    };
    fixture.write("uniform-classification.txt", &uniform);
    eprintln!("discarded record uniform C: {uniform}{source}");
    fixture.run_c(&c, expected, "", source);
    c
}

fn unsupported(source: &str) {
    let program = parser::parse(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let checked = check::analyze(&program).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let error = typed::lower_main(&checked).unwrap_err();
    assert!(
        error.starts_with("E-TYPED-UNSUPPORTED:"),
        "{error}\n{source}"
    );
}

fn source_error(source: &str) {
    let program = parser::parse(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let error = check::analyze(&program).err().unwrap();
    assert_eq!(interpreter::run(&program).unwrap_err(), error, "{source}");
    assert_eq!(
        rill::backend::emit(&program).unwrap_err(),
        error,
        "{source}"
    );
}

fn trace_frames(c: String) -> String {
    let enter =
        "static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }";
    let leave = "static void rl_leave(void) { --rl_call_depth; }";
    assert_eq!(c.matches(enter).count(), 1);
    assert_eq!(c.matches(leave).count(), 1);
    c.replace(enter, "static void rl_enter(void) { fprintf(stderr, \"E%u;\", rl_call_depth + 1); if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }")
        .replace(leave, "static void rl_leave(void) { fprintf(stderr, \"L%u;\", rl_call_depth); --rl_call_depth; }")
}

fn trace_mark(c: String) -> String {
    // These fixtures declare mark first, with one inout and one value parameter.
    let signature = c
        .lines()
        .find(|line| line.starts_with("static void rl_fn_0(") && line.ends_with(" {"))
        .unwrap();
    let parameter = signature
        .rsplit_once("rl_l_")
        .unwrap()
        .1
        .split(')')
        .next()
        .unwrap();
    let needle = format!("{signature}\nrl_enter();\n");
    assert_eq!(c.matches(&needle).count(), 1);
    let replacement = format!("{needle}fprintf(stderr, \"A%d;\", (int)rl_l_{parameter});\n");
    c.replacen(&needle, &replacement, 1)
}

#[test]
fn direct_records_and_forwarding_return_without_aggregate_storage() {
    for (extra, main, expected) in [
        ("", "let weak=make(inout n);n", "12"),
        ("", "make(inout n);n", "12"),
        ("", "make(inout n);", "()"),
        (
            "fn relay(inout n){make(inout n)}",
            "let weak=relay(inout n);n",
            "12",
        ),
        ("", "let weak=make(inout n);let weak=42;weak", "42"),
        (
            "",
            "let first=make(inout n);let second=make(inout n);n",
            "1212",
        ),
    ] {
        let source = format!(
            "{O}{R}{MARK}fn make(inout n){{{RECORD}}} {extra} fn main(){{let mut n=0;{main}}}"
        );
        let c = compare(&source, Some(expected));
        assert!(!c.contains("rl_adt_"), "discarded R/O acquired storage");
        assert!(c.contains("rl_leave(); return;"));
    }
}

#[test]
fn nested_record_and_enum_fields_substitute_complete_nominal_arguments() {
    for source in [
        format!("{O}{R}type W<a> = W(R<a>) {MARK}fn make(inout n){{W({RECORD})}} fn main(){{let mut n=0;let weak=make(inout n);n}}"),
        format!("{O}type Inner<a> = {{hole:O<a>;tag:i32}} type Outer<a> = {{nested:Inner<O<a>>;tail:i32}} {MARK}fn make(inout n){{record Outer{{tail=mark(inout n,1);nested=record Inner{{tag=mark(inout n,2);hole=Some(None)}}}}}} fn main(){{let mut n=0;let weak=make(inout n);n}}"),
        format!("type Outer<a> = {{nested:Inner<O<a>>;tail:i32}} type Inner<a> = {{hole:O<a>;tag:i32}} {O}{MARK}fn make(inout n){{record Outer{{tail=mark(inout n,1);nested=record Inner{{tag=mark(inout n,2);hole=Some(None)}}}}}} fn main(){{let mut n=0;let weak=make(inout n);n}}"),
    ] {
        let c = compare(&source, Some("12"));
        assert!(!c.contains("rl_adt_"), "nested discarded aggregate acquired storage");
    }
}

#[test]
fn empty_and_all_returning_record_operands_complete_normally() {
    for source in [
        format!("{O}type Empty = {{}} type Nest<a> = {{hole:O<a>;empty:Empty;last:i32}} fn make(){{record Nest{{empty=record Empty{{}};hole=None;last=42}}}} fn main(){{let weak=make();42}}"),
        format!("{O}type Store<a> = {{value:O<a>}} fn make(){{record Store{{value=None}}}} fn main(){{let unused=make();42}}"),
        format!("{O}{R}fn make(){{record R{{right=1;hole=Some(None);left=2}}}} fn main(){{let unused=make();42}}"),
    ] {
        compare(&source, Some("42"));
    }
}

#[test]
fn record_branches_and_matches_execute_only_selected_written_fields() {
    for (condition, expected) in [("true", "12"), ("false", "34")] {
        let source = format!("{O}{R}{MARK}fn make(inout n,choose){{if choose{{{RECORD}}}else{{record R{{left=mark(inout n,3);hole=None;right=mark(inout n,4)}}}}}} fn main(){{let mut n=0;let weak=make(inout n,{condition});n}}");
        compare(&source, Some(expected));
    }
    for (argument, expected) in [(0, "12"), (1, "34")] {
        let source = format!("{O}{R}{MARK}fn make(inout n,choose){{match choose{{0=>{RECORD}|_=>record R{{left=mark(inout n,3);hole=None;right=mark(inout n,4)}}}}}} fn main(){{let mut n=0;let weak=make(inout n,{argument});n}}");
        compare(&source, Some(expected));
    }
    for body in [
        format!("if choose{{{RECORD}}}else{{record R{{right=1/0;hole=None;left=mark(inout n,9)}}}}"),
        format!("match choose{{true=>{RECORD}|false=>record R{{right=1/0;hole=None;left=mark(inout n,9)}}}}"),
    ] {
        let source = format!("{O}{R}{MARK}fn make(inout n,choose){{{body}}} fn main(){{let mut n=0;let weak=make(inout n,true);n}}");
        compare(&source, Some("12"));
    }
}

#[test]
fn body_only_record_metadata_and_dead_prefixes_remain_verified() {
    for source in [
        format!("{O}{R}fn answer(n){{let weak=if n==0{{record R{{right=1;hole=None;left=2}}}}else{{record R{{right=1/0;hole=None;left=2}}}};n+42}} fn main(){{answer(0)}}"),
        format!("{O}{R}fn main(){{if false{{let weak=if true{{record R{{right=1/0;hole=None;left=2}}}}else{{record R{{right=3;hole=None;left=4}}}};0}}else{{42}}}}"),
        format!("{O}{R}fn answer(){{let weak=match true{{true=>record R{{right=1;hole=None;left=2}}|false=>record R{{right=1/0;hole=None;left=2}}}};42}} fn main(){{answer()}}"),
    ] {
        compare(&source, Some("42"));
    }
}

#[test]
fn represented_roots_fields_siblings_and_read_snapshots_keep_their_loans() {
    for (source, expected) in [
        (format!("{O}{R}type P = {{n:i32}} {MARK}fn make(inout n){{{RECORD}}} fn relay(inout n){{make(inout n)}} fn main(){{let mut p=record P{{n=0}};let weak=relay(inout p.n);p.n}}"), "12"),
        (format!("{O}{R}type P = {{n:i32}} {MARK}fn make(inout p:P){{record R{{right=mark(inout p.n,1);hole=None;left=mark(inout p.n,2)}}}} fn main(){{let mut p=record P{{n=0}};let weak=make(inout p);p.n}}"), "12"),
        (format!("{O}{R}fn make(read n){{record R{{right=n+1;hole=None;left=n+2}}}} fn main(){{let n=40;let weak=make(n);42}}"), "42"),
        (format!("{O}{R}fn tick(inout count){{count<-count+1;40}} fn make(read n){{record R{{right=n+1;hole=None;left=n+2}}}} fn main(){{let mut count=0;let weak=make(tick(inout count));count+41}}"), "42"),
        (format!("{O}{R}type P = {{left:i32;right:i32}} fn make(read old,inout n){{record R{{right={{n<-n+old;n}};hole=None;left=n}}}} fn main(){{let mut p=record P{{left=20;right=22}};let weak=make(p.left,inout p.right);p.right}}"), "42"),
        (format!("{O}{R}type P = {{n:i32}} fn make(read old,inout n){{record R{{right={{n<-n+old;n}};hole=None;left=n+1}}}} fn main(){{let mut p=record P{{n=20}};let weak=make({{p.n+1}},inout p.n);p.n+1}}"), "42"),
        (format!("{O}{R}type P = {{n:i32}} fn make(old:P,inout p:P){{record R{{right={{p.n<-p.n+2;p.n}};hole=None;left=old.n}}}} fn main(){{let mut p=record P{{n=20}};let old=p;let weak=make(old,inout p);old.n+p.n}}"), "42"),
    ] {
        let c = compare(&source, Some(expected));
        assert!(!c.contains("restrict"), "Copy field loans do not prove pointee noalias");
    }
}

#[test]
fn observed_and_discarded_mark_calls_keep_distinct_exact_demand_instances() {
    let source = format!("{O}{R}{MARK}fn make(inout n){{{RECORD}}} fn main(){{let mut n=0;let before=mark(inout n,3);let weak=make(inout n);n+before}}");
    let c = compare(&source, Some("315"));
    assert!(c.contains("static int32_t rl_fn_"));
    assert!(c.contains("static void rl_fn_"));
    assert!(!c.contains("rl_adt_"));
}

#[test]
fn record_field_effects_and_traps_have_exact_entry_argument_and_leave_events() {
    for (first, last, expected, trace) in [
        (
            "mark(inout n,1)",
            "mark(inout n,2)",
            Some("12"),
            "E1;E2;E3;A1;L3;E3;A2;L3;L2;L1;",
        ),
        ("1/0", "mark(inout n,2)", None, "E1;E2;"),
        ("mark(inout n,1)", "1/0", None, "E1;E2;E3;A1;L3;"),
    ] {
        let source = format!("{O}{R}{MARK}fn make(inout n){{record R{{right={first};hole=None;left={last}}}}} fn main(){{let mut n=0;let weak=make(inout n);n}}");
        let c = compare(&source, expected);
        let c = trace_mark(trace_frames(c));
        Fixture::new().run_c(&c, expected, trace, &source);
    }
}

#[test]
fn discarded_record_arithmetic_and_computed_read_inputs_still_trap() {
    for source in [
        format!("{O}{R}fn make(read n){{record R{{right=n+1;hole=None;left=n}}}} fn main(){{let weak=make(2147483647);42}}"),
        format!("{O}{R}fn make(read n){{record R{{right=n;hole=None;left=n}}}} fn main(){{let weak=make(2147483647+1);42}}"),
        format!("{O}{R}fn make(){{record R{{right=1;hole=None;left=2147483647+1}}}} fn main(){{let weak=make();42}}"),
    ] {
        compare(&source, None);
    }
}

#[test]
fn bare_nonreturning_record_field_keeps_prefix_effects_and_real_depth_guard() {
    let source = format!("{O}type S<a,b> = {{first:i32;hole:O<b>;stop:a;last:i32}} {MARK}fn spin(){{spin()}} fn make(inout n){{record S{{first=mark(inout n,1);hole=None;stop=spin();last=mark(inout n,2)}}}} fn main(){{let mut n=0;let weak=make(inout n);n}}");
    let c = compare(&source, None);
    let mut trace = "E1;E2;E3;A1;L3;".to_string();
    for depth in 3..=33 {
        trace.push_str(&format!("E{depth};"));
    }
    Fixture::new().run_c(&trace_mark(trace_frames(c)), None, &trace, &source);
    compare(
        "type Store<a> = {value:a} fn forever(){forever()} fn main(){let unused=record Store{value=forever()};42}",
        None,
    );
}

#[test]
fn record_fields_reuse_only_the_existing_represented_interruption_adapter() {
    let source = format!("{O}{R}{MARK}fn spin(){{spin()}} fn ignore(x){{42}} fn make(inout n){{record R{{right=mark(inout n,1);hole=None;left=ignore(spin())}}}} fn main(){{let mut n=0;let weak=make(inout n);n}}");
    let c = compare(&source, None);
    let mut trace = "E1;E2;E3;A1;L3;".to_string();
    for depth in 3..=33 {
        trace.push_str(&format!("E{depth};"));
    }
    Fixture::new().run_c(&trace_mark(trace_frames(c)), None, &trace, &source);
    unsupported(&format!("{O}{R}fn spin(){{spin()}} fn unknown(x){{record R{{right=1;hole=None;left=2}}}} fn main(){{let weak=unknown(spin());42}}"));
}

#[test]
fn recursive_record_base_preserves_main_inclusive_depth_and_argument_order() {
    for n in [30, 31] {
        let source = format!("{O}{R}fn recur(n){{if n==0{{record R{{right=1;hole=None;left=2}}}}else{{recur(n-1)}}}} fn main(){{let weak=recur({n});42}}");
        let expected = if n == 30 { Some("42") } else { None };
        let c = compare(&source, expected);
        let c = trace_frames(c);
        let needle = "static int32_t rl_checked(int op, int32_t x, int32_t y) {";
        assert_eq!(c.matches(needle).count(), 1);
        let replacement = format!("{needle}\nif(op==1) fprintf(stderr, \"A%d;\", (int)(x-y));");
        let c = c.replacen(needle, &replacement, 1);
        let mut trace = "E1;E2;".to_string();
        for remaining in (0..n).rev() {
            let depth = n - remaining + 2;
            trace.push_str(&format!("A{remaining};E{depth};"));
        }
        if n == 30 {
            for depth in (1..=32).rev() {
                trace.push_str(&format!("L{depth};"));
            }
        }
        Fixture::new().run_c(&c, expected, &trace, &source);
    }
}

#[test]
fn observed_mutable_aliased_and_projected_layoutless_records_stay_unsupported() {
    for main in [
        "made()",
        "record R{right=1;hole=None;left=2}",
        "let weak=made();weak",
        "let weak=made();let alias=weak;42",
        "let mut weak=made();42",
        "let weak=made();weak.left",
        "let weak=made();let unused=weak.left;42",
        "let weak=made();match weak.hole{None=>42|Some(_)=>0}",
        "let weak=made();if false{weak.left}else{42}",
        "let weak=made();if false{inspect(weak)}else{42}",
        "let weak=made();if false{observe(weak)}else{42}",
        "let mut weak=made();if false{weak<-made();0}else{42}",
        "let mut weak=made();if false{weak.left<-42;0}else{42}",
        "inspect(made())",
    ] {
        unsupported(&format!("{O}{R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn inspect(x){{42}} fn observe(read x){{42}} fn main(){{{main}}}"));
    }
}

#[test]
fn record_discard_does_not_broaden_local_shapes_or_unknown_storage() {
    for source in [
        format!("{O}type R<a> = {{value:O<a>}} fn main(){{let r=record R{{value=None}};let weak=r.value;42}}"),
        format!("{O}{R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn main(){{let shape=None;let weak=made();42}}"),
        format!("{O}type Store<a> = {{value:a}} fn main(){{let mut value=record Store{{value=None}};42}}"),
        format!("{O}fn main(){{None}}"),
        format!("{O}fn absent(){{None}} fn main(){{let weak=absent();match weak{{None=>42|Some(_)=>0}}}}"),
    ] {
        unsupported(&source);
    }
}

#[test]
fn record_discard_keeps_full_source_type_label_and_loan_checks() {
    for fields in [
        "right=1;hole=None;right=2",
        "right=1;hole=None",
        "right=1;hole=None;left=2;other=3",
        "right=true;hole=None;left=2",
        "right=1;hole=None;left=if false{true+1}else{2}",
    ] {
        source_error(&format!(
            "{O}{R}fn made(){{record R{{{fields}}}}} fn main(){{let weak=made();42}}"
        ));
    }
    for source in [
        format!("{O}{R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn ints(x:R<i32>){{()}} fn bools(x:R<bool>){{()}} fn main(){{let weak=made();ints(weak);bools(weak);42}}"),
        format!("{O}{R}type Other<a> = {{left:i32;hole:O<a>;right:i32}} fn made(n){{if n{{record R{{right=1;hole=None;left=2}}}}else{{record Other{{right=1;hole=None;left=2}}}}}} fn main(){{let weak=made(true);42}}"),
        format!("{O}{R}fn bad(){{1+true}} fn made(){{record R{{right=1;hole=None;left=2}}}} fn main(){{let weak=made();42}}"),
        format!("{O}{R}fn made(read a,inout b){{record R{{right={{b<-b+a;b}};hole=None;left=a}}}} fn main(){{let mut n=0;let weak=made(n,inout n);42}}"),
        format!("{O}{R}type P = {{left:i32;right:i32}} fn made(read a,inout b){{record R{{right={{b<-b+a;b}};hole=None;left=a}}}} fn main(){{let mut p=record P{{left=20;right=22}};let weak=made(p.left,inout p.left);42}}"),
        format!("{O}{R}type P = {{left:i32;right:i32}} fn made(read p:P,inout n){{record R{{right={{n<-p.left;n}};hole=None;left=0}}}} fn main(){{let mut p=record P{{left=20;right=22}};let weak=made(p,inout p.right);42}}"),
        format!("{O}{R}fn made(read n){{record R{{right={{n<-42;n}};hole=None;left=0}}}} fn main(){{let weak=made(0);42}}"),
    ] {
        source_error(&source);
    }
}

#[test]
fn discarded_records_do_not_admit_owner_or_native_dependencies() {
    for source in [
        format!("type O<a> = None | Some(a) | Hidden(SystemBoxI32) {R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn main(){{let weak=made();42}}"),
        format!("type O<a> = None | Some(a) | Hidden(ptr<Handle>) {R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn main(){{let weak=made();42}}"),
        format!("{O}{R}extern fn foreign() -> i32; fn made(){{record R{{right=foreign();hole=None;left=2}}}} fn main(){{let weak=made();42}}"),
    ] {
        // Source-vs-lowering rejection remains the established owner/native boundary.
        let program = parser::parse(&source).unwrap();
        if let Ok(checked) = check::analyze(&program) {
            assert!(typed::lower_main(&checked).is_err(), "{source}");
        }
    }
}

#[test]
fn optional_origin_apis_still_reject_after_record_semantic_lowering() {
    for source in [
        format!("{O}{R}fn made(){{record R{{right=1;hole=None;left=2}}}} fn main(){{let weak=made();42}}"),
        format!("{O}{R}fn main(){{let weak=if true{{record R{{right=1;hole=None;left=2}}}}else{{record R{{right=3;hole=None;left=4}}}};42}}"),
    ] {
        let (_, lowered, _) = lower(&source);
        assert_eq!(typed::evaluate(&lowered).unwrap().to_string(), "42");
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", &source);
        let sources = rill::modules::compile::load_program_with_sources(&path).unwrap();
        for error in [
            lower_main_with_function_origins(sources.clone()).unwrap_err(),
            lower_main_with_copy_node_origins(sources).unwrap_err(),
        ] {
            assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
            assert!(error.contains("control results"), "{error}");
        }
    }
}
