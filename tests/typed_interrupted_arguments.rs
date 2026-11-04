//! Interrupted eager value argument source/C acceptance.
//! The C gate uses CC (default cc) at O0/O2; run separately with GCC and Clang
//! for the full matrix. Test-only instrumentation retains the real source main.
use rill::{
    check, interpreter,
    modules::compile::load_program_with_sources,
    parser, typed,
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
            "rill-interrupted-{}-{}-{}",
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
        let stdout = self.0.join("stdout");
        let stderr = self.0.join("stderr");
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
fn ordinary_value_calls_nested_calls_and_bare_results_are_interrupted() {
    for source in [
        "fn forever() { forever() } fn ignore(x) { 42 } fn main() { ignore(forever()) }",
        "fn forever() { forever() } fn ignore(x) { 42 } fn main() { ignore(ignore(forever())) }",
        "fn forever() { forever() } fn id(x) { x } fn main() { id(forever()) }",
        "fn a() { b() } fn b() { a() } fn ignore(x) { 42 } fn main() { ignore(a()) }",
        "fn forever() { forever() } fn ignore(a, b, c) { 42 } fn main() { ignore(1, forever(), 1 / 0) }",
        "fn forever() { forever() } fn ignore(a, b) { 42 } fn main() { let mut n = 0; ignore({ n <- n + 1; n }, forever()) }",
    ] { compare(source, None); }
}

#[test]
fn selected_and_unselected_calls_preserve_branch_results() {
    compare("fn forever() { forever() } fn ignore(x) { 41 } fn main() { if false { ignore(forever()) } else { 42 } }", Some("42"));
    compare("fn forever() { forever() } fn ignore(x) { 41 } fn main() { if true { ignore(forever()) } else { 42 } }", None);
    compare("fn forever() { forever() } fn ignore(x) { 41 } fn main() { match false { true => ignore(forever()) | false => 42 } }", Some("42"));
}

#[test]
fn omitted_suffix_and_unused_declarations_do_not_demand_payload_storage() {
    for source in [
        "type O<a> = None | Some(a) fn forever() { forever() } fn ignore(a, b) { 42 } fn main() { ignore(forever(), None) }",
        "type O<a> = None | Some(a) extern fn keep(n: i32) -> i32; fn forever() { forever() } fn ignore(x) { 42 } fn main() { ignore(forever()) }",
        "type O<a> = None | Some(a) fn unused() { None } fn forever() { forever() } fn ignore(a, b) { 42 } fn main() { ignore(forever(), unused()) }",
    ] { compare(source, None); }
}

#[test]
fn original_source_errors_in_suffixes_and_never_entered_bodies_win() {
    for source in [
        "fn forever() { forever() } fn ignore(a, b) { 42 } fn main() { ignore(forever(), true + 1) }",
        "fn forever() { forever() } fn ignore(a, b) { 42 } fn main() { ignore(forever(), match true { true => 42 }) }",
        "fn forever() { forever() } fn ignore(a) { true + 1 } fn main() { ignore(forever()) }",
        "fn set(inout x, read y) { x <- y } fn forever() { forever() } fn ignore(a,b) { 42 } fn main() { let mut n=0; ignore(forever(), set(inout n,n)) }",
        "type O<a> = None | Some(a) fn forever() { forever() } fn ignore(a,b) {42} fn main(){let mut n=None; ignore(forever(), {n <- Some(1); n <- Some(true); 0})}",
    ] {
        let program = parser::parse(source).unwrap();
        let error = check::analyze(&program).err().unwrap();
        assert_eq!(interpreter::run(&program).unwrap_err(), error);
        assert_eq!(rill::backend::emit(&program).unwrap_err(), error);
    }
}

#[test]
fn already_representable_outer_loans_keep_their_k3_call_path() {
    for source in [
        "fn loop(n:i32){loop(n)} fn sink(read x:i32,y:i32){y} fn spin(){spin()} fn main(){if false {let unused=spin();42} else {sink(0,loop(0))}}",
        "fn loop(n:i32){loop(n)} fn sink(inout x:i32,y:i32){x <- y; y} fn spin(){spin()} fn main(){let mut n=0; if false {let unused=spin();42} else {sink(inout n,loop(0))}}",
    ] { compare(source, None); }
}

#[test]
fn unsupported_outer_modes_and_real_unknown_storage_remain_rejected() {
    for source in [
        "fn forever() {forever()} fn inspect(read x){42} fn main(){inspect(forever())}",
        "fn forever() {forever()} fn inspect(inout n: i32, x){42} fn main(){let mut n=0; inspect(inout n, forever())}",
        "fn forever(){forever()} fn main(){let x=forever(); x}",
        "fn forever(){forever()} fn main(){let mut x=forever(); 42}",
        "type O<a> = None | Some(a) fn absent(){None} fn ignore(x){42} fn main(){ignore(absent())}",
        "type O<a> = None | Some(a) fn forever(){forever()} fn main(){let unused=forever(); None}",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        assert!(typed::lower_main(&checked).is_err(), "{source}");
    }
}

#[test]
fn origin_apis_reject_only_after_ordinary_success() {
    for source in [
        "fn forever(){forever()} fn ignore(x){42} fn main(){ignore(forever())}",
        "fn forever(){forever()} fn ignore(x){42} fn main(){if false {ignore(forever())} else {42}}",
    ] {
        lower(source);
        let fixture=Fixture::new();
        let path=fixture.write("main.rill", source);
        let sources=load_program_with_sources(&path).unwrap();
        for error in [lower_main_with_function_origins(sources.clone()).unwrap_err(), lower_main_with_copy_node_origins(sources).unwrap_err()] {
            assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
            assert!(error.contains("no-normal"), "{error}");
        }
    }
}

fn replace_once(source: String, needle: &str, replacement: &str) -> String {
    assert_eq!(
        source.matches(needle).count(),
        1,
        "instrumentation anchor: {needle}"
    );
    source.replacen(needle, replacement, 1)
}
fn instrument(c: String) -> String {
    let c = replace_once(
        c,
        "_Noreturn static void rl_trap(void) {",
        r#"
static volatile unsigned entries, attempts, leaves, prefix, args, suffix, outer, argtrap, order, current_depth;
_Noreturn static void rl_trap(void) {
 fprintf(stderr, "K3b entries=%u attempts=%u depth=%u leaves=%u prefix=%u args=%u suffix=%u outer=%u argtrap=%u order=%u\n", entries, attempts, current_depth, leaves, prefix, args, suffix, outer, argtrap, order);
"#,
    );
    let c=replace_once(c,"static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }", "static void rl_enter(void) { ++attempts; if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; ++entries; current_depth=rl_call_depth; }");
    let c = replace_once(
        c,
        "static void rl_leave(void) { --rl_call_depth; }",
        "static void rl_leave(void) { ++leaves; --rl_call_depth; current_depth=rl_call_depth; }",
    );
    replace_once(
        c,
        "static int32_t rl_checked(int op, int32_t x, int32_t y) {",
        r#"
static int32_t rl_checked(int op, int32_t x, int32_t y) {
 if(op==0 && y==10000){if(prefix!=0 || args!=0) order=1; ++prefix;}
 if(op==0 && y==1){if(prefix!=1 || x!=(int32_t)args) order=1; ++args;}
 if(op==0 && y==20000) ++suffix;
 if(op==0 && y==30000) ++outer;
 if(op==3 && y==0) ++argtrap;
"#,
    )
}

#[test]
fn exact_prefix_argument_guard_and_suppressed_suffix_callee_events() {
    for (prefix, argument, entries, attempts, args, argtrap) in [
        ("{ n <- n + 10000; n }", "n + 1", 32, 33, 31, 0),
        (
            "n + 10000",
            "{let next=n+1; if n==30 {next/0} else {next}}",
            32,
            32,
            31,
            1,
        ),
        ("{let prefix=n+10000; prefix/0}", "n + 1", 1, 1, 0, 1),
    ] {
        let source=format!("fn forever(n:i32){{forever({argument})}} fn ignore(a,b,c){{a+30000}} fn main(){{let mut n=0; ignore({prefix},forever(0),{{n <- n+20000; n}})}}");
        let (_, typed, c) = lower(&source);
        assert_eq!(typed::evaluate(&typed).unwrap_err(), "runtime trap");
        let trace=format!("K3b entries={entries} attempts={attempts} depth={entries} leaves=0 prefix=1 args={args} suffix=0 outer=0 argtrap={argtrap} order=0\n");
        Fixture::new().run_c(&instrument(c), None, &trace, &source);
    }
}
