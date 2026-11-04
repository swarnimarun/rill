//! Independently designed closed-signature tests. Preserve every C artifact.
#[path = "support/native_launch.rs"]
mod native_launch;

use rill::{ast::TypeExpr, backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[path = "support/result_annotation_compatibility.rs"]
mod compatibility;

static NEXT: AtomicUsize = AtomicUsize::new(0);
type Consumer = fn(&rill::ast::Program) -> Result<(), String>;
const CONSUMERS: [Consumer; 6] = [
    check::check,
    |p| check::analyze(p).map(|_| ()),
    |p| interpreter::run(p).map(|_| ()),
    |p| interpreter::run_reference(p).map(|_| ()),
    |p| backend::emit(p).map(|_| ()),
    |p| backend::emit_reference(p).map(|_| ()),
];
const BUDGET_ERROR: &str = "E-TYPE-COMPLEXITY: signature annotation input budget exceeded";

fn fixture(label: &str) -> PathBuf {
    let base = std::env::var_os("RILL_SIGNATURE_EVIDENCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    loop {
        let dir = base.join(format!(
            "independent-signature-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("fixture: {e}"),
        }
    }
}

fn output(dir: &Path, command: &mut Command, label: &str, seconds: u64) -> Output {
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let out = dir.join(format!("{serial}-{label}.out"));
    let err = dir.join(format!("{serial}-{label}.err"));
    command
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap());
    let status = native_launch::status(command, dir, Duration::from_secs(seconds))
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    Output {
        status,
        stdout: fs::read(out).unwrap(),
        stderr: fs::read(err).unwrap(),
    }
}

fn guard(dir: &Path) {
    if let Some(path) = std::env::var_os("K4_RESOURCE_GUARD") {
        let result = output(dir, Command::new("bash").arg(path), "guard", 5);
        if !result.status.success() {
            eprintln!(
                "resource guard: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            std::process::exit(86);
        }
    }
}

fn c_runs(dir: &Path, label: &str, c: &str, expected: &str) {
    let source = dir.join(format!("{label}.c"));
    fs::write(&source, c).unwrap();
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    for opt in ["-O0", "-O2"] {
        let binary = dir.join(format!("{label}-{}", &opt[1..]));
        guard(dir);
        let compiled = output(
            dir,
            Command::new(&cc)
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(&source)
                .arg("-o")
                .arg(&binary),
            "compile",
            30,
        );
        assert!(
            compiled.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        if !compiled.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&compiled.stderr));
        }
        guard(dir);
        let ran = output(dir, &mut Command::new(binary), "execute", 10);
        assert_eq!(ran.status.code(), Some(0));
        assert_eq!(ran.stdout, format!("{expected}\n").as_bytes());
        assert_eq!(ran.stderr, b"");
        eprintln!(
            "INDEPENDENT_SIGNATURE_C_PASS {} {label} {opt}",
            cc.to_string_lossy()
        );
    }
}

fn pure_program(dir: &Path, program: &rill::ast::Program, expected: &str) {
    let checked = check::analyze(program).unwrap();
    let ir = typed::lower_main(&checked).unwrap();
    typed::verify(&ir).unwrap();
    for actual in [
        typed::evaluate(&ir),
        interpreter::run(program),
        interpreter::run_reference(program),
    ] {
        assert_eq!(actual.unwrap().to_string(), expected);
    }
    let c = typed::emit_c(&ir).unwrap();
    assert_eq!(backend::emit(program).unwrap(), c);
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
    c_runs(dir, "typed", &c, expected);
    c_runs(
        dir,
        "reference",
        &backend::emit_reference(program).unwrap(),
        expected,
    );
}

fn pure(label: &str, source: &str, expected: &str) {
    let dir = fixture(label);
    fs::write(dir.join("main.rill"), source).unwrap();
    let program = parser::parse(source).unwrap();
    pure_program(&dir, &program, expected);
}

