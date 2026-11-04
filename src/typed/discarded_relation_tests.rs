//! Author proof-mutation tests, independent of the discard-lowering helper.
use super::*;
use crate::typed::{self, Computation, ExprKind};
fn rejects(ir: &TypedMain) {
    assert!(typed::verify(ir).is_err());
    assert!(typed::evaluate(ir).is_err());
    assert!(typed::emit_c(ir).is_err());
}
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    typed::lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
#[test]
fn inactive_internal_only_discard_schema_cannot_gain_owner_or_native_fields() {
    let source="type O<a> = None|Some(a) type B<a> = BN|BS(a) fn absent(){let unused=if true{BN}else{BN};None} fn main(){let weak=absent();42}";
    for name in ["SystemBoxI32", "u32"] {
        let mut ir = lower(source);
        let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
        proof.witness.environment.types[1].variants[1].fields[0].name = name.into();
        ir.logical.declarations[1].variants[1].fields[0].name = name.into();
        rejects(&ir);
    }
}
#[test]
fn source_use_recount_rejects_hidden_read_despite_zero_cache() {
    let mut ir =
        lower("type O<a> = None|Some(a) fn absent(){None} fn main(){let weak=absent();42}");
    let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
    let source = proof
        .witness
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    let SourceKind::Let(binding, _, body) = proof.witness.nodes[source.body.0].kind else {
        panic!()
    };
    assert_eq!(proof.witness.bindings[binding.0].uses, 0);
    proof.witness.nodes[body.0].kind = SourceKind::Var(Reference::Local(binding));
    rejects(&ir);
}
#[test]
fn consistent_retained_witness_edit_still_fails_original_source_equality() {
    let ast = crate::parser::parse(
        "type O<a> = None|Some(a) fn absent(){None} fn main(){let weak=absent();42}",
    )
    .unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    let mut ir = typed::lower_main(&checked).unwrap();
    let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
    let node = proof
        .witness
        .nodes
        .iter_mut()
        .find(|node| node.kind == SourceKind::Int(42))
        .unwrap();
    node.kind = SourceKind::Int(43);
    let ExprKind::DiscardLet { body, .. } =
        &mut ir.functions[ir.entry].body.value_expr_mut().unwrap().kind
    else {
        panic!()
    };
    body.kind = ExprKind::Int(43);
    typed::verify(&ir).unwrap();
    assert!(ir
        .logical
        .control
        .as_ref()
        .unwrap()
        .verify_source(&checked)
        .is_err());
}
#[test]
fn discarded_constructor_flow_is_true_and_frozen_work_has_exact_boundary() {
    let ir = lower("type O<a> = None|Some(a) fn absent(){None} fn main(){let weak=absent();42}");
    let flows = IrFacts::build(&ir).unwrap();
    assert!(matches!(ir.functions[0].body, Computation::Discarded(_)));
    assert_eq!(flows.function_may_return(0), Some(true));
    assert_eq!(flows.function_may_return(ir.entry), Some(true));
    let proof = ir.logical.control.as_ref().unwrap();
    let mut budget = Budget::default();
    proof.charge(&mut budget).unwrap();
    let cost = budget.used;
    assert!(cost > 0 && cost < MAX_WORK);
    let mut exact = Budget::default();
    exact.charge(MAX_WORK - cost).unwrap();
    proof.charge(&mut exact).unwrap();
    let mut short = Budget::default();
    short.charge(MAX_WORK - cost + 1).unwrap();
    assert!(proof.charge(&mut short).unwrap_err().contains("COMPLEXITY"));
}

#[test]
fn inactive_source_only_suffix_schema_cannot_gain_owner_or_native_fields() {
    let source = "type B<a> = BN|BS(a) fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),BN)}";
    for name in ["SystemBoxI32", "u32"] {
        let mut ir = lower(source);
        let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
        proof.witness.environment.types[0].variants[1].fields[0].name = name.into();
        ir.logical.declarations[0].variants[1].fields[0].name = name.into();
        rejects(&ir);
    }
}
