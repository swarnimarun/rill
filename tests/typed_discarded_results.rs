//! Author-focused normally returning discarded-result parity.
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

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-discarded-{}-{}-{}",
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
        let binary = self.0.join("main");
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        for optimization in ["-O0", "-O2"] {
            let context = format!("{} {optimization}\n{source}", compiler.to_string_lossy());
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
            if !build.stderr.is_empty() {
                eprintln!(
                    "{context}: C diagnostic: {}",
                    String::from_utf8_lossy(&build.stderr)
                );
            }
            let output = self.output(&mut Command::new(&binary), 10, &context);
            match expected {
                Some(value) => {
                    assert!(output.status.success(), "{context}");
                    assert_eq!(output.stdout, format!("{value}\n").as_bytes(), "{context}");
                    assert_eq!(output.stderr, trace.as_bytes(), "{context}");
                }
                None => {
                    // Signal termination and host-stack failure are not the language trap.
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
    let typed = typed::lower_main(&checked).unwrap_or_else(|error| panic!("{error}\n{source}"));
    typed::verify(&typed).unwrap();
    let c = typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(&program).unwrap(), c, "{source}");
    assert_eq!(
        typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
        c
    );
    assert_eq!(program, snapshot, "lowering mutated checked source");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert!(!c.contains("__builtin_unreachable") && !c.contains("_Noreturn static void rl_fn_"));
    // A no-result Rill call cannot initialize a fabricated value temporary.
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
    (program, typed, c)
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
    Fixture::new().run_c(&c, expected, "", source);
    c
}

#[test]
fn ordinary_discarded_results_return_without_layout_or_dummy_value() {
    for source in [
        "type O<a> = None | Some(a) fn absent(){None} fn main(){let weak=absent();42}",
        "type O<a> = None | Some(a) fn absent(){None} fn main(){absent();42}",
        "type O<a> = None | Some(a) fn absent(){None} fn main(){absent();}",
        "type O<a> = None | Some(a) fn absent(){None} fn main(){let weak=absent();let weak=42;weak}",
        "type O<a> = None | Some(a) fn absent(){None} fn main(){let a=absent();let b=absent();42}",
        "type O<a> = None | Some(a) fn absent(){None} fn relay(){absent()} fn main(){let weak=relay();42}",
    ] {
        let expected = if source.ends_with("absent();}") { "()" } else { "42" };
        let c = compare(source, Some(expected));
        assert!(!c.contains("rl_adt_"));
        assert!(c.contains("rl_leave(); return;"));
    }
}
#[test]
fn discarded_effects_loans_control_and_constructor_order() {
    for (source, expected) in [
        ("type O<a> =None|Some(a) fn absent(inout n){n<-n+1;None} fn main(){let mut n=41;let weak=absent(inout n);n}", "42"),
        ("type O<a> =None|Some(a) type P={n:i32} fn absent(inout n){n<-n+1;None} fn main(){let mut p=record P{n=41};let weak=absent(inout p.n);p.n}", "42"),
        ("type O<a> =None|Some(a) fn absent(read n){let checked=n+1;None} fn main(){let weak=absent(40+1);42}", "42"),
        ("type O<a> =None|Some(a) type W<a> =W(i32,O<a>,i32) fn mark(inout n,d){n<-n*10+d;n} fn made(inout n){W(mark(inout n,1),None,mark(inout n,2))} fn main(){let mut n=0;let weak=made(inout n);n}", "12"),
        ("type O<a> =None|Some(a) fn choose(n){if n==0{None}else{choose(n-1)}} fn main(){let weak=choose(4);42}", "42"),
        ("type O<a> =None|Some(a) fn choose(n){match n{0=>None|_=>choose(n-1)}} fn main(){let weak=choose(4);42}", "42"),
        ("type O<a> =None|Some(a) type W<a> =W(i32,O<a>) fn made(None:i32){W(None,None())} fn main(){let weak=made(42);42}", "42"),
        ("type O<a> =None|Some(a) type B<a> =BN|BS(a) fn absent(){let unused=if true{BN}else{BN};None} fn main(){let weak=absent();42}", "42"),
    ] { compare(source, Some(expected)); }
}
#[test]
fn discarded_traps_and_existing_nonreturning_composition() {
    for source in [
        "type O<a> =None|Some(a) fn absent(read n){let checked=n+1;None} fn main(){let weak=absent(2147483647);42}",
        "type O<a> =None|Some(a) type W<a> =W(i32,O<a>) fn made(){W(1/0,None)} fn main(){let weak=made();42}",
        "type O<a> =None|Some(a) fn absent(){None} fn spin(){spin()} fn main(){let weak=absent();spin()}",
        "type O<a> =None|Some(a) type W<a,b> =W(a,O<b>) fn spin(){spin()} fn made(){W(spin(),None)} fn main(){let weak=made();42}",
        "type O<a> =None|Some(a) fn spin(){spin()} fn ignore(x){42} fn absent(){None} fn main(){let weak=absent();ignore(spin())}",
        "type O<a> = None | Some(a) fn again() { if false { None } else { again() } } fn main() { let unused = again(); 42 }",
        "type O<a> = None | Some(a) type W<a> = W(i32, O<a>) fn spin(){spin()} fn ignore(x){42} fn made(){W(ignore(spin()), None)} fn main(){let weak=made();42}",
        "type O<a> = None|Some(a) fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),None)}",
    ] { compare(source, None); }
}
#[test]
fn observed_mutable_and_lexically_used_layoutless_results_stay_unsupported() {
    for main in [
        "let weak=absent();weak",
        "let weak=absent();let alias=weak;42",
        "let mut weak=absent();42",
        "let weak=absent();match weak{None=>42|Some(_)=>0}",
        "absent()",
        "None",
        "let weak=absent();if false{inspect(weak)}else{42}",
    ] {
        let source = format!(
            "type O<a> =None|Some(a) fn absent(){{None}} fn inspect(x){{42}} fn main(){{{main}}}"
        );
        let parsed = parser::parse(&source).unwrap();
        let checked = check::analyze(&parsed).unwrap();
        assert!(
            typed::lower_main(&checked)
                .unwrap_err()
                .starts_with("E-TYPED-UNSUPPORTED:"),
            "{source}"
        );
    }
}

#[test]
fn direct_discarded_record_no_longer_requires_a_physical_layout() {
    compare(
        "type R<a> ={x:a} type O<a> =None|Some(a) fn made(){record R{x=None}} fn main(){let weak=made();42}",
        Some("42"),
    );
}
#[test]
fn discard_preserves_source_errors() {
    for source in [
        "type O<a> =None|Some(a) fn absent(){None} fn ints(x:O<i32>){()} fn bools(x:O<bool>){()} fn main(){let weak=absent();ints(weak);bools(weak);42}",
        "type O<a> =None|Some(a) fn absent(read x,inout y){None} fn main(){let mut n=0;let weak=absent(n,inout n);42}",
        "type O<a> =None|Some(a) fn absent(){if false{1+true;None}else{None}} fn main(){let weak=absent();42}",
        "type O<a> =None|Some(a) fn absent(){None} fn bad(){1+true} fn main(){let weak=absent();42}",
    ] { let parsed=parser::parse(source).unwrap(); assert!(check::analyze(&parsed).is_err(), "{source}"); }
}

#[test]
fn optional_origin_apis_reject_discard_only_after_semantic_lowering() {
    let source = "type O<a> = None | Some(a) fn absent(){None} fn main(){let weak=absent();42}";
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    typed::lower_main(&checked).unwrap();
    let fixture = Fixture::new();
    let path = fixture.write("main.rill", source);
    let sources = rill::modules::compile::load_program_with_sources(&path).unwrap();
    for error in [
        lower_main_with_function_origins(sources.clone()).unwrap_err(),
        lower_main_with_copy_node_origins(sources).unwrap_err(),
    ] {
        assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
        assert!(error.contains("control results"), "{error}");
    }
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
#[test]
fn source_main_and_discard_calls_keep_exact_normal_and_depth_frames() {
    let source = "type O<a> = None | Some(a) fn absent(){None} fn relay(){absent()} fn main(){let weak=relay();42}";
    let (_, _, c) = lower(source);
    Fixture::new().run_c(&trace_frames(c), Some("42"), "E1;E2;E3;L3;L2;L1;", source);
    for n in [30, 31] {
        let source = format!("type O<a> = None | Some(a) fn rec(n){{if n==0{{None}}else{{rec(n-1)}}}} fn main(){{let weak=rec({n});42}}");
        let (_, ir, c) = lower(&source);
        let expected = if n == 30 { Some("42") } else { None };
        let mut trace = String::new();
        for depth in 1..=n + 2 {
            trace.push_str(&format!("E{depth};"));
        }
        if n == 30 {
            for depth in (1..=32).rev() {
                trace.push_str(&format!("L{depth};"));
            }
        }
        assert_eq!(typed::evaluate(&ir).is_ok(), n == 30);
        Fixture::new().run_c(&trace_frames(c), expected, &trace, &source);
    }
}

#[test]
fn internal_only_discarded_types_and_unused_foreign_declarations_are_retained() {
    for source in [
        "type O<a> = None | Some(a) fn main(){let weak=if true{None}else{None};42}",
        "type O<a> = None | Some(a) fn main(){let weak=match true{true=>None|false=>None};42}",
        "type O<a> = None | Some(a) fn main(){if false{let weak=if true{None}else{None};0}else{42}}",
        "type O<a> = None | Some(a) type Other=Other(i32) extern fn unused(n:i32)->i32; fn absent(){None} fn main(){let weak=absent();42}",
    ] {
        compare(source, Some("42"));
    }
}

#[test]
fn constructor_arguments_retain_exact_source_main_events_and_trap_order() {
    for (last, expected, trace) in [
        (
            "mark(inout n,2)",
            Some("12"),
            "E1;E2;E3;A1;L3;E3;A2;L3;L2;L1;",
        ),
        ("1/0", None, "E1;E2;E3;A1;L3;"),
    ] {
        let source=format!("type O<a> = None|Some(a) type W<a> = W(i32,O<a>,i32) fn mark(inout n,d){{n<-n*10+d;n}} fn made(inout n){{W(mark(inout n,1),None,{last})}} fn main(){{let mut n=0;let weak=made(inout n);n}}");
        let (_, _, c) = lower(&source);
        let c = trace_frames(c);
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
        let c = c.replacen(&needle, &replacement, 1);
        Fixture::new().run_c(&c, expected, trace, &source);
    }
}
