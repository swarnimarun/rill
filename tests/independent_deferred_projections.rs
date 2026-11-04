//! Independently authored deferred-projection scheme, loan and C11 regressions.
use rill::{backend, check, interpreter, parser, typed};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const REFERENCE_BOUNDARY: &str =
    "E-C-ADT: unresolved polymorphic value requires local specialization";
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];
static NEXT: AtomicUsize = AtomicUsize::new(0);
type Consumer = fn(&rill::ast::Program) -> Result<(), String>;

struct Case {
    label: String,
    source: String,
    reference_c: bool,
    family: u8,
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let generic = [
        "fn f(x,y){let r=g(x,y);let projected=r.value;let alias=projected;alias}",
        "fn g(x,y){h(x,y)}",
        "fn h(x,y){if true{record R{value=x}}else{record R{value=f(x,y)}}}",
    ];
    let partial = [
        "fn f(x,p){let wrapper=record Outer{child=p;spare=5};let projected=wrapper.child.tail.value;g(x,p);projected}",
        "fn g(x,p){h(x,p)}",
        "fn h(x,p){let fixed=if true{p}else{record Middle{tail=record Bottom{marker=false;value=x}}};if false{f(x,p)}else{x}}",
    ];
    for order in ORDERS {
        cases.push(Case {
            label: format!("generic-three-{}{}{}", order[0], order[1], order[2]),
            source: format!(
                "type R<a> = {{value:a}}\n{}\n{}\n{}\nfn main(){{if f(true,9){{f(42,false)}}else{{0}}}}",
                generic[order[0]], generic[order[1]], generic[order[2]],
            ),
            reference_c: true,
            family: 1,
        });
        cases.push(Case {
            label: format!("partial-three-{}{}{}", order[0], order[1], order[2]),
            source: format!(
                "type Bottom<a> = {{value:a;marker:bool}} type Middle<a> = {{tail:Bottom<a>}} type Outer<a> = {{spare:i32;child:a}}\n{}\n{}\n{}\nfn main(){{if f(true,record Middle{{tail=record Bottom{{value=true;marker=true}}}}){{f(42,record Middle{{tail=record Bottom{{value=42;marker=false}}}})}}else{{0}}}}",
                partial[order[0]], partial[order[1]], partial[order[2]],
            ),
            reference_c: true,
            family: 2,
        });
    }
    let first = "fn f(x){let r=g(x);let projected=r.value;let alias=projected;let mixed=Pair(alias,None);let ints=keep_i(mixed);let bools=keep_b(mixed);if true{ints}else{bools}}";
    let later = "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}";
    for (index, order) in [[first, later], [later, first]].into_iter().enumerate() {
        cases.push(Case {
            label: format!("selective-stays-generic-{index}"),
            source: format!(
                "type O<a> = None|Some(a) type Pair<a,b> = Pair(a,b) type R<a> = {{value:a}}\nfn keep_i(p){{match p{{Pair(value,rest)=>{{let known=if true{{rest}}else{{Some(0)}};value}}}}}}\nfn keep_b(p){{match p{{Pair(value,rest)=>{{let known=if true{{rest}}else{{Some(false)}};value}}}}}}\n{}\n{}\nfn main(){{if f(true){{f(42)}}else{{0}}}}",
                order[0], order[1],
            ),
            reference_c: false,
            family: 3,
        });
    }
    cases.push(Case {
        label: "pattern-shadow-during-pending-exclusive-loan".into(),
        source: "type I={b:i32;a:i32} type R={i:I;other:i32} type Wrap<a> = Wrap(a)\nfn set(inout destination,n){destination<-n}\nfn f(inout p){set(inout p.i.a,match Wrap(record R{other=0;i=record I{b=0;a=41}}){Wrap(p)=>{let mut p=p;p.i.a<-p.i.a+1;p.i.a}});let fixed=if true{p}else{record R{other=0;i=record I{b=0;a=0}}};p.i.a+p.i.b+p.other}\nfn main(){let mut p=record R{other=0;i=record I{b=0;a=1}};f(inout p)}".into(),
        reference_c: true,
        family: 4,
    });
    cases
}

fn canonical(ty: &check::Type, ids: &mut HashMap<usize, usize>) -> check::Type {
    match ty {
        check::Type::Var(id) => {
            let next = ids.len();
            check::Type::Var(*ids.entry(*id).or_insert(next))
        }
        check::Type::Nominal(name, arguments) => check::Type::Nominal(
            name.clone(),
            arguments
                .iter()
                .map(|argument| canonical(argument, ids))
                .collect(),
        ),
        atom => atom.clone(),
    }
}

