//! K3 acceptance assertions, prepared source-only and not execution evidence.
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
const FOREVER: &str = "fn forever() { forever() }\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-no-normal-{}-{}-{}",
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
fn bare_direct_and_mutual_results_use_void_without_a_main_printer() {
    for source in [
        "fn forever() { forever() } fn main() { forever() }",
        "fn first(n: i32) { second(n) } fn second(n: i32) { first(n) } fn main() { first(0) }",
    ] {
        let c = compare(source, None);
        assert!(c.contains("static void rl_fn_"));
        assert!(!c.contains("static int32_t rl_fn_") && !c.contains("static rl_unit rl_fn_"));
        let main = c.rsplit_once("\nint main(void)").unwrap().1;
        assert!(!main.contains("result") && !main.contains("printf") && !main.contains("puts"));
    }
}

#[test]
fn prefixes_runtime_branches_matches_and_distinct_parameter_instances_remain_real() {
    for source in [
        "fn forever(n: i32) { let next = n + 1; if n < 2 { forever(next) } else { forever(n) } } fn main() { forever(0) }",
        "type Tag = Off | On fn forever(tag: Tag, n: i32) { match tag { Off => forever(On, n + 1) | On => forever(Off, n + 1) } } fn main() { forever(Off, 0) }",
        "fn forever(n: i32) { forever(n) } fn main() { let mut n = 0; while n < 2 { n <- n + 1; }; forever(n) }",
        "fn forever(x) { forever(x) } fn main() { if false { let unused = forever(true); 1 } else { let unused = forever(0); 2 } }",
        "type P = { x: i32; y: i32 } fn forever(value: i32, read observed: i32, inout changed: i32) { changed <- changed + 1; forever(value, observed, inout changed) } fn main() { let mut p = record P { x = 1; y = 2 }; forever(p.x, { let done = p.x <- 3; p.x }, inout p.y) }",
    ] {
        compare(source, None);
    }
}

#[test]
fn unused_divergent_lets_keep_concrete_abi_and_runtime_branch_selection() {
    for tail in [
        "let unused = forever(); 42",
        "let item = forever(); let item = 42; item",
        "if true { let unused = forever(); 41 } else { 42 }",
    ] {
        let c = compare(&format!("{FOREVER}fn main() {{ {tail} }}"), None);
        assert!(c.contains("static void rl_fn_"));
        assert!(c
            .rsplit_once("\nint main(void)")
            .unwrap()
            .1
            .contains("int32_t result ="));
    }
    compare(
        &format!(
            "{FOREVER}fn main() {{ if false {{ let unused = forever(); 41 }} else {{ 42 }} }}"
        ),
        Some("42"),
    );
    compare(&format!("{FOREVER}fn main() {{ match false {{ true => {{ let unused = forever(); 41 }} | false => 42 }} }}"), Some("42"));
}

#[test]
fn argument_and_prefix_traps_are_not_replaced_by_synthetic_success() {
    for source in [
        "fn forever(n: i32) { forever(n) } fn main() { forever(1 / 0) }",
        "fn forever(n: i32) { forever(n) } fn main() { let n = 2147483647 + 1; forever(n) }",
        "fn forever(n: i32) { forever(n) } fn main() { forever(byte_at(\"\", 0)) }",
        "fn forever(read n: i32) { forever(n) } fn main() { forever(1 / 0) }",
    ] {
        compare(source, None);
    }
}

#[test]
fn possible_concrete_base_return_keeps_the_ordinary_value_contract() {
    let c = compare(
        "fn again(n: i32) { if false { 42 } else { again(n) } } fn main() { again(0) }",
        None,
    );
    assert!(!c.contains("static void rl_fn_"));
    assert!(c.contains("static int32_t rl_fn_"));
    compare(
        "fn again(n: i32) { if n == 0 { 42 } else { again(n - 1) } } fn main() { again(2) }",
        Some("42"),
    );
}

#[test]
fn unsupported_representation_is_not_admitted_by_discard_or_nonreturning_context() {
    // Interrupted eager value calls moved to typed_interrupted_arguments.
    for source in [
        "fn forever() { forever() } fn main() { let used = forever(); used }",
        "fn forever() { forever() } fn main() { let mut unused = forever(); 42 }",
        "type O<a> = None | Some(a) fn forever() { forever() } fn main() { let unused = forever(); None }",
        "type O<a> = None | Some(a) fn again() { if false { None } else { again() } } fn main() { let unused = again(); 42 }",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap_or_else(|error| panic!("must remain source-accepted: {error}\n{source}"));
        assert!(typed::lower_main(&checked).is_err(), "{source}");
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", source);
        let sources = load_program_with_sources(&path).unwrap();
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap_err();
        assert_eq!(lower_main_with_function_origins(sources.clone()).unwrap_err(), ordinary);
        assert_eq!(lower_main_with_copy_node_origins(sources).unwrap_err(), ordinary);
        // Unsupported Copy lowering may retain its existing reference fallback;
        // this assertion does not claim every public fallback must reject it.
    }
}