#[test]
fn independent_mixed_scc_schemes_and_all_six_orders() {
    use check::Type::{Int, Nominal, Var};
    let declarations = [
        "fn f(p,x)->i32{g(p,x);p.x}",
        "fn g(p:P,x)->i32{if false{let q=h(p,x);q.x}else{0}}",
        "fn h(p,x)->P{f(p,x);p}",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let source = format!(
            "type P={{x:i32}} {} {} {} fn main(){{f(record P{{x=42}},true)+f(record P{{x=0}},1)}}",
            declarations[order[0]], declarations[order[1]], declarations[order[2]]
        );
        let program = parser::parse(&source).unwrap();
        let checked = check::analyze(&program).unwrap();
        for name in ["f", "g", "h"] {
            assert_eq!(
                compatibility::canonical_signature(&checked.functions[name]),
                (
                    vec![0],
                    vec![Nominal("P".into(), vec![]), Var(0)],
                    if name == "h" {
                        Nominal("P".into(), vec![])
                    } else {
                        Int
                    }
                ),
                "{name}, {order:?}"
            );
        }
        pure(
            &format!("mixed-{}{}{}", order[0], order[1], order[2]),
            &source,
            "42",
        );
    }
}

#[test]
fn independent_original_slot_and_snapshot_effects_keep_exact_observations() {
    for (label, prefix, f, g, main, expected) in [
        (
            "original-slot",
            "type P={x:i32;y:i32} type Pair={a:P;b:P} fn replace(inout p:P)->i32{p<-record P{x=4;y=7};42}",
            "fn f(inout p)->i32{g(inout p);p.y*100+p.x}",
            "fn g(inout p:P)->unit{if false{f(inout p);()}else{p.x<-replace(inout p);()}}",
            "fn main(){let mut pair=record Pair{a=record P{x=1;y=2};b=record P{x=23;y=99}};let answer=f(inout pair.a);answer*100+pair.b.x}",
            "74223",
        ),
        (
            "value-and-computed-read",
            "type P={x:i32;y:i32}",
            "fn f(value,read snapshot,inout p)->i32{g(value,snapshot,inout p);value.x*10000+snapshot.x*100+p.x}",
            "fn g(value:P,read snapshot:P,inout p:P)->unit{if false{f(value,snapshot,inout p);()}else{p<-record P{x=7;y=8};()}}",
            "fn main(){let mut p=record P{x=2;y=9};f(p,if true{p}else{p},inout p)}",
            "20207",
        ),
        (
            "argument-and-body-order",
            "type P={x:i32} fn mark(inout log,digit)->P{log<-log*10+digit;record P{x=digit}}",
            "fn f(value,read snapshot,inout p,inout log)->i32{g(value,snapshot,inout p,inout log);value.x*100+snapshot.x*10+p.x}",
            "fn g(value:P,read snapshot:P,inout p:P,inout log)->unit{if false{f(value,snapshot,inout p,inout log);()}else{log<-log*10+3;p.x<-value.x+snapshot.x;()}}",
            "fn main(){let mut log=0;let mut p=record P{x=0};let answer=f(mark(inout log,1),mark(inout log,2),inout p,inout log);log*1000+answer}",
            "123123",
        ),
    ] {
        for functions in [format!("{f} {g}"), format!("{g} {f}")] {
            pure(label, &format!("{prefix} {functions} {main}"), expected);
        }
    }
}

#[test]
fn independent_results_then_all_parameters_then_bodies_error_priority() {
    for (source, expected) in [
        (
            "fn f(p:MissingParameter)->i32{g(p)} fn g(p)->MissingResult{f(p)} fn main(){42}",
            "E-TYPE: unknown type 'MissingResult'",
        ),
        (
            "fn f(p)->i32{g(p);p.x} fn g(p:MissingParameter)->i32{f(p)} fn main(){42}",
            "E-TYPE: unknown type 'MissingParameter'",
        ),
    ] {
        let program = parser::parse(source).unwrap();
        for consumer in CONSUMERS {
            assert_eq!(consumer(&program).unwrap_err(), expected);
        }
    }
}