fn signature(function: &check::FunctionType) -> (Vec<usize>, Vec<check::Type>, check::Type) {
    assert!(function.quantified.windows(2).all(|pair| pair[0] < pair[1]));
    let mut ids = HashMap::new();
    let params = function
        .params
        .iter()
        .map(|ty| canonical(ty, &mut ids))
        .collect();
    let result = canonical(&function.result, &mut ids);
    let mut quantified: Vec<_> = function
        .quantified
        .iter()
        .map(|id| {
            let next = ids.len();
            *ids.entry(*id).or_insert(next)
        })
        .collect();
    quantified.sort_unstable();
    (quantified, params, result)
}

fn check_schemes(case: &Case, checked: &check::CheckedProgram<'_>) {
    use check::Type::{Nominal, Var};
    match case.family {
        1 => {
            for name in ["f", "g", "h"] {
                let result = if name == "f" {
                    Var(0)
                } else {
                    Nominal("R".into(), vec![Var(0)])
                };
                assert_eq!(
                    signature(&checked.functions[name]),
                    (vec![0, 1], vec![Var(0), Var(1)], result),
                    "{} {name}",
                    case.label
                );
            }
        }
        2 => {
            for name in ["f", "g", "h"] {
                assert_eq!(
                    signature(&checked.functions[name]),
                    (
                        vec![0],
                        vec![Var(0), Nominal("Middle".into(), vec![Var(0)])],
                        Var(0)
                    ),
                    "{} {name}",
                    case.label
                );
            }
        }
        3 => {
            assert_eq!(
                signature(&checked.functions["f"]),
                (vec![0], vec![Var(0)], Var(0)),
                "{}",
                case.label
            );
            assert_eq!(
                signature(&checked.functions["g"]),
                (vec![0], vec![Var(0)], Nominal("R".into(), vec![Var(0)])),
                "{}",
                case.label
            );
        }
        _ => {}
    }
}

#[test]
fn independent_generic_cycles_and_selective_schemes_are_order_stable() {
    for case in cases() {
        let program =
            parser::parse(&case.source).unwrap_or_else(|error| panic!("{}: {error}", case.label));
        let checked =
            check::analyze(&program).unwrap_or_else(|error| panic!("{}: {error}", case.label));
        check_schemes(&case, &checked);
        let lowered =
            typed::lower_main(&checked).unwrap_or_else(|error| panic!("{}: {error}", case.label));
        typed::verify(&lowered).unwrap();
        for actual in [
            typed::evaluate(&lowered),
            interpreter::run(&program),
            interpreter::run_reference(&program),
        ] {
            assert_eq!(actual.unwrap().to_string(), "42", "{}", case.label);
        }
        let c = typed::emit_c(&lowered).unwrap();
        assert_eq!(backend::emit(&program).unwrap(), c, "{}", case.label);
        assert!(!c.contains("rl_value") && !c.contains("RL_CAP") && !c.contains("restrict"));
        if case.reference_c {
            backend::emit_reference(&program)
                .unwrap_or_else(|error| panic!("{}: {error}", case.label));
        } else {
            assert_eq!(
                backend::emit_reference(&program).unwrap_err(),
                REFERENCE_BOUNDARY,
                "{}",
                case.label
            );
        }
    }
}