#[test]
fn retained_continuations_still_fail_ordinary_source_checks_first() {
    for source in [
        "fn forever() { forever() } fn main() { let unused = forever(); true + 1 }",
        "fn forever() { forever() } fn main() { let unused = forever(); match true { true => 42 } }",
        "fn set(inout x: i32, read y: i32) { x <- y } fn forever() { forever() } fn main() { let unused = forever(); let mut n = 0; set(inout n, n) }",
        "extern fn native() -> i32; fn forever() { forever() } fn main() { let unused = forever(); let owner = system_box_i32_abort(1); native() + box_read(owner) }",
    ] {
        let program = parser::parse(source).unwrap();
        let ordinary = check::analyze(&program).err().unwrap();
        assert_eq!(interpreter::run(&program).unwrap_err(), ordinary);
        assert_eq!(rill::backend::emit(&program).unwrap_err(), ordinary);
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", source);
        // SourceProgram loading itself performs source checks, before an
        // optional origin API can receive a checked input.
        let error = load_program_with_sources(&path).unwrap_err();
        assert_eq!(error.code, "E-MODULE-CHECK");
        assert_eq!(error.message, ordinary);
    }
}

#[test]
fn optional_origins_reject_only_after_successful_ordinary_lowering() {
    for tail in [
        "forever()",
        "let unused = forever(); 42",
        "if false { let unused = forever(); 41 } else { 42 }",
    ] {
        let source = format!("{FOREVER}fn main() {{ {tail} }}");
        lower(&source);
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        for error in [
            lower_main_with_function_origins(sources.clone()).unwrap_err(),
            lower_main_with_copy_node_origins(sources).unwrap_err(),
        ] {
            assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
            assert!(error.contains("no-normal"), "{error}");
        }
    }
    let fixture = Fixture::new();
    let path = fixture.write("main.rill", "fn main() { 42 }");
    let sources = load_program_with_sources(&path).unwrap();
    lower_main_with_function_origins(sources.clone()).unwrap();
    lower_main_with_copy_node_origins(sources).unwrap();
}

#[test]
fn unused_native_declaration_survives_a_divergent_result() {
    let c = compare("extern fn keep(n: i32) -> i32; fn forever() { forever() } fn main() { let unused = forever(); 42 }", None);
    assert!(c.contains("extern int32_t keep(int32_t);"));
}

fn replace_once(source: String, needle: &str, replacement: &str) -> String {
    assert_eq!(
        source.matches(needle).count(),
        1,
        "test instrumentation anchor changed: {needle}"
    );
    source.replacen(needle, replacement, 1)
}

fn instrument(c: String) -> String {
    let c = replace_once(
        c,
        "_Noreturn static void rl_trap(void) {",
        r#"
static volatile unsigned ev_entries, ev_attempts, ev_depth, ev_prefix, ev_args;
static volatile unsigned ev_tail, ev_argtrap, ev_order, ev_leaves;
static volatile int32_t ev_last;
_Noreturn static void rl_trap(void) {
    fprintf(stderr, "K3 entries=%u attempts=%u depth=%u prefixes=%u args=%u last=%" PRId32 " tail=%u argtrap=%u order=%u leaves=%u\n",
        ev_entries, ev_attempts, ev_depth, ev_prefix, ev_args, ev_last,
        ev_tail, ev_argtrap, ev_order, ev_leaves);
"#,
    );
    let c = replace_once(c,
        "static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }",
        "static void rl_enter(void) { ++ev_attempts; if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; ++ev_entries; ev_depth = rl_call_depth; }");
    let c = replace_once(
        c,
        "static void rl_leave(void) { --rl_call_depth; }",
        "static void rl_leave(void) { ++ev_leaves; --rl_call_depth; ev_depth = rl_call_depth; }",
    );
    replace_once(
        c,
        "static int32_t rl_checked(int op, int32_t x, int32_t y) {",
        r#"
static int32_t rl_checked(int op, int32_t x, int32_t y) {
    if (op == 0 && y == 10000) {
        if (x != (int32_t)ev_prefix || ev_prefix != ev_args) ev_order = 1;
        ++ev_prefix;
    }
    if (op == 0 && y == 1) {
        if (x != (int32_t)ev_args || ev_prefix != ev_args + 1) ev_order = 1;
        ++ev_args; ev_last = x + 1;
    }
    if (op == 3 && y == 0) ++ev_argtrap;
    if (op == 2 && x == 5 && y == 7) ++ev_tail;
"#,
    )
}

#[test]
fn real_main_counts_31_recursive_prefixes_and_boundary_argument_before_guard() {
    for (argument, attempts, argument_traps) in [
        ("n + 1", 33, 0),
        (
            "{ let next = n + 1; if n == 30 { next / 0 } else { next } }",
            32,
            1,
        ),
    ] {
        let source = format!("fn forever(n: i32) {{ let prefix = n + 10000; forever({argument}) }} fn main() {{ let unused = forever(0); 5 * 7 }}");
        let (program, lowered, c) = lower(&source);
        assert_eq!(typed::evaluate(&lowered).unwrap_err(), "runtime trap");
        assert_eq!(interpreter::run(&program).unwrap_err(), "runtime trap");
        assert_eq!(
            interpreter::run_reference(&program).unwrap_err(),
            "runtime trap"
        );
        let trace = format!("K3 entries=32 attempts={attempts} depth=32 prefixes=31 args=31 last=31 tail=0 argtrap={argument_traps} order=0 leaves=0\n");
        // Keep the generated C main unchanged. Calling the callee directly at
        // depth one would allow 32 prefixes and would not establish this boundary.
        Fixture::new().run_c(&instrument(c), None, &trace, &source);
    }
}
