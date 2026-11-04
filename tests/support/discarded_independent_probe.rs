//! Independent discarded-result parity and event-trace executable probes.
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
        "static int32_t rl_checked(int op, int32_t x, int32_t y) { if(op==0 && (y==10001 || y==10002 || y==10003)) fprintf(stderr, \"M%dD%u;\", (int)y, rl_call_depth);");
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

const O: &str = "type O<a> = None | Some(a)\n";
fn main() {
    let root = PathBuf::from(
        std::env::var_os("K4_PROBE_OUTPUT").expect("fresh approved RAM output directory required"),
    );
    fs::create_dir(&root).unwrap();
    accepted(
        &root,
        "core",
        &format!("{O}fn absent(){{None}} fn main(){{let weak=absent();42}}"),
        Some("42"),
        Some("A0;E1;A1;E2;L2;L1;X0;\n"),
    );
    accepted(
        &root,
        "forward",
        &format!(
            "{O}fn absent(){{None}} fn forward(){{absent()}} fn main(){{let weak=forward();42}}"
        ),
        Some("42"),
        Some("A0;E1;A1;E2;A2;E3;L3;L2;L1;X0;\n"),
    );
    accepted(&root,"inout_once", &format!("{O}fn absent(inout n:i32){{n<-n+1;None}} fn main(){{let mut n=0;let weak=absent(inout n);n+41}}"),Some("42"),None);
    accepted(&root,"mixed_demands", &format!("{O}type W<a> = W(i32,O<a>) fn mark(inout n:i32,d:i32){{n<-n+d;n}} fn made(inout n:i32){{W(mark(inout n,10002),None)}} fn main(){{let mut n=0;let first=mark(inout n,10001);let weak=made(inout n);n-first-9960}}"),Some("42"),Some("A0;E1;A1;E2;M10001D2;L2;A1;E2;A2;E3;M10002D3;L3;L2;L1;X0;\n"));
    accepted(&root,"constructor_order", &format!("{O}type W<a> = W(i32,O<a>,i32) fn mark(inout n:i32,d:i32){{n<-n+d;n}} fn made(inout n:i32){{W(mark(inout n,10001),None,mark(inout n,10002))}} fn main(){{let mut n=0;let weak=made(inout n);n-19961}}"),Some("42"),Some("A0;E1;A1;E2;A2;E3;M10001D3;L3;A2;E3;M10002D3;L3;L2;L1;X0;\n"));
    accepted(&root,"constructor_trap", &format!("{O}type W<a> = W(i32,O<a>,i32) fn mark(d:i32){{0+d}} fn made(){{W(mark(10001)/0,None,mark(10002))}} fn main(){{let weak=made();42}}"),None,Some("A0;E1;A1;E2;A2;E3;M10001D3;L3;rill: runtime trap\nX2;\n"));
    accepted(&root,"nested_constructor", &format!("{O}type W<a> = W(O<O<a>>) fn absent(){{W(Some(None))}} fn main(){{let weak=absent();42}}"),Some("42"),None);
    accepted(&root,"read_temporary", &format!("{O}fn absent(read n:i32,inout k:i32){{k<-k+n;None}} fn main(){{let mut k=0;let weak=absent(20+22,inout k);k}}"),Some("42"),None);
    accepted(&root,"sibling_field_loans", &format!("{O}type R = {{left:i32;right:i32}} fn absent(read a:i32,inout b:i32){{b<-b+a;None}} fn main(){{let mut r=record R{{right=22;left=20}};let weak=absent(r.left,inout r.right);r.right}}"),Some("42"),None);
    accepted(
        &root,
        "branch_not_taken",
        &format!("{O}fn absent(){{None}} fn main(){{if false{{let weak=absent();41}}else{{42}}}}"),
        Some("42"),
        Some("A0;E1;L1;X0;\n"),
    );
    accepted(&root,"discard_if_match", &format!("{O}fn absent(n:i32){{if n==0{{None}}else{{match n{{1=>Some(None)|_=>None}}}}}} fn main(){{let weak=absent(1);42}}"),Some("42"),None);
    let repeated_events = format!("A0;E1;{}L1;X0;\n", "A1;E2;L2;".repeat(40));
    accepted(&root,"forty_returns", &format!("{O}fn absent(){{None}} fn main(){{let mut n=0;while n<40{{let weak=absent();n<-n+1;}};n+2}}"),Some("42"),Some(&repeated_events));
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
        accepted(&root,&format!("depth_{n}"), &format!("{O}fn down(n:i32){{if n==0{{None}}else{{down(n-1)}}}} fn main(){{let weak=down({n});42}}"),expected,Some(&events));
    }
    for trapping_argument in [false, true] {
        let mut events = "A0;E1;A1;E2;".to_string();
        for depth in 2..=32 {
            events.push_str(&format!("M10003D{depth};"));
            if depth < 32 {
                events.push_str(&format!("A{depth};E{};", depth + 1));
            }
        }
        if !trapping_argument {
            events.push_str("A32;");
        }
        events.push_str("rill: runtime trap\nX32;\n");
        let argument = if trapping_argument {
            "{let next=n-1;let seen=next+10003;if n==1{seen/0}else{next}}"
        } else {
            "{let next=n-1;let seen=next+10003;next}"
        };
        accepted(&root, if trapping_argument {"argument_trap_before_guard"}else{"argument_before_guard"}, &format!("{O}fn down(n:i32){{if n==0{{None}}else{{down({argument})}}}} fn main(){{let weak=down(31);42}}"),None,Some(&events));
    }
    accepted(
        &root,
        "discard_then_nonreturn",
        &format!(
            "{O}fn absent(){{None}} fn spin(){{spin()}} fn main(){{let weak=absent();spin()}}"
        ),
        None,
        None,
    );
    accepted(&root,"interrupted_composition", &format!("{O}fn absent(){{None}} fn spin(){{spin()}} fn ignore(x){{42}} fn main(){{let weak=absent();ignore(spin())}}"),None,None);
    accepted(
        &root,
        "unrelated_payload",
        &format!(
            "{O}type Other<a> = Other(a) fn absent(){{None}} fn main(){{let weak=absent();42}}"
        ),
        Some("42"),
        None,
    );
    accepted(
        &root,
        "shadowed_unused",
        &format!("{O}fn absent(){{None}} fn main(){{let weak=absent();let weak=42;weak}}"),
        Some("42"),
        None,
    );
    for (name, tail, expected) in [
        ("used_result", "let weak=absent();weak", None),
        (
            "used_alias",
            "let weak=absent();let alias=weak;42",
            Some("42"),
        ),
        ("mutable", "let mut weak=absent();42", Some("42")),
        (
            "tag_match",
            "let weak=absent();match weak{None=>42|Some(_)=>0}",
            Some("42"),
        ),
        ("observable_main", "absent()", None),
        (
            "hidden_dead_use",
            "let weak=absent();if false{inspect(weak)}else{42}",
            Some("42"),
        ),
    ] {
        boundary(
            &root,
            name,
            &format!("{O}fn absent(){{None}} fn inspect(x){{42}} fn main(){{{tail}}}"),
            expected,
        );
    }
    boundary(
        &root,
        "unknown_parameter",
        &format!("{O}fn absent(){{None}} fn ignore(x){{42}} fn main(){{ignore(absent())}}"),
        Some("42"),
    );
    boundary(
        &root,
        "observable_none",
        &format!("{O}fn main(){{None}}"),
        None,
    );
    boundary(&root,"layoutless_record",&format!("{O}type R<a> = {{last:i32;weak:O<a>;first:i32}} fn made(){{record R{{first=1;weak=None;last=2}}}} fn main(){{let weak=made();42}}"),Some("42"));
    boundary(&root,"projected_local_shape",&format!("{O}type R<a> = {{value:O<a>}} fn main(){{let r=record R{{value=None}};let weak=r.value;42}}"),Some("42"));
    checker_error(&root,"weak_not_generalized",&format!("{O}fn absent(){{None}} fn ints(x:O<i32>){{()}} fn bools(x:O<bool>){{()}} fn main(){{let weak=absent();ints(weak);bools(weak);42}}"));
    checker_error(&root,"loan_overlap",&format!("{O}fn absent(read a:i32,inout b:i32){{None}} fn main(){{let mut n=0;let weak=absent(n,inout n);42}}"));
    checker_error(&root,"dead_source_error",&format!("{O}fn absent(){{if false{{1+true;None}}else{{None}}}} fn main(){{let weak=absent();42}}"));
    checker_error(
        &root,
        "unused_body_error",
        &format!("{O}fn bad(){{true+1}} fn absent(){{None}} fn main(){{let weak=absent();42}}"),
    );
    checker_error(
        &root,
        "missing_match_arm",
        &format!(
            "{O}fn absent(n:bool){{match n{{true=>None}}}} fn main(){{let weak=absent(true);42}}"
        ),
    );
    checker_error(
        &root,
        "wrong_constructor_arity",
        &format!("{O}fn absent(){{Some(1,2)}} fn main(){{let weak=absent();42}}"),
    );
    accepted(
        &root,
        "internal_if_only",
        &format!("{O}fn main(){{let weak=if true{{None}}else{{None}};42}}"),
        Some("42"),
        Some("A0;E1;L1;X0;\n"),
    );
    accepted(
        &root,
        "internal_match_only",
        &format!("{O}fn main(){{let weak=match true{{true=>None|false=>None}};42}}"),
        Some("42"),
        Some("A0;E1;L1;X0;\n"),
    );
    accepted(
        &root,
        "internal_dormant_only",
        &format!("{O}fn main(){{if false{{let weak=if true{{None}}else{{None}};41}}else{{42}}}}"),
        Some("42"),
        Some("A0;E1;L1;X0;\n"),
    );
    accepted(&root,"two_internal_scopes", &format!("{O}fn side(){{let weak=if true{{None}}else{{None}};1}} fn main(){{let weak=if true{{None}}else{{None}};side()+41}}"),Some("42"),Some("A0;E1;A1;E2;L2;L1;X0;\n"));
    accepted(&root,"bare_nonreturn_operand", &format!("{O}type W<a,b> = W(a,O<b>) fn spin(){{spin()}} fn made(){{W(spin(),None)}} fn main(){{let weak=made();42}}"),None,None);
    let mut interrupted_events = String::new();
    for depth in 0..32 {
        interrupted_events.push_str(&format!("A{depth};E{};", depth + 1));
    }
    interrupted_events.push_str("A32;rill: runtime trap\nX32;\n");
    accepted(&root,"concrete_interrupted_operand", &format!("{O}type W<a> = W(i32,O<a>) fn spin(){{spin()}} fn ignore(x){{0+10002}} fn made(){{W(ignore(spin()),None)}} fn main(){{let weak=made();42}}"),None,Some(&interrupted_events));
    accepted(
        &root,
        "logical_only_interrupted_suffix",
        &format!(
            "{O}fn spin(n:i32){{spin(n)+1}} fn ignore(a,b){{42}} fn main(){{ignore(spin(0),None)}}"
        ),
        None,
        None,
    );
    accepted(&root,"same_source_value_and_discard", &format!("{O}fn absent(){{None}} fn take(x:O<i32>){{42}} fn main(){{let weak=absent();take(absent())}}"),Some("42"),Some("A0;E1;A1;E2;L2;A1;E2;L2;A1;E2;L2;L1;X0;\n"));
    accepted(&root,"shadowed_constructor_parameter", &format!("{O}type W<a> = W(i32,O<a>) fn made(None:i32){{W(None,None())}} fn main(){{let weak=made(42);42}}"),Some("42"),None);
    accepted(&root,"vector_operand_layout", &format!("{O}type W<a,b> = W(a,O<b>) fn made(){{W(u8x16_load(bytes_new(16),0),None)}} fn main(){{let weak=made();42}}"),Some("42"),None);
    accepted(&root,"string_operand_trap", &format!("{O}type W<a> = W(i32,O<a>) fn made(){{W(byte_at(\"x\",1),None)}} fn main(){{let weak=made();42}}"),None,None);
    accepted(&root,"represented_record_written_order", &format!("{O}type R = {{x:i32;y:i32}} type W<a> = W(R,O<a>) fn mark(inout n:i32,d:i32){{n<-n+d;n}} fn made(inout n:i32){{W(record R{{y=mark(inout n,10001);x=mark(inout n,10002)}},None)}} fn main(){{let mut n=0;let weak=made(inout n);n-19961}}"),Some("42"),Some("A0;E1;A1;E2;A2;E3;M10001D3;L3;A2;E3;M10002D3;L3;L2;L1;X0;\n"));
    accepted(&root,"short_circuit_discard_condition", &format!("{O}fn made(){{if false && (1/0==0){{None}}else{{None}}}} fn main(){{let weak=made();42}}"),Some("42"),None);
    boundary(
        &root,
        "mixed_local_shape",
        &format!("{O}fn absent(){{None}} fn main(){{let n=None;let weak=absent();42}}"),
        Some("42"),
    );
    boundary(&root,"hidden_read_loan",&format!("{O}fn absent(){{None}} fn observe(read x){{42}} fn main(){{let weak=absent();if false{{observe(weak)}}else{{42}}}}"),Some("42"));
    checker_error(&root,"recursive_occurs_check",&format!("{O}fn grow(n:i32){{if n==0{{None}}else{{Some(grow(n-1))}}}} fn main(){{let weak=grow(0);42}}"));
    checker_error(
        &root,
        "applied_type_parameter",
        &format!(
            "{O}type Bad<a> = Bad(a<i32>) fn absent(){{None}} fn main(){{let weak=absent();42}}"
        ),
    );
    accepted(&root,"retained_k1_weak_mutable", "type Flag<a> = Off | On fn flag(n:bool){if n{On}else{Off}} fn main(){let mut f=flag(false);f<-flag(true);match f{On=>42|Off=>0}}",Some("42"),None);
    accepted(&root,"retained_k2_fixed_product", "type Flag<a> = Off | On type Stamp<a> = Stamp(Flag<a>,i32) fn make(n:i32){Stamp(Off,n)} fn main(){let s=make(42);match s{Stamp(_,x)=>x}}",Some("42"),None);
    println!("ALL REQUESTED INDEPENDENT PUBLIC PROBES PASSED");
    assert_eq!(
        FAILURES.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "independent probe failures above"
    );
}
