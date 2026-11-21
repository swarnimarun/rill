//! Independently authored annotation acceptance probes; retain generated evidence.
use rill::{ast::TypeExpr, backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
static RESOURCE_FAILED: AtomicBool = AtomicBool::new(false);
fn fixture() -> PathBuf {
    loop {
        let path = std::env::temp_dir().join(format!(
            "rill-independent-result-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("fixture: {error}"),
        }
    }
}

fn compile_c(dir: &std::path::Path, c: &str, expected: Option<&str>, success: bool) {
    assert!(!RESOURCE_FAILED.load(Ordering::SeqCst));
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let source = dir.join("output.c");
    fs::write(&source, c).unwrap();
    for opt in ["-O0", "-O2"] {
        if let Some(guard) = std::env::var_os("K4_RESOURCE_GUARD") {
            if !Command::new("bash").arg(guard).status().unwrap().success() {
                RESOURCE_FAILED.store(true, Ordering::SeqCst);
                eprintln!("resource guard failed; stopping the complete test process");
                std::process::exit(86);
            }
        }
        let binary = dir.join(&opt[1..]);
        let built = Command::new(&cc)
            .args(["-std=c11", "-pedantic-errors", opt, "-I"])
            .arg(dir)
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        fs::write(
            dir.join(format!("{}-compile.out", &opt[1..])),
            &built.stdout,
        )
        .unwrap();
        fs::write(
            dir.join(format!("{}-compile.err", &opt[1..])),
            &built.stderr,
        )
        .unwrap();
        assert_eq!(
            built.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        if !success {
            eprintln!("INDEPENDENT_HEADER_REJECT {} {opt}", cc.to_string_lossy());
            continue;
        }
        if !built.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&built.stderr));
        }
        let output = Command::new(binary).output().unwrap();
        fs::write(dir.join(format!("{}-run.out", &opt[1..])), &output.stdout).unwrap();
        fs::write(dir.join(format!("{}-run.err", &opt[1..])), &output.stderr).unwrap();
        assert_eq!(
            output.status.code(),
            Some(if expected.is_some() { 0 } else { 1 })
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected.map(|v| format!("{v}\n")).unwrap_or_default()
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            if expected.is_some() {
                ""
            } else {
                "rill: runtime trap\n"
            }
        );
        eprintln!("INDEPENDENT_RESULT_C_PASS {} {opt}", cc.to_string_lossy());
    }
}
fn pure(source: &str, expected: &str, reference_c: bool) {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let ir = typed::lower_main(&checked).unwrap();
    typed::verify(&ir).unwrap();
    for value in [
        typed::evaluate(&ir),
        interpreter::run(&program),
        interpreter::run_reference(&program),
    ] {
        assert_eq!(value.unwrap().to_string(), expected);
    }
    let c = typed::emit_c(&ir).unwrap();
    assert_eq!(c, backend::emit(&program).unwrap());
    assert!(!c.contains("rl_value"));
    let dir = fixture();
    fs::write(dir.join("main.rill"), source).unwrap();
    compile_c(&dir, &c, Some(expected), true);
    if reference_c {
        compile_c(
            &fixture(),
            &backend::emit_reference(&program).unwrap(),
            Some(expected),
            true,
        );
    }
}
#[test]
fn independent_result_mixed_scc_all_six_declaration_orders() {
    let declarations = [
        "fn a(n)->i32{let p=b(n);p.x}",
        "fn b(n)->P{c(n)}",
        "fn c(n){if n==0{record P{x=42}}else{record P{x=a(n-1)}}}",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        pure(
            &format!(
                "type P={{x:i32}} {} {} {} fn main()->i32{{a(3)}}",
                declarations[order[0]], declarations[order[1]], declarations[order[2]]
            ),
            "42",
            true,
        );
    }
}
#[test]
fn independent_result_contextual_nested_phantom_and_monomorphic_constraints() {
    pure("type O<a> =None|Some(a) type M<a> =Mark type R<a> ={m:M<a>;n:i32} fn absent(){None} fn make()->R<bool>{record R{n=42;m=Mark}} fn main()->i32{let ignored=absent();let r=make();r.n}", "42", false);
    for source in [
        "type O<a> =None|Some(a) fn absent(){None} fn main()->i32{let weak=absent();match weak{None=>42|Some(_)=>0}}",
        "fn spin(){spin()} fn main()->i32{let mut value=spin();42}",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        assert!(typed::lower_main(&checked).is_err());
    }
    for source in [
        "type M<a> =Mark fn hidden()->M<SystemBoxI32>{Mark} fn generic(x){x} fn main(){42}",
        "type M<a> =Mark extern fn unused()->i32; fn hidden()->M<SystemBytes>{Mark} fn main(){42}",
    ] {
        assert!(check::check(&parser::parse(source).unwrap())
            .unwrap_err()
            .starts_with("E-OWNED"));
    }
}
#[test]
fn independent_result_public_ast_preflight_covers_every_consumer() {
    type Consumer = fn(&rill::ast::Program) -> Result<(), String>;
    let consumers: [Consumer; 6] = [
        check::check,
        |p| check::analyze(p).map(|_| ()),
        |p| interpreter::run(p).map(|_| ()),
        |p| interpreter::run_reference(p).map(|_| ()),
        |p| backend::emit(p).map(|_| ()),
        |p| backend::emit_reference(p).map(|_| ()),
    ];
    for depth in [127, 128, 129, 10_000] {
        let mut program =
            parser::parse("type W<a> =Wrap(a) fn dormant(){dormant()} fn main(){42}").unwrap();
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
        program.functions[0].result_annotation = Some(tree);
        for consumer in consumers {
            let result = consumer(&program);
            if depth > 128 {
                assert_eq!(
                    result.unwrap_err(),
                    "E-TYPE-COMPLEXITY: signature annotation input budget exceeded"
                );
            } else {
                assert!(result.is_ok(), "depth {depth}: {result:?}");
            }
        }
        let mut pending = vec![program.functions[0].result_annotation.take().unwrap()];
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.args);
        }
    }
}
#[test]
fn independent_result_native_manifest_and_unused_header_parity() {
    let prefix = "@include(\"native.h\"); extern fn unseen()->u32; extern fn raw()->ptr<Handle>; ";
    let inferred = format!("{prefix}fn wrap(){{raw()}} fn main(){{42}}");
    let annotated = format!("{prefix}fn wrap()->ptr<Handle>{{raw()}} fn main()->i32{{42}}");
    let c: Vec<_> = [&inferred, &annotated]
        .iter()
        .map(|s| {
            let p = parser::parse(s).unwrap();
            let checked = check::analyze(&p).unwrap();
            let ir = typed::lower_main(&checked).unwrap();
            assert_eq!(typed::evaluate(&ir).unwrap().to_string(), "42");
            assert_eq!(interpreter::run_reference(&p).unwrap().to_string(), "42");
            let c = typed::emit_c(&ir).unwrap();
            assert_eq!(backend::emit(&p).unwrap(), c);
            c
        })
        .collect();
    assert_eq!(c[0], c[1], "annotations changed unused native obligations");
    for bad in [false, true] {
        let dir = fixture();
        fs::write(dir.join("native.h"),format!("#include <stdint.h>\ntypedef struct Handle Handle;\n{} unseen(void);\nHandle *raw(void);\n",if bad{"int32_t"}else{"uint32_t"})).unwrap();
        compile_c(&dir, &c[1], Some("42"), !bad);
    }
    let prefix = "@include(\"native.h\"); extern fn raw()->ptr<Handle>; ";
    let inferred = format!("{prefix}fn wrap(){{raw()}} fn main(){{is_null(wrap())}}");
    let annotated =
        format!("{prefix}fn wrap()->ptr<Handle>{{raw()}} fn main()->bool{{is_null(wrap())}}");
    let mut outputs = Vec::new();
    for source in [&inferred, &annotated] {
        let p = parser::parse(source).unwrap();
        let ir = typed::lower_main(&check::analyze(&p).unwrap()).unwrap();
        assert!(typed::evaluate(&ir).is_err());
        assert!(interpreter::run_reference(&p).is_err());
        outputs.push(typed::emit_c(&ir).unwrap());
    }
    assert_eq!(outputs[0], outputs[1]);
    let dir = fixture();
    fs::write(
        dir.join("native.h"),
        "typedef struct Handle Handle;\nstatic inline Handle *raw(void){return (Handle *)0;}\n",
    )
    .unwrap();
    compile_c(&dir, &outputs[1], Some("true"), true);
}
#[test]
fn independent_result_module_alias_shadow_and_snapshot_branding() {
    use rill::{
        modules::compile::load_program_with_sources,
        source_map::AstStep,
        typed::provenance::{
            lower_main_with_copy_node_origins,
            nodes::{GenerationReason, NodeOrigin, RelationTarget},
        },
    };
    let dir = fixture();
    let entry = dir.join("main.rill");
    fs::write(
        dir.join("api.rill"),
        "export type P={x:i32} export fn make()->P{record P{x=42}}",
    )
    .unwrap();
    fs::write(&entry,"import api; fn use_api(api:bool)->api.P{api.make()} fn main()->i32{let p=use_api(true);p.x}").unwrap();
    let a = lower_main_with_copy_node_origins(load_program_with_sources(&entry).unwrap()).unwrap();
    let b = lower_main_with_copy_node_origins(load_program_with_sources(&entry).unwrap()).unwrap();
    assert_eq!(typed::evaluate(a.typed()).unwrap().to_string(), "42");
    let c = typed::emit_c(a.typed()).unwrap();
    assert_eq!(backend::emit(a.sources().program()).unwrap(), c);
    for node in a.nodes() {
        if let NodeOrigin::Generated {
            reason: GenerationReason::AnnotatedResultType,
            related,
        } = &node.origin
        {
            let relations = a.relations(related).unwrap();
            let RelationTarget::Source(id) = &relations[2].target else {
                panic!("annotation source")
            };
            assert_eq!(
                a.function_origins().source_node(id).unwrap().path.steps,
                vec![AstStep::FunctionResult]
            );
            assert!(b.function_origins().source_node(id).is_none());
        }
    }
    compile_c(&dir, &c, Some("42"), true);
    compile_c(
        &fixture(),
        &backend::emit_reference(a.sources().program()).unwrap(),
        Some("42"),
        true,
    );
}
