//! Independent discarded-record parity and event-trace executable probes.
//! Compile as an external Rust harness against a freshly built candidate rlib.
//! All outputs are retained. Set K4_PROBE_OUTPUT to a fresh child of the approved
//! RAM scratch directory. This program never deletes files or changes the repo.
use rill::{backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn save(path: &Path, bytes: impl AsRef<[u8]>) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .unwrap();
    file.write_all(bytes.as_ref()).unwrap();
}
fn command(mut command: Command, root: &Path, stem: &str, seconds: u64) -> Output {
    let stdout = root.join(format!("{stem}.stdout"));
    let stderr = root.join(format!("{stem}.stderr"));
    command
        .stdin(Stdio::null())
        .stdout(
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&stdout)
                .unwrap(),
        )
        .stderr(
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&stderr)
                .unwrap(),
        );
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{stem}: timeout is not a language trap");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    }
}
fn replace_once(text: String, needle: &str, replacement: &str) -> String {
    assert_eq!(
        text.matches(needle).count(),
        1,
        "instrumentation anchor: {needle}"
    );
    text.replacen(needle, replacement, 1)
}
fn trace(c: String) -> String {
    let c = replace_once(
        c,
        "_Noreturn static void rl_trap(void) {",
        r#"
static unsigned k4_trace_depth;
static void k4_finish(void) { fprintf(stderr, "X%u;\n", k4_trace_depth); }
_Noreturn static void rl_trap(void) {"#,
    );
    let c = replace_once(c,
        "static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }",
        "static void rl_enter(void) { fprintf(stderr, \"A%u;\", rl_call_depth); if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; k4_trace_depth=rl_call_depth; fprintf(stderr, \"E%u;\", rl_call_depth); }");
    let c = replace_once(c,
        "static void rl_leave(void) { --rl_call_depth; }",
        "static void rl_leave(void) { fprintf(stderr, \"L%u;\", rl_call_depth); --rl_call_depth; k4_trace_depth=rl_call_depth; }");
    let c = replace_once(c,
        "static int32_t rl_checked(int op, int32_t x, int32_t y) {",
        "static int32_t rl_checked(int op, int32_t x, int32_t y) { if(op==0 && (y==1 || y==2)) fprintf(stderr, \"M%dD%u;\", (int)y, rl_call_depth);");
    replace_once(
        c,
        "int main(void) {",
        "int main(void) { if(atexit(k4_finish)!=0) return 91;",
    )
}
fn run_c(root: &Path, stem: &str, c: &str, expected: Option<&str>, stderr: &str) {
    let source = root.join(format!("{stem}.c"));
    save(&source, c);
    for compiler in ["gcc", "clang"] {
        for opt in ["-O0", "-O2"] {
            let name = format!("{stem}-{compiler}-{}", &opt[1..]);
            let binary = root.join(&name);
            let mut cmd = Command::new(compiler);
            cmd.args(["-std=c11", "-pedantic-errors", opt])
                .arg(&source)
                .arg("-o")
                .arg(&binary);
            let mut guard = Command::new("bash");
            guard.arg(std::env::var_os("K4_RESOURCE_GUARD").expect("resource guard required"));
            let guarded = command(guard, root, &format!("{name}-guard"), 5);
            assert!(
                guarded.status.success(),
                "{name}: resource guard failed: {}",
                String::from_utf8_lossy(&guarded.stdout)
            );
            let build = command(cmd, root, &format!("{name}-build"), 45);
            assert!(
                build.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            // Warnings are retained verbatim and reported, never silently dropped.
            if !build.stderr.is_empty() {
                println!(
                    "C DIAGNOSTIC {name}: {}",
                    String::from_utf8_lossy(&build.stderr)
                );
            }
            let output = command(Command::new(&binary), root, &format!("{name}-run"), 10);
            match expected {
                Some(value) => {
                    assert!(output.status.success(), "{name}");
                    assert_eq!(output.stdout, format!("{value}\n").as_bytes(), "{name}");
                }
                None => {
                    assert_eq!(
                        output.status.code(),
                        Some(1),
                        "{name}: not an ordinary language trap"
                    );
                    assert!(output.stdout.is_empty(), "{name}");
                }
            }
            assert_eq!(output.stderr, stderr.as_bytes(), "{name}");
        }
    }
}
fn accepted_inner(
    root: &Path,
    name: &str,
    source: &str,
    expected: Option<&str>,
    events: Option<&str>,
) {
    save(&root.join(format!("{name}.rill")), source);
    let ast = parser::parse(source).unwrap_or_else(|error| panic!("{name}: {error}"));
    let original = ast.clone();
    let checked = check::analyze(&ast).unwrap_or_else(|error| panic!("{name}: {error}"));
    let reference = interpreter::run_reference(&ast);
    match expected {
        Some(value) => assert_eq!(reference.as_ref().unwrap().to_string(), value, "{name}"),
        None => assert_eq!(reference.as_ref().unwrap_err(), "runtime trap", "{name}"),
    }
    let lowered = typed::lower_main(&checked)
        .unwrap_or_else(|error| panic!("{name}: TYPED LOWERING FAILED: {error}"));
    typed::verify(&lowered).unwrap();
    assert_eq!(typed::evaluate(&lowered), reference, "{name}");
    assert_eq!(interpreter::run(&ast), reference, "{name}");
    let c = typed::emit_c(&lowered).unwrap();
    assert_eq!(
        backend::emit(&ast).unwrap(),
        c,
        "{name}: default backend fallback is not typed support"
    );
    assert_eq!(
        typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
        c,
        "{name}: unstable instance numbering"
    );
    assert_eq!(ast, original, "{name}: source mutation");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{name}");
    assert!(
        !c.contains("_Noreturn static void rl_fn_") && !c.contains("__builtin_unreachable"),
        "{name}"
    );
    for line in c
        .lines()
        .filter(|line| line.starts_with("static void rl_fn_"))
    {
        let function = line
            .strip_prefix("static void ")
            .unwrap()
            .split('(')
            .next()
            .unwrap();
        assert!(
            !c.contains(&format!("= {function}(")),
            "{name}: void result manufactured into value"
        );
    }
    let uniform = backend::emit_reference(&ast);
    let uniform_log = match uniform {
        Ok(_) => "SUPPORTED".to_owned(),
        Err(error) => format!("REJECTED: {error}"),
    };
    save(
        &root.join(format!("{name}-uniform-classification.txt")),
        &uniform_log,
    );
    println!("UNIFORM {name} {uniform_log}");
    run_c(
        root,
        name,
        &c,
        expected,
        if expected.is_some() {
            ""
        } else {
            "rill: runtime trap\n"
        },
    );
    if let Some(events) = events {
        run_c(root, &format!("{name}-trace"), &trace(c), expected, events);
    }
    println!("PASS {name}");
}
fn boundary_inner(root: &Path, name: &str, source: &str, reference_expected: Option<&str>) {
    save(&root.join(format!("{name}.rill")), source);
    let ast = parser::parse(source).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
    let checked = check::analyze(&ast)
        .unwrap_or_else(|e| panic!("{name}: source rejected unexpectedly: {e}"));
    let error = typed::lower_main(&checked).unwrap_err();
    assert!(
        error.starts_with("E-TYPED-UNSUPPORTED:"),
        "{name}: wrong rejection class: {error}"
    );
    if let Some(expected) = reference_expected {
        assert_eq!(
            interpreter::run_reference(&ast).unwrap().to_string(),
            expected,
            "{name}"
        );
    }
    save(&root.join(format!("{name}-diagnostic.txt")), error);
    println!("PASS boundary {name}");
}
fn checker_error_inner(root: &Path, name: &str, source: &str) {
    save(&root.join(format!("{name}.rill")), source);
    let ast = parser::parse(source).unwrap_or_else(|error| {
        panic!("{name}: parser error rather than intended checker error: {error}")
    });
    let error = check::analyze(&ast)
        .err()
        .unwrap_or_else(|| panic!("{name}: checker unsoundly accepted"));
    assert_eq!(interpreter::run(&ast).unwrap_err(), error, "{name}");
    assert_eq!(backend::emit(&ast).unwrap_err(), error, "{name}");
    save(&root.join(format!("{name}-diagnostic.txt")), error);
    println!("PASS checker {name}");
}

static FAILURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn record_case(name: &str, action: impl FnOnce() + std::panic::UnwindSafe) {
    if std::panic::catch_unwind(action).is_err() {
        FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        eprintln!("FAIL independent case {name}");
    }
}
fn accepted(root: &Path, name: &str, source: &str, expected: Option<&str>, events: Option<&str>) {
    record_case(name, || {
        accepted_inner(root, name, source, expected, events)
    });
}
fn boundary(root: &Path, name: &str, source: &str, expected: Option<&str>) {
    record_case(name, || boundary_inner(root, name, source, expected));
}
fn checker_error(root: &Path, name: &str, source: &str) {
    record_case(name, || checker_error_inner(root, name, source));
}

fn module_records(root: &Path) {
    let library="export type O<a> = None|Some(a) export type R<a> = {left:i32;hole:O<a>;right:i32} fn mark(inout n:i32,d:i32){n<-n*10+d;n} export fn made(inout n:i32){record R{right=mark(inout n,1);hole=None;left=mark(inout n,2)}}";
    let entry = "import modulelib as m; fn main(){let mut n=0;let weak=m.made(inout n);n}";
    save(&root.join("modulelib.rill"), library);
    save(&root.join("module-main.rill"), entry);
    let sources =
        rill::modules::compile::load_program_with_sources(&root.join("module-main.rill")).unwrap();
    let program = sources.program();
    assert!(
        program
            .types
            .iter()
            .any(|d| d.record_fields.is_some() && d.name != d.variants[0].name),
        "module fixture must distinguish Type/Value symbol names"
    );
    let checked = check::analyze(program).unwrap();
    let typed = typed::lower_main(&checked).unwrap();
    typed::verify(&typed).unwrap();
    let expected = interpreter::run_reference(program);
    assert_eq!(expected.as_ref().unwrap().to_string(), "12");
    assert_eq!(typed::evaluate(&typed), expected);
    assert_eq!(interpreter::run(program), expected);
    let c = typed::emit_c(&typed).unwrap();
    assert_eq!(backend::emit(program).unwrap(), c);
    run_c(root, "record_module", &c, Some("12"), "");
    run_c(root, "record_module-trace", &trace(c), Some("12"), ORDER);
    for error in [
        typed::provenance::lower_main_with_function_origins(sources.clone()).unwrap_err(),
        typed::provenance::nodes::lower_main_with_copy_node_origins(sources).unwrap_err(),
    ] {
        assert!(
            error.starts_with("E-TYPED-UNSUPPORTED:") && error.contains("control results"),
            "{error}"
        );
    }
    println!("PASS record module and optional origin boundary");
}

// Independent record fixtures; see the verification report for executed source.
const O: &str = "type O<a> = None | Some(a) ";
const R: &str = "type R<a> = {left:i32;hole:O<a>;right:i32} ";
const MARK: &str = "fn mark(inout n:i32,d:i32){n<-n*10+d;n} ";
const CONSTRUCTION: &str = "record R{right=mark(inout n,1);hole=None;left=mark(inout n,2)}";
const ORDER: &str = "A0;E1;A1;E2;A2;E3;M1D3;L3;A2;E3;M2D3;L3;L2;L1;X0;\n";
fn main() {
    let root = PathBuf::from(
        std::env::var_os("K4_PROBE_OUTPUT").expect("fresh approved RAM output required"),
    );
    fs::create_dir(&root).unwrap();
    accepted(&root,"record_order",&format!("{O}{R}{MARK}fn made(inout n){{{CONSTRUCTION}}} fn main(){{let mut n=0;let weak=made(inout n);n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_forward",&format!("{O}{R}{MARK}fn made(inout n){{{CONSTRUCTION}}} fn relay(inout n){{made(inout n)}} fn main(){{let mut n=0;let weak=relay(inout n);n}}"),Some("12"),Some("A0;E1;A1;E2;A2;E3;A3;E4;M1D4;L4;A3;E4;M2D4;L4;L3;L2;L1;X0;\n"));
    accepted(&root,"record_under_enum",&format!("{O}{R}type W<a> = W(R<a>) {MARK}fn made(inout n){{W({CONSTRUCTION})}} fn main(){{let mut n=0;let weak=made(inout n);n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_under_record",&format!("{O}{R}type Outer<a> = {{nested:R<a>;more:O<O<a>>}} {MARK}fn made(inout n){{record Outer{{more=Some(None);nested={CONSTRUCTION}}}}} fn main(){{let mut n=0;let weak=made(inout n);n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_conditional",&format!("{O}{R}{MARK}fn made(inout n,b:bool){{if b{{{CONSTRUCTION}}}else{{record R{{hole=None;left=1/0;right=0}}}}}} fn main(){{let mut n=0;let weak=made(inout n,true);n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_match",&format!("{O}{R}{MARK}fn made(inout n,b:i32){{match b{{0=>record R{{hole=None;left=1/0;right=0}}|_=>{CONSTRUCTION}}}}} fn main(){{let mut n=0;let weak=made(inout n,1);n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_dormant_internal",&format!("{O}{R}fn main(){{if false{{let weak=if true{{record R{{hole=None;left=1/0;right=0}}}}else{{record R{{hole=None;left=0;right=0}}}};41}}else{{42}}}}"),Some("42"),Some("A0;E1;L1;X0;\n"));
    accepted(&root,"record_root_field_loan",&format!("{O}{R}type P={{n:i32}} {MARK}fn made(inout n){{{CONSTRUCTION}}} fn main(){{let mut p=record P{{n=0}};let weak=made(inout p.n);p.n}}"),Some("12"),Some(ORDER));
    accepted(&root,"record_sibling_read_loan",&format!("{O}{R}type P={{a:i32;b:i32}} fn add(read a:i32,inout b:i32){{b<-b+a;b}} fn made(read a:i32,inout b:i32){{record R{{right=add(a,inout b);hole=None;left=add(a,inout b)}}}} fn main(){{let mut p=record P{{a=21;b=0}};let weak=made(p.a,inout p.b);p.b}}"),Some("42"),None);
    accepted(&root,"record_read_temporary_once",&format!("{O}{R}fn mark(inout n:i32){{n<-n+1;21}} fn add(read a:i32,inout b:i32){{b<-b+a;b}} fn made(inout n:i32,inout b:i32){{record R{{right=add(mark(inout n),inout b);hole=None;left=b}}}} fn main(){{let mut n=0;let mut b=0;let weak=made(inout n,inout b);n+b*2-1}}"),Some("42"),None);
    accepted(&root,"record_mixed_demands",&format!("{O}{R}{MARK}fn made(inout n){{{CONSTRUCTION}}} fn main(){{let mut n=0;let seen=mark(inout n,2);let weak=made(inout n);n-seen*100}}"),Some("12"),Some("A0;E1;A1;E2;M2D2;L2;A1;E2;A2;E3;M1D3;L3;A2;E3;M2D3;L3;L2;L1;X0;\n"));
    for (name, first, last, events) in [
        (
            "record_early_trap",
            "1/0",
            "mark(inout n,2)",
            "A0;E1;A1;E2;rill: runtime trap\nX2;\n",
        ),
        (
            "record_late_trap",
            "mark(inout n,1)",
            "1/0",
            "A0;E1;A1;E2;A2;E3;M1D3;L3;rill: runtime trap\nX2;\n",
        ),
    ] {
        accepted(&root,name,&format!("{O}{R}{MARK}fn made(inout n){{record R{{right={first};hole=None;left={last}}}}} fn main(){{let mut n=0;let weak=made(inout n);n}}"),None,Some(events));
    }
    accepted(&root,"record_diverging_field",&format!("{O}type S<a,b> ={{first:i32;hole:O<b>;stop:a;last:i32}} {MARK}fn spin(){{spin()}} fn made(inout n){{record S{{first=mark(inout n,1);hole=None;stop=spin();last=mark(inout n,2)}}}} fn main(){{let mut n=0;let weak=made(inout n);n}}"),None,None);
    accepted(&root,"record_direct_forever", "type Store<a> ={value:a} fn forever(){forever()} fn main(){let unused=record Store{value=forever()};42}",None,None);
    accepted(&root,"record_interrupted_represented_field",&format!("{O}{R}fn spin(){{spin()}} fn ignore(a){{42}} fn made(){{record R{{right=ignore(spin());hole=None;left=1/0}}}} fn main(){{let weak=made();42}}"),None,None);
    accepted(&root,"record_omitted_suffix",&format!("{O}{R}fn spin(n:i32){{spin(n)+1}} fn ignore(a,b){{42}} fn main(){{ignore(spin(0),record R{{right=0;hole=None;left=1/0}})}}"),None,None);
    for (n, expected) in [(30, Some("42")), (31, None)] {
        let mut events = String::new();
        for depth in 0..32 {
            events.push_str(&format!("A{depth};E{};", depth + 1));
        }
        if n == 30 {
            for depth in (1..=32).rev() {
                events.push_str(&format!("L{depth};"));
            }
            events.push_str("X0;\n");
        } else {
            events.push_str("A32;rill: runtime trap\nX32;\n");
        }
        accepted(&root,&format!("record_depth_{n}"),&format!("{O}type R<a> ={{hole:O<a>}} fn down(n:i32){{if n==0{{record R{{hole=None}}}}else{{down(n-1)}}}} fn main(){{let weak=down({n});42}}"),expected,Some(&events));
    }
    accepted(&root,"record_empty_nested",&format!("{O}type E={{}} type R<a> ={{empty:E;hole:O<a>}} fn made(){{record R{{hole=None;empty=record E{{}}}}}} fn main(){{let weak=made();42}}"),Some("42"),Some("A0;E1;A1;E2;L2;L1;X0;\n"));
    accepted(&root,"record_vector_nested",&format!("{O}type R<a,b> ={{value:a;hole:O<b>}} fn made(){{record R{{hole=None;value=u8x16_load(bytes_new(16),0)}}}} fn main(){{let weak=made();42}}"),Some("42"),None);
    accepted(&root,"record_weak_shadowing",&format!("{O}{R}fn made(){{record R{{hole=None;left=1;right=2}}}} fn main(){{let weak=made();let weak=42;weak}}"),Some("42"),None);
    accepted(&root,"record_observe_and_discard_same_source",&format!("{O}type R<a> ={{hole:O<a>}} fn made(){{record R{{hole=None}}}} fn take(x:R<i32>){{42}} fn main(){{let weak=made();take(made())}}"),Some("42"),None);
    for (name, tail, expected) in [
        ("record_observed_main", "made()", None),
        (
            "record_used_alias",
            "let weak=made();let alias=weak;42",
            Some("42"),
        ),
        ("record_used_result", "let weak=made();weak", None),
        (
            "record_projected_int",
            "let weak=made();weak.left",
            Some("1"),
        ),
        ("record_mutable", "let mut weak=made();42", Some("42")),
        (
            "record_hidden_read",
            "let weak=made();if false{inspect(weak)}else{42}",
            Some("42"),
        ),
        ("record_unknown_argument", "inspect(made())", Some("42")),
    ] {
        boundary(&root,name,&format!("{O}{R}fn made(){{record R{{hole=None;left=1;right=2}}}} fn inspect(read x){{42}} fn main(){{{tail}}}"),expected);
    }
    boundary(&root,"record_projected_shape",&format!("{O}type R<a> ={{hole:O<a>}} fn main(){{let r=record R{{hole=None}};let weak=r.hole;42}}"),Some("42"));
    boundary(&root,"record_unknown_nominal_interruption",&format!("{O}type R<a> ={{hole:O<a>}} fn spin(){{spin()}} fn made(x){{record R{{hole=None}}}} fn main(){{let weak=made(spin());42}}"),None);
    for (name, construction) in [
        ("record_duplicate", "record R{left=1;hole=None;left=2}"),
        ("record_missing", "record R{left=1;hole=None}"),
        ("record_unknown_label", "record R{left=1;hole=None;wrong=2}"),
        ("record_wrong_type", "record R{left=true;hole=None;right=2}"),
    ] {
        checker_error(
            &root,
            name,
            &format!("{O}{R}fn made(){{{construction}}} fn main(){{let weak=made();42}}"),
        );
    }
    checker_error(&root,"record_weak_restriction",&format!("{O}type R<a> ={{hole:O<a>}} fn made(){{record R{{hole=None}}}} fn ints(x:R<i32>){{()}} fn bools(x:R<bool>){{()}} fn main(){{let weak=made();ints(weak);bools(weak);42}}"));
    checker_error(&root,"record_overlap",&format!("{O}{R}fn made(read a:i32,inout b:i32){{record R{{hole=None;left=a;right=b}}}} fn main(){{let mut n=0;let weak=made(n,inout n);42}}"));
    checker_error(&root,"record_hidden_bad_function",&format!("{O}{R}fn bad(){{true+1}} fn made(){{record R{{hole=None;left=1;right=2}}}} fn main(){{let weak=made();42}}"));
    record_case("record_module", || module_records(&root));
    let failures = FAILURES.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(failures, 0, "independent record failures: {failures}");
    println!("COMPLETE independent discarded-record matrix");
}
