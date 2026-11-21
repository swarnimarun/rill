//! Independent discarded-result verification and retained-source regressions.
use super::*;
const PREFIX: &str = "type O<a> = None | Some(a) ";
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn seed(p: &mut TypedMain, index: usize) -> &mut DiscardExpr {
    let Computation::Value(body) = &mut p.functions[index].body else {
        panic!()
    };
    let ExprKind::DiscardLet { value, .. } = &mut body.kind else {
        panic!()
    };
    value
}
fn reject(p: &TypedMain) {
    let e = verify(p).unwrap_err();
    assert_eq!(evaluate(p).unwrap_err(), e);
    assert_eq!(emit_c(p).unwrap_err(), e);
}
#[test]
fn independent_k4_direct_if_match_and_dormant_discard_need_no_discard_function() {
    for body in [
        "let weak=if true{None}else{None};42",
        "let weak=match true{true=>None|false=>None};42",
        "if false{let weak=if true{None}else{None};41}else{42}",
    ] {
        let source = format!("{PREFIX}fn main(){{{body}}}");
        let p = lower(&source);
        assert!(p.adts.is_empty());
        assert!(p
            .functions
            .iter()
            .all(|f| matches!(f.result, ResultRepr::Value(_))));
        verify(&p).unwrap();
        assert_eq!(evaluate(&p).unwrap().to_string(), "42");
        let c = emit_c(&p).unwrap();
        assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
        let ast = crate::parser::parse(&source).unwrap();
        assert_eq!(crate::interpreter::run_reference(&ast), evaluate(&p));
        assert_eq!(crate::backend::emit(&ast).unwrap(), c);
    }
}
#[test]
fn independent_k4_internal_only_missing_foreign_and_extra_metadata_reject() {
    let source = format!("{PREFIX}fn main(){{let weak=if true{{None}}else{{None}};42}}");
    let original = lower(&source);
    let foreign = lower(&source);
    for action in 0..5 {
        let mut bad = original.clone();
        match action {
            0 => bad.logical.control = None,
            1 => bad.logical.domain = None,
            2 => bad.logical.declarations.clear(),
            3 => bad.logical = foreign.logical.clone(),
            _ => {
                let ordinal = bad.logical.binders.iter().filter(|b| b.scope == 0).count();
                bad.logical
                    .binders
                    .push(logical::OpaqueBinding { scope: 0, ordinal });
            }
        }
        reject(&bad);
    }
}
#[test]
fn independent_k4_internal_discard_does_not_authorize_another_functions_opaque_scope() {
    let source=format!("{PREFIX}fn side(){{let weak=if true{{None}}else{{None}};1}} fn main(){{let weak=if true{{None}}else{{None}};side()+41}}");
    let mut p = lower(&source);
    let main = p.entry;
    let side = (0..p.functions.len()).find(|i| *i != main).unwrap();
    assert!(p
        .functions
        .iter()
        .all(|f| matches!(f.result, ResultRepr::Value(_))));
    let main_type = seed(&mut p, main).result.clone();
    let side_type = seed(&mut p, side).result.clone();
    assert_ne!(main_type, side_type);
    let facts = p.logical.verify(&p).unwrap();
    facts.logical_in_scope(&main_type, main).unwrap();
    facts.logical_in_scope(&side_type, side).unwrap();
    assert!(facts.logical_in_scope(&side_type, main).is_err());
    assert!(facts.logical_in_scope(&main_type, side).is_err());
    seed(&mut p, main).result = side_type;
    reject(&p);
}
#[test]
fn independent_k4_internal_logical_depth_preflight_precedes_recursive_relation() {
    for (depth, complexity) in [(128, false), (129, true)] {
        let mut p = lower(&format!(
            "{PREFIX}fn main(){{let weak=if true{{None}}else{{None}};42}}"
        ));
        let main = p.entry;
        let mut ty = LogicalType::Atom(crate::check::Type::Int);
        for _ in 0..depth {
            ty = LogicalType::Nominal {
                definition: usize::MAX,
                arguments: vec![ty],
            };
        }
        seed(&mut p, main).result = ty;
        let e = verify(&p).unwrap_err();
        assert_eq!(e.contains("COMPLEXITY"), complexity, "{depth}: {e}");
        reject(&p);
    }
}
