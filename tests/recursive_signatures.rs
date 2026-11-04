//! Existing closed parameters constrain monomorphic SCC placeholders before bodies.
use rill::{ast::TypeExpr, backend, check, interpreter, parser, typed};
#[path = "support/recursive_signature_cases.rs"]
mod cases;
#[path = "support/result_annotation_compatibility.rs"]
mod compatibility;
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

#[test]
fn directly_annotated_recursive_signatures_preserve_modes_and_execution() {
    for (label, source, expected) in cases::cases() {
        let program = parser::parse(&source).unwrap_or_else(|e| panic!("{label}: {e}"));
        let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{label}: {e}"));
        let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{label}: {e}"));
        typed::verify(&lowered).unwrap();
        for actual in [
            typed::evaluate(&lowered),
            interpreter::run(&program),
            interpreter::run_reference(&program),
        ] {
            assert_eq!(actual.unwrap().to_string(), expected, "{label}");
        }
        assert_eq!(
            typed::emit_c(&lowered).unwrap(),
            backend::emit(&program).unwrap(),
            "{label}"
        );
        backend::emit_reference(&program).unwrap();
    }
}

#[test]
fn constrained_recursive_schemes_keep_exact_quantifier_membership() {
    use check::Type::{Int, Nominal, Var};
    let f = "fn f(p,x)->i32{g(p,x);p.x}";
    let g = "fn g(p:P,x)->i32{if false{f(p,x)}else{0}}";
    for functions in [format!("{f}\n{g}"), format!("{g}\n{f}")] {
        let source = format!("type P={{x:i32}} {functions} fn independent(a,b:P){{a}} fn main(){{f(record P{{x=21}},true)+f(record P{{x=21}},0)}}");
        let program = parser::parse(&source).unwrap();
        let checked = check::analyze(&program).unwrap();
        for name in ["f", "g"] {
            assert_eq!(
                compatibility::canonical_signature(&checked.functions[name]),
                (vec![0], vec![Nominal("P".into(), vec![]), Var(0)], Int),
                "{name}"
            );
        }
        assert_eq!(
            compatibility::canonical_signature(&checked.functions["independent"]),
            (vec![0], vec![Var(0), Nominal("P".into(), vec![])], Var(0))
        );
        assert_eq!(
            compatibility::canonical_signature(&checked.functions["main"]),
            (vec![], vec![], Int)
        );
        assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
    }
}

#[test]
fn bounded_deferred_projection_migration_preserves_other_fingerprints() {
    assert_eq!(
        compatibility::fingerprints(),
        include_str!("support/result_annotation_schemes.txt")
    );
    // The later projection increment intentionally accepts this intermediate
    // parameter case through SCC-local body constraints, not annotation guessing.
    let source = "type P={x:i32} fn f(p)->i32{g(p);p.x} fn g(p)->i32{h(p)} fn h(p:P)->i32{if false{f(p)}else{0}} fn main(){42}";
    check::check(&parser::parse(source).unwrap()).unwrap();
}

