//! Intended as a child module of crate::typed, integrated only by parent grant.
use super::*;
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn rejects(p: &TypedMain) {
    assert!(verify(p).is_err());
    assert!(evaluate(p).is_err());
    assert!(emit_c(p).is_err());
}
fn entry(p: &mut TypedMain) -> &mut InterruptedCall {
    match &mut p.functions[p.entry].body {
        Computation::Value(ExprNode {
            kind: ExprKind::InterruptedCall(call),
            ..
        }) => call,
        Computation::NoNormal(NoNormalExpr {
            kind: NoNormalKind::InterruptedCall(call),
            ..
        }) => call,
        _ => panic!("source entry interruption lost"),
    }
}
const BASIC: &str = "fn spin(){spin()} fn ignore(a,b,c){42} fn main(){ignore(3,spin(),8)}";
#[test]
fn independent_interrupted_prefix_barrier_foreign_evidence_and_missing_proof() {
    let original = lower(BASIC);
    let mut bad = original.clone();
    entry(&mut bad).prefix[0].kind = ExprKind::Int(4);
    rejects(&bad);
    let mut bad = original.clone();
    entry(&mut bad).prefix.clear();
    rejects(&bad);
    let mut bad = original.clone();
    *entry(&mut bad).barrier = Computation::Value(ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(42),
    });
    rejects(&bad);
    let mut foreign = lower(BASIC);
    let mut bad = original.clone();
    entry(&mut bad).evidence = entry(&mut foreign).evidence.clone();
    rejects(&bad);
    let mut bad = original;
    assert!(bad.logical.control.take().is_some());
    rejects(&bad);
}
#[test]
fn independent_nested_concrete_barrier_is_rederived_not_tag_seeded() {
    let mut bad = lower("fn spin(){spin()} fn ignore(x){42} fn main(){ignore(ignore(spin()))}");
    let call = entry(&mut bad);
    let Computation::Value(value) = &mut *call.barrier else {
        panic!("expected concrete nested barrier")
    };
    assert!(matches!(value.kind, ExprKind::InterruptedCall(_)));
    value.kind = ExprKind::Int(42);
    rejects(&bad);
}
#[test]
fn independent_dormant_interruption_requires_proof_with_only_concrete_tags() {
    let mut bad=lower("fn spin(n:i32){if false {42} else {spin(n)}} fn forever(){forever()} fn ignore(x){42} fn main(){if true {42} else {ignore(forever())}}");
    assert_eq!(evaluate(&bad).unwrap(), crate::interpreter::Value::Int(42));
    bad.logical.control = None;
    // Erase NoNormal declarations so only the dormant concrete extension tag
    // can trigger the missing-proof detector. This is deliberately malformed.
    for function in &mut bad.functions {
        if matches!(function.result, ResultRepr::NoNormal(_)) {
            function.result = ResultRepr::Value(Type::Int);
            function.body = Computation::Value(ExprNode {
                ty: Type::Int,
                kind: ExprKind::Int(0),
            });
        }
    }
    let error = verify(&bad).unwrap_err();
    assert!(error.contains("canonical source proof"), "{error}");
    rejects(&bad);
}
#[test]
fn independent_source_main_mutation_has_31_barrier_frames_and_no_suffix() {
    let p=lower("fn spin(inout n){n<-n+100;spin(inout n)} fn ignore(a,b,c){5*7} fn main(){let mut n=0;ignore({n<-n+1;n},spin(inout n),{n<-n+10000;n})}");
    verify(&p).unwrap();
    let Computation::Value(value) = &p.functions[p.entry].body else {
        panic!()
    };
    let ExprKind::Let {
        local,
        mutable: true,
        value,
        body,
    } = &value.kind
    else {
        panic!()
    };
    let mut locals = CopyLocals::default();
    let mut state = EvalState::default();
    let mut owners = std::collections::HashMap::new();
    let initial = evaluate_expr(value, &mut locals, &p, 1, &mut state, &mut owners).unwrap();
    locals.insert(*local, initial);
    assert_eq!(
        evaluate_expr(body, &mut locals, &p, 1, &mut state, &mut owners).unwrap_err(),
        "runtime trap"
    );
    assert_eq!(
        locals.read(local).unwrap(),
        crate::interpreter::Value::Int(3101)
    );
}
#[test]
fn independent_interrupted_ordinary_verification_still_checks_prefix_scope() {
    let mut bad = lower(BASIC);
    entry(&mut bad).prefix[0].kind = ExprKind::Read(LocalId(u32::MAX));
    rejects(&bad);
}
#[test]
fn independent_interrupted_actual_tree_boundary_is_128_then_129() {
    for (wrappers, complexity) in [(127, false), (128, true)] {
        let mut bad = lower(BASIC);
        let mut value = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(3),
        };
        for _ in 0..wrappers {
            value = ExprNode {
                ty: Type::Int,
                kind: ExprKind::ByteLen(Box::new(value)),
            };
        }
        entry(&mut bad).prefix[0] = value;
        let error = verify(&bad).unwrap_err();
        assert_eq!(
            error.contains("CONTROL-COMPLEXITY"),
            complexity,
            "{wrappers}: {error}"
        );
        rejects(&bad);
    }
}