#[test]
fn independent_late_constraints_do_not_hide_occurs_or_component_errors() {
    let first = "fn f(x){let r=g(x);let projected=r.value;if true{projected}else{r}}";
    let later = "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}";
    let mut negatives = Vec::new();
    for order in [[first, later], [later, first]] {
        negatives.push((
            format!(
                "type R<a> = {{value:a}} {} {} fn main(){{42}}",
                order[0], order[1]
            ),
            "E-TYPE:",
        ));
    }
    for order in [
        "fn f(p){let x=p.value;g(p);x} fn g(p){f(p)} fn caller(p:R){f(p)}",
        "fn caller(p:R){f(p)} fn g(p){f(p)} fn f(p){let x=p.value;g(p);x}",
    ] {
        negatives.push((
            format!("type R={{value:i32}} {order} fn main(){{caller(record R{{value=42}})}}"),
            "E-RECORD:",
        ));
    }
    negatives.push(("type A={value:i32} type B={value:i32} fn bad(p,q){let first=p.value;let second=q.value;let a=if true{p}else{record A{value=first}};let b=if true{q}else{record B{value=second}};if true{p}else{q}} fn main(){42}".into(), "E-TYPE:"));
    let first = "fn f(x){let r=g(x);let projected=r.value;let alias=projected;let mixed=Pair(None,alias);let one=integers(mixed);booleans(mixed);alias}";
    let later = "fn g(x){if true{record R{value=None}}else{record R{value=f(x)}}}";
    for order in [[first, later], [later, first]] {
        negatives.push((format!("type O<a> = None|Some(a) type Pair<a,b> = Pair(a,b) type R<a> = {{value:a}} fn integers(p:Pair<O<i32>,O<i32>>){{0}} fn booleans(p:Pair<O<bool>,O<bool>>){{0}} {} {} fn main(){{42}}", order[0], order[1]), "E-TYPE:"));
    }
    for (source, prefix) in negatives {
        let program = parser::parse(&source).unwrap();
        let consumers: [Consumer; 6] = [
            check::check,
            |program| interpreter::run(program).map(|_| ()),
            |program| interpreter::run_reference(program).map(|_| ()),
            |program| backend::emit(program).map(|_| ()),
            |program| backend::emit_reference(program).map(|_| ()),
            |program| {
                check::analyze(program).and_then(|checked| typed::lower_main(&checked).map(|_| ()))
            },
        ];
        for consumer in consumers {
            let error = consumer(&program).unwrap_err();
            assert!(
                error.starts_with(prefix),
                "expected {prefix}, got {error}: {source}"
            );
        }
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rill-independent-projection-{stamp}-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("main.rill"), source).unwrap();
        eprintln!("INDEPENDENT_PROJECTION_ARTIFACTS {}", directory.display());
        Self(directory)
    }
    fn output(&self, command: &mut Command, label: &str, seconds: u64) -> Output {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.0.join(format!("{id}-{label}.out"));
        let stderr = self.0.join(format!("{id}-{label}.err"));
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{label}: timeout is not a successful check or language trap");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
    fn guard(&self) {
        if let Some(guard) = std::env::var_os("K4_RESOURCE_GUARD") {
            let result = self.output(Command::new("bash").arg(guard), "guard", 10);
            if !result.status.success() {
                eprintln!(
                    "resource floor: {} {}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
                std::process::exit(86);
            }
        }
    }
    fn execute(&self, label: &str, c: &str) {
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let source = self.0.join(format!("{label}.c"));
        fs::write(&source, c).unwrap();
        for optimization in ["-O0", "-O2"] {
            self.guard();
            let binary = self.0.join(format!("{label}-{}", &optimization[1..]));
            let build = self.output(
                Command::new(&compiler)
                    .args(["-std=c11", "-pedantic-errors", optimization])
                    .arg(&source)
                    .arg("-o")
                    .arg(&binary),
                "compile",
                30,
            );
            assert!(
                build.status.success(),
                "{label}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            if !build.stderr.is_empty() {
                eprintln!("{label}: {}", String::from_utf8_lossy(&build.stderr));
            }
            self.guard();
            let run = self.output(&mut Command::new(&binary), "execute", 10);
            assert_eq!(run.status.code(), Some(0), "{label} {optimization}");
            assert_eq!(run.stdout, b"42\n", "{label} {optimization}");
            assert!(
                run.stderr.is_empty(),
                "{label}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            eprintln!(
                "INDEPENDENT_PROJECTION_C_PASS {} {label} {optimization}",
                compiler.to_string_lossy()
            );
        }
    }
}

#[test]
fn independent_fixtures_execute_as_selected_c11_at_o0_and_o2() {
    for case in cases() {
        let program = parser::parse(&case.source).unwrap();
        let checked = check::analyze(&program).unwrap();
        check_schemes(&case, &checked);
        let lowered = typed::lower_main(&checked).unwrap();
        typed::verify(&lowered).unwrap();
        for actual in [
            typed::evaluate(&lowered),
            interpreter::run(&program),
            interpreter::run_reference(&program),
        ] {
            assert_eq!(actual.unwrap().to_string(), "42", "{}", case.label);
        }
        let c = typed::emit_c(&lowered).unwrap();
        assert_eq!(backend::emit(&program).unwrap(), c);
        assert!(!c.contains("rl_value") && !c.contains("RL_CAP") && !c.contains("restrict"));
        let fixture = Fixture::new(&case.label, &case.source);
        fixture.execute("typed", &c);
        if case.reference_c {
            fixture.execute("reference", &backend::emit_reference(&program).unwrap());
        } else {
            assert_eq!(
                backend::emit_reference(&program).unwrap_err(),
                REFERENCE_BOUNDARY
            );
            eprintln!("INDEPENDENT_PROJECTION_REFERENCE_C_BOUNDARY {}", case.label);
        }
    }
}