#[test]
fn independent_negative_modes_and_logical_identities_remain_checked() {
    for (source, code) in [
        ("type P={x:i32} fn f(read p)->i32{g(inout p);p.x} fn g(inout p:P)->i32{if false{f(p)}else{0}} fn main(){42}", "E-LOAN"),
        ("type P={x:i32} fn f(inout p)->i32{g(inout p);p.x} fn g(inout p:P)->i32{if false{f(inout p)}else{0}} fn main(){let p=record P{x=42};f(inout p)}", "E-LOAN"),
        ("type P={x:i32} fn f(inout p)->i32{g(p);p.x} fn g(inout p:P)->i32{if false{f(inout p)}else{0}} fn main(){42}", "E-LOAN"),
        ("type N<a> =Tag fn f(p:N<bool>)->i32{g(p)} fn g(p:N<i32>)->i32{if false{f(Tag)}else{0}} fn main(){42}", "E-TYPE"),
        ("fn f(p:ptr<Handle>)->bool{g(p)} fn g(p:const_ptr<Handle>)->bool{if false{f(c_null())}else{true}} fn main(){42}", "E-TYPE"),
        ("fn f(p:ptr<Handle>)->bool{g(p)} fn g(p:ptr<Other>)->bool{if false{f(c_null())}else{true}} fn main(){42}", "E-TYPE"),
        ("type O<a> =None|Some(a) fn f(p:i32)->i32{g(p);let mut slot=None;let alias=slot;slot<-Some(p);let bad=if true{alias}else{Some(true)};42} fn g(p:i32)->i32{if false{f(p)}else{0}} fn main(){42}", "E-TYPE"),
    ] {
        let program = parser::parse(source).unwrap();
        for consumer in CONSUMERS {
            let error = consumer(&program).unwrap_err();
            assert!(error.starts_with(code), "{source}: {error}");
        }
    }
    assert_eq!(
        compatibility::fingerprints(),
        include_str!("support/result_annotation_schemes.txt")
    );
}

fn nested(depth: usize) -> TypeExpr {
    let mut tree = TypeExpr {
        name: "i32".into(),
        args: vec![],
    };
    for _ in 1..depth {
        tree = TypeExpr {
            name: "W".into(),
            args: vec![tree],
        };
    }
    tree
}

fn dismantle(tree: TypeExpr) {
    let mut pending = vec![tree];
    while let Some(mut node) = pending.pop() {
        pending.append(&mut node.args);
    }
}

#[test]
fn independent_every_public_route_admits_later_parameters_before_recursion() {
    for depth in [127, 128, 129, 20_000] {
        let mut program = parser::parse(
            "type W<a> =Wrap(a) fn dormant(first:i32,deep)->i32{dormant(first,deep)} fn main(){42}",
        )
        .unwrap();
        program.functions[0].params[1].annotation = Some(nested(depth));
        let results: Vec<_> = CONSUMERS.iter().map(|consume| consume(&program)).collect();
        dismantle(program.functions[0].params[1].annotation.take().unwrap());
        for result in results {
            if depth <= 128 {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err(), BUDGET_ERROR);
            }
        }
    }
    for pointer in ["ptr", "const_ptr"] {
        for depth in [128, 129, 20_000] {
            let mut program = parser::parse("fn dormant(x)->i32{0} fn main(){42}").unwrap();
            program.functions[0].params[0].annotation = Some(TypeExpr {
                name: pointer.into(),
                // Neither an early malformed arity nor an earlier sibling may
                // hide an arbitrarily deep later raw subtree from admission.
                args: vec![nested(1), nested(depth - 1)],
            });
            let results: Vec<_> = CONSUMERS.iter().map(|consume| consume(&program)).collect();
            dismantle(program.functions[0].params[0].annotation.take().unwrap());
            for result in results {
                assert_eq!(
                    result.unwrap_err(),
                    if depth <= 128 {
                        "E-FFI: pointer types require one C pointee name"
                    } else {
                        BUDGET_ERROR
                    }
                );
            }
        }
    }
}

