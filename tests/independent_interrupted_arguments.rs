//! Independent K3b source, C11, event, and retained-boundary regressions.
use rill::{backend, check, interpreter, parser, typed};
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
        let root = std::env::var_os("RILL_INDEPENDENT_ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = root.join(format!(
            "k3b-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn command(&self, command: &mut Command) -> Output {
        let out = self.0.join("stdout");
        let err = self.0.join("stderr");
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&out).unwrap())
            .stderr(fs::File::create(&err).unwrap());
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("timeout is not a Rill trap");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Output {
            status,
            stdout: fs::read(out).unwrap(),
            stderr: fs::read(err).unwrap(),
        }
    }
    fn c(&self, c: &str, value: Option<&str>, trace: &str) {
        let source = self.0.join("case.c");
        let binary = self.0.join("case");
        fs::write(&source, c).unwrap();
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        for opt in ["-O0", "-O2"] {
            let output = self.command(
                Command::new(&compiler)
                    .args(["-std=c11", "-pedantic-errors", opt])
                    .arg(&source)
                    .arg("-o")
                    .arg(&binary),
            );
            assert!(
                output.status.success(),
                "{} {opt}: {}",
                compiler.to_string_lossy(),
                String::from_utf8_lossy(&output.stderr)
            );
            fs::write(self.0.join(format!("compile-{opt}.stderr")), &output.stderr).unwrap();
            if !output.stderr.is_empty() {
                let diagnostics = String::from_utf8_lossy(&output.stderr);
                let warnings: Vec<_> = diagnostics
                    .lines()
                    .filter(|line| line.contains(" warning:"))
                    .collect();
                // The unchanged Copy-pattern emitter wraps its equality test
                // before the surrounding if adds parentheses. Retain Clang's
                // known stylistic warning, but reject any other diagnostic.
                assert!(
                    !warnings.is_empty()
                        && warnings
                            .iter()
                            .all(|line| line.ends_with("[-Wparentheses-equality]")),
                    "unexpected C diagnostics: {diagnostics}"
                );
                eprintln!(
                    "retained known Clang parentheses warning at {} ({opt})",
                    self.0.display()
                );
            }
            let output = self.command(&mut Command::new(&binary));
            match value {
                Some(value) => {
                    assert_eq!(output.status.code(), Some(0));
                    assert_eq!(output.stdout, format!("{value}\n").as_bytes());
                    assert_eq!(output.stderr, trace.as_bytes());
                }
                None => {
                    assert_eq!(
                        output.status.code(),
                        Some(1),
                        "signal or host-stack crash is not a language trap"
                    );
                    assert!(output.stdout.is_empty());
                    assert_eq!(
                        output.stderr,
                        format!("{trace}rill: runtime trap\n").as_bytes()
                    );
                }
            }
        }
    }
}
fn lower(source: &str) -> (rill::ast::Program, typed::TypedMain, String) {
    let ast = parser::parse(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let checked = check::analyze(&ast).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let t = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{e}\n{source}"));
    typed::verify(&t).unwrap();
    let c = typed::emit_c(&t).unwrap();
    assert_eq!(backend::emit(&ast).unwrap(), c);
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
    assert!(!c.contains("__builtin_unreachable") && !c.contains("_Noreturn static void rl_fn_"));
    for line in c.lines().filter(|l| l.starts_with("static void rl_fn_")) {
        let name = line
            .trim_start_matches("static void ")
            .split('(')
            .next()
            .unwrap();
        assert!(!c.contains(&format!("= {name}(")), "fabricated void result");
    }
    (ast, t, c)
}
fn compare(source: &str, expected: Option<&str>) -> String {
    let (ast, t, c) = lower(source);
    let result = typed::evaluate(&t);
    match expected {
        Some(value) => assert_eq!(result.as_ref().unwrap().to_string(), value),
        None => assert_eq!(result.as_ref().unwrap_err(), "runtime trap"),
    }
    if check::analyze(&ast).unwrap().has_owned() {
        assert_eq!(
            interpreter::run_reference(&ast).unwrap_err(),
            "E-REFERENCE: owner programs require the checked typed path"
        );
    } else {
        assert_eq!(interpreter::run_reference(&ast), result);
    }
    assert_eq!(interpreter::run(&ast), result);
    Fixture::new().c(&c, expected, "");
    c
}
#[test]
fn independent_interrupted_sources_and_ordinary_controls() {
    for source in [
        "fn spin(){spin()} fn ignore(x){42} fn main(){ignore(spin())}",
        "fn spin(){spin()} fn ignore(x){42} fn main(){ignore(ignore(spin()))}",
        "fn spin(){spin()} fn identity(x){x} fn main(){identity(spin())}",
        "fn spin(){spin()} fn choose(a,b,c){a} fn main(){choose(42,spin(),7)}",
        "type O<a> = None | Some(a) fn spin(){spin()} fn ignore(a,b){42} fn main(){ignore(spin(),None)}",
        "type Unused<a> = Empty | Full(a) extern fn keep(x:i32)->i32; fn spin(){spin()} fn ignore(x){42} fn main(){ignore(spin())}",
        "fn spin(n:i32){spin(n)} fn ignore(x){42} fn main(){ignore(spin(0))}",
        "fn loop(n:i32){loop(n)} fn sink(read x:i32,y:i32){y} fn spin(){spin()} fn main(){if false {let unused=spin();42} else {sink(0,loop(0))}}",
        "fn loop(n:i32){loop(n)} fn sink(inout x:i32,y:i32){x<-y;42} fn spin(){spin()} fn main(){let mut n=0;if false {let unused=spin();42} else {sink(inout n,loop(0))}}",
    ] { compare(source, None); }
    for source in [
        "fn spin(){spin()} fn ignore(x){42} fn main(){if false {ignore(spin())} else {42}}",
        "fn spin(){spin()} fn ignore(x){42} fn main(){match false {true=>ignore(ignore(spin())) | false=>42}}",
        "type Flag<a> = Off | On fn choose(x){match x {Off=>42 | On=>7}} fn main(){choose(Off)}",
        "type Flag<a> = Off | On type Wrap<a> = {flag: Flag<a>; count: i32} fn main(){let p=record Wrap {flag=Off;count=42};p.count}",
        "fn main(){let b=system_box_i32_abort(42);box_read(b)}",
    ] { compare(source, Some("42")); }
}
#[test]
fn independent_unexecuted_source_still_checks_first() {
    for source in [
        "fn spin(){spin()} fn ignore(a,b){42} fn main(){ignore(spin(),true+1)}",
        "fn spin(){spin()} fn ignore(a,b){42} fn main(){ignore(spin(),match true {true=>42})}",
        "fn spin(){spin()} fn ignore(a,b){42} fn main(){ignore(spin(),missing)}",
        "fn bad(inout x,read y){x<-y} fn spin(){spin()} fn ignore(a,b){42} fn main(){let mut n=0;ignore(spin(),bad(inout n,n))}",
        "type Bad = {value: Bad} fn spin(){spin()} fn ignore(x){42} fn main(){ignore(spin())}",
    ] {
        let ast = parser::parse(source).unwrap(); let error = check::analyze(&ast).err().expect(source);
        assert_eq!(interpreter::run(&ast).unwrap_err(), error); assert_eq!(backend::emit(&ast).unwrap_err(), error);
    }
}
#[test]
fn independent_genuine_layout_and_mode_boundaries_remain() {
    for source in [
        "fn spin(){spin()} fn main(){let used=spin();used}",
        "fn spin(){spin()} fn main(){let mut unused=spin();42}",
        "type O<a> = None | Some(a) fn absent(){None} fn main(){let unused=absent();42}",
        "type O<a> = None | Some(a) fn spin(){spin()} fn ignore(a,b){42} fn main(){ignore(None,spin())}",
        "fn spin(){spin()} fn ignore(read a,b){42} fn main(){let n=0;ignore(n,spin())}",
        "fn spin(){spin()} fn ignore(inout a,b){42} fn main(){let mut n=0;ignore(inout n,spin())}",
    ] {
        let ast = parser::parse(source).unwrap(); let checked = check::analyze(&ast).unwrap_or_else(|e| panic!("source acceptance changed: {e}\n{source}"));
        assert!(typed::lower_main(&checked).is_err(), "unsupported form admitted: {source}");
    }
}
fn replace(c: String, needle: &str, replacement: &str) -> String {
    assert_eq!(
        c.matches(needle).count(),
        1,
        "instrumentation anchor: {needle}"
    );
    c.replacen(needle, replacement, 1)
}
fn instrument(c: String) -> String {
    let c = replace(
        c,
        "_Noreturn static void rl_trap(void) {",
        r#"
static volatile unsigned en, at, dep, pre, spin, arg, suffix, body, leaves, divzero;
_Noreturn static void rl_trap(void) {
 fprintf(stderr,"K3b en=%u at=%u dep=%u pre=%u spin=%u arg=%u suffix=%u body=%u leaves=%u divzero=%u\n",en,at,dep,pre,spin,arg,suffix,body,leaves,divzero);
"#,
    );
    let c=replace(c,"static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }","static void rl_enter(void) { ++at; if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; ++en; dep=rl_call_depth; }");
    let c = replace(
        c,
        "static void rl_leave(void) { --rl_call_depth; }",
        "static void rl_leave(void) { ++leaves; --rl_call_depth; dep=rl_call_depth; }",
    );
    replace(
        c,
        "static int32_t rl_checked(int op, int32_t x, int32_t y) {",
        r#"
static int32_t rl_checked(int op, int32_t x, int32_t y) {
 if(op==0 && y==11) ++pre;
 if(op==0 && y==100) ++spin;
 if(op==0 && y==1) ++arg;
 if(op==0 && y==10000) ++suffix;
 if(op==2 && x==5 && y==7) ++body;
 if(op==3 && y==0) ++divzero;
"#,
    )
}
#[test]
fn independent_exact_main_inclusive_interruption_events() {
    for (prefix, barrier_arg, nested, entries, attempts, spins, args, divides) in [
        ("7+11", "n+1", false, 32, 33, 31, 31, 0),
        ("7+11", "n+1", true, 32, 33, 31, 31, 0),
        (
            "7+11",
            "{let next=n+1;if n==30 {next/0} else {next}}",
            false,
            32,
            32,
            31,
            31,
            1,
        ),
        ("{let mark=7+11;1/0}", "n+1", false, 1, 1, 0, 0, 1),
    ] {
        let call = format!("ignore({prefix},spin(0),9+10000)");
        let call = if nested {
            format!("wrap({call})")
        } else {
            call
        };
        let source=format!("fn spin(n:i32){{let marker=n+100;spin({barrier_arg})}} fn ignore(a,b,c){{5*7}} fn wrap(x){{3*9}} fn main(){{{call}}}");
        let (ast, t, c) = lower(&source);
        assert_eq!(typed::evaluate(&t).unwrap_err(), "runtime trap");
        assert_eq!(
            interpreter::run_reference(&ast).unwrap_err(),
            "runtime trap"
        );
        let trace=format!("K3b en={entries} at={attempts} dep={entries} pre=1 spin={spins} arg={args} suffix=0 body=0 leaves=0 divzero={divides}\n");
        Fixture::new().c(&instrument(c), None, &trace);
    }
}
#[test]
fn independent_concrete_native_prefix_stays_ordinary_contextual_boundary_stays_rejected() {
    let source =
        "fn spin(){spin();42} fn sink(p:ptr<void>,x:i32){x} fn main(){sink(c_null(),spin())}";
    let (ast, typed, c) = lower(source);
    let raw = "E-FFI: raw C values require native C emission";
    assert_eq!(typed::evaluate(&typed).unwrap_err(), raw);
    assert_eq!(interpreter::run_reference(&ast).unwrap_err(), raw);
    assert_eq!(interpreter::run(&ast).unwrap_err(), raw);
    Fixture::new().c(&c, None, "");
    let contextual="fn spin(){spin();42} fn forever(){forever()} fn sink(p:ptr<void>,x:i32){x} fn main(){if false {let unused=forever();42} else {sink(c_null(),spin())}}";
    let ast = parser::parse(contextual).unwrap();
    let checked = check::analyze(&ast).unwrap();
    assert!(typed::lower_main(&checked)
        .unwrap_err()
        .contains("pure Copy"));
}