#[test]
fn closed_parameter_preconstraints_do_not_relax_rejections() {
    for source in [
        "fn f(x:i32)->bool{x} fn main(){42}",
        "fn f(x:i32){x} fn main(){f(true)}",
        "fn f(x:i32){g(x)} fn g(x:bool){f(x)} fn main(){42}",
        "type A={x:i32} type B={x:i32} fn f(p:A){g(p)} fn g(p:B){f(p)} fn main(){42}",
        "fn unused(x:i32<bool>){x} fn main(){42}",
        "fn unused(x:Missing){x} fn main(){42}",
        "fn unused(x:_){x} fn main(){42}",
        "type O<a> =None|Some(a) fn unused(x:O<i32,bool>){x} fn main(){42}",
        "fn unused(x:ptr<Handle<i32>>){x} fn main(){42}",
        "fn wrong(x:ptr<Handle>)->const_ptr<Handle>{x} fn main(){42}",
        "fn wrong(x:ptr<Handle>)->ptr<Other>{x} fn main(){42}",
        "type O<a> =None|Some(a) fn f(x:i32){let mut slot=None;let alias=slot;slot<-Some(x);if true{alias}else{Some(true)}} fn main(){42}",
        "type W<a> =Wrap(a) fn f(x,n:i32){g(Wrap(x),n)} fn g(x,n:i32){f(x,n)} fn main(){42}",
        "type O<a> =None|Some(a) fn unused(x:O<i32>){match x{None=>42}} fn main(){42}",
        "fn f(x:i32){if true{f(true)}else{f(1)}} fn main(){42}",
        "type P={x:i32} fn f(read p){g(inout p);p.x} fn g(inout p:P){if false{f(p);}else{()}} fn main(){42}",
        "type P={x:i32} fn f(read p:P){p.x<-1} fn main(){42}",
        "type P={x:i32} fn f(inout a:P,inout b:P){a.x<-b.x} fn main(){let mut p=record P{x=1};f(inout p,inout p)}",
        "type P={x:i32} fn f(read a:P,inout b:P){b.x<-a.x} fn main(){let mut p=record P{x=1};f(p,inout p)}",
        "type P={x:i32} fn f(inout p:P){p.x<-2} fn main(){let p=record P{x=1};f(inout p)}",
        "fn inherited(x:ptr<SystemBytes>){x} fn main(){42}",
        "fn inherited(x:const_ptr<SystemBoxI32>){x} fn main(){42}",
    ] {
        let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        for consumer in CONSUMERS {
            assert!(consumer(&program).is_err(), "accepted {source}");
        }
    }
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
fn public_consumers_admit_parameter_and_mixed_depths_before_recursive_work() {
    for declaration in [
        "fn dormant(x){dormant(x)} fn main(){42}",
        "let dormant x = dormant(x)\nlet main = 42",
    ] {
        for mixed in [false, true] {
            for depth in [127, 128, 129, 10_000] {
                let mut program =
                    parser::parse(&format!("type W<a> =Wrap(a)\n{declaration}")).unwrap();
                program.functions[0].params[0].annotation = Some(nested(depth));
                if mixed {
                    program.functions[0].result_annotation = Some(nested(128));
                }
                // Collect before assertions so the caller-owned deep trees are
                // dismantled even if the compiler unexpectedly rejects/accepts.
                let results: Vec<_> = CONSUMERS
                    .into_iter()
                    .map(|consumer| consumer(&program))
                    .collect();
                dismantle(program.functions[0].params[0].annotation.take().unwrap());
                if let Some(result) = program.functions[0].result_annotation.take() {
                    dismantle(result);
                }
                for result in results {
                    if depth > 128 {
                        assert_eq!(result.unwrap_err(), BUDGET_ERROR);
                    } else {
                        assert!(result.is_ok(), "depth={depth}, mixed={mixed}: {result:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_raw_parameter_subtrees_are_fully_charged_without_rewriting() {
    for pointer in ["ptr", "const_ptr"] {
        for depth in [128, 129, 10_000] {
            let mut program = parser::parse("fn dormant(x){x} fn main(){42}").unwrap();
            program.functions[0].params[0].annotation = Some(TypeExpr {
                name: pointer.into(),
                args: vec![nested(depth - 1)],
            });
            let results: Vec<_> = CONSUMERS
                .into_iter()
                .map(|consumer| consumer(&program))
                .collect();
            dismantle(program.functions[0].params[0].annotation.take().unwrap());
            for result in results {
                let error = result.unwrap_err();
                if depth > 128 {
                    assert_eq!(error, BUDGET_ERROR);
                } else {
                    assert_eq!(error, "E-FFI: pointer types require one C pointee name");
                }
            }
        }
    }
}

#[test]
fn parser_parameter_nesting_guard_is_independent_of_public_tree_admission() {
    for prefix in ["fn dormant(x:", "let dormant (x:"] {
        let ending = if prefix.starts_with("fn") {
            "){x} fn main(){42}"
        } else {
            ") = x\nlet main = 42"
        };
        let source = format!(
            "{prefix}{}i32{}{ending}",
            "W<".repeat(10_000),
            ">".repeat(10_000)
        );
        assert!(parser::parse(&source)
            .unwrap_err()
            .contains("E-PARSE-DEPTH"));
    }
}