#[test]
fn independent_imported_nested_parameter_paths_keep_resolved_identity() {
    use rill::{
        modules::compile::{load_program, load_program_snapshot, load_program_with_sources},
        source_map::{AstNode, AstStep},
    };
    let f = "fn f(p)->api.P{g(p);match p{api.None=>record api.P{x=0}|api.Some(q)=>q}}";
    let g = "fn g(api:api.O<api.P>)->i32{if false{let q=f(api);q.x}else{0}}";
    for functions in [format!("{f} {g}"), format!("{g} {f}")] {
        let dir = fixture("module-origins");
        fs::write(
            dir.join("api.rill"),
            "export type O<a> =None|Some(a) export type P={x:i32}",
        )
        .unwrap();
        fs::write(
            dir.join("main.rill"),
            format!(
                "import api; {functions} fn main(){{let q=f(api.Some(record api.P{{x=42}}));q.x}}"
            ),
        )
        .unwrap();
        let entry = dir.join("main.rill");
        let program = load_program(&entry).unwrap();
        assert_eq!(load_program_snapshot(&entry).unwrap().program(), &program);
        let mapped = load_program_with_sources(&entry).unwrap();
        assert_eq!(mapped.program(), &program);
        let g_symbol = mapped
            .symbols()
            .iter()
            .find(|s| s.symbol.name == "g")
            .unwrap();
        let parameter = g_symbol
            .target
            .child(AstStep::Parameter(0))
            .child(AstStep::Annotation);
        let argument = parameter.child(AstStep::TypeArgument(0));
        for (path, spelling) in [(&parameter, "api.O<api.P>"), (&argument, "api.P")] {
            let origin = mapped.origin(path).unwrap();
            assert_eq!(mapped.origin_text(&origin.origins[0]), Some(spelling));
            assert_eq!(origin.origins[0].path.steps, path.steps);
            assert!(matches!(
                mapped.node(path),
                Some(AstNode::TypeExpression(_))
            ));
            assert!(!path.steps.contains(&AstStep::FunctionResult));
        }
        let f_symbol = mapped
            .symbols()
            .iter()
            .find(|s| s.symbol.name == "f")
            .unwrap();
        let result = f_symbol.target.child(AstStep::FunctionResult);
        assert_eq!(
            mapped.origin_text(&mapped.origin(&result).unwrap().origins[0]),
            Some("api.P")
        );
        let Some(AstNode::TypeExpression(inner)) = mapped.node(&argument) else {
            panic!("nested parameter type")
        };
        let Some(AstNode::TypeExpression(ret)) = mapped.node(&result) else {
            panic!("result type")
        };
        assert_eq!(inner, ret);
        assert_ne!(argument, result);
        pure_program(&dir, &program, "42");
    }
}

#[test]
fn independent_module_nominal_and_private_parameters_reject_in_all_loaders() {
    use rill::modules::compile::{load_program, load_program_snapshot, load_program_with_sources};
    for (api, second, main, code) in [
        (
            "export type P={x:i32}",
            "export type P={x:i32}",
            "import api; import second; fn f(p:api.P)->i32{g(p)} fn g(p:second.P)->i32{if false{f(record api.P{x=0})}else{0}} fn main(){42}",
            "E-MODULE-CHECK",
        ),
        (
            "type Hidden=Secret export type Phantom<a> =Empty export fn hidden(x:Phantom<Hidden>)->i32{0}",
            "fn unrelated(){0}",
            "import api; fn main(){42}",
            "E-MODULE-PRIVATE-TYPE",
        ),
        (
            "type Hidden=Secret export fn visible(){42}",
            "fn unrelated(){0}",
            "import api; fn unused(p:api.Hidden)->i32{0} fn main(){42}",
            "E-MODULE-PRIVATE",
        ),
    ] {
        let dir = fixture("module-reject");
        fs::write(dir.join("api.rill"), api).unwrap();
        fs::write(dir.join("second.rill"), second).unwrap();
        fs::write(dir.join("main.rill"), main).unwrap();
        let entry = dir.join("main.rill");
        let ordinary = load_program(&entry).unwrap_err();
        let snapshot = load_program_snapshot(&entry).err().unwrap();
        let mapped = load_program_with_sources(&entry).unwrap_err();
        assert_eq!(ordinary.code, code, "{ordinary}");
        assert_eq!((snapshot.code, &snapshot.message), (ordinary.code, &ordinary.message));
        assert_eq!((mapped.code, mapped.message), (ordinary.code, ordinary.message));
    }
}
