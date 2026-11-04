//! Independent discarded-result verification and retained-source regressions.
use super::*;
use crate::typed::{Computation, DiscardExpr, DiscardKind, ExprKind, ResultRepr, TypedMain};
const CORE: &str = "type O<a> = None|Some(a) fn absent(){None} fn main(){let weak=absent();42}";
fn fixture(source: &str) -> (Frozen, TypedMain) {
    let ast = crate::parser::parse(source).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    let (typed, instances) = crate::typed::lower_main_traced(&checked, None, None).unwrap();
    let frozen = SourceFacts::build(&checked)
        .unwrap()
        .freeze(&instances)
        .unwrap();
    frozen.verify_source(&checked).unwrap();
    frozen.verify_structure(&typed).unwrap();
    (frozen, typed)
}
fn install_reject(frozen: Frozen, mut typed: TypedMain) {
    typed.logical.control = Some(std::sync::Arc::new(frozen));
    let error = crate::typed::verify(&typed).unwrap_err();
    assert_eq!(crate::typed::evaluate(&typed).unwrap_err(), error);
    assert_eq!(crate::typed::emit_c(&typed).unwrap_err(), error);
}
#[test]
fn independent_k4_every_internal_discard_type_checks_inactive_schema_dependencies() {
    // B has no physical layout and is not any function's result. Its discarded
    // conditional initializer is deliberately expansive, not a local shape.
    let source="type O<a> = None|Some(a) type B<a> = BN|BS(a) fn absent(){let unused=if true{BN}else{BN};None} fn main(){let weak=absent();42}";
    for forbidden in ["SystemBoxI32", "u32"] {
        let (mut frozen, mut typed) = fixture(source);
        assert!(typed.adts.is_empty());
        let id = frozen
            .witness
            .environment
            .types
            .iter()
            .position(|d| d.name == "B")
            .unwrap();
        frozen.witness.environment.types[id].variants[1].fields[0].name = forbidden.into();
        typed.logical.declarations[id].variants[1].fields[0].name = forbidden.into();
        // Both declaration copies agree. Rejection must reconstruct the full
        // dependency graph and validate the internal logical-only B result.
        install_reject(frozen, typed);
    }
}
#[test]
fn independent_k4_immutable_nonuse_is_recounted_not_a_saved_zero_flag() {
    for action in 0..3 {
        let (mut frozen, typed) = fixture(CORE);
        let id = frozen
            .witness
            .bindings
            .iter()
            .position(|b| b.name == "weak")
            .unwrap();
        assert_eq!(frozen.witness.bindings[id].uses, 0);
        match action {
            0 => frozen.witness.bindings[id].mutable = true,
            1 => {
                let tail = frozen
                    .witness
                    .nodes
                    .iter_mut()
                    .find(|n| n.kind == SourceKind::Int(42))
                    .unwrap();
                tail.kind = SourceKind::Var(Reference::Local(BindingId(id)));
                // Leave cached uses == 0, despite this real retained reference.
            }
            _ => {
                frozen.witness.bindings[id].site = BindingSite::Parameter {
                    source: 0,
                    ordinal: 0,
                }
            }
        }
        install_reject(frozen, typed);
    }
}
#[test]
fn independent_k4_demand_signature_quantifier_and_substitution_caches_cannot_authorize() {
    for action in 0..4 {
        let (mut frozen, typed) = fixture(CORE);
        let d = frozen
            .instances
            .iter()
            .position(|i| i.demand == instances::Demand::Discard)
            .unwrap();
        match action {
            0 => frozen.instances[d].demand = instances::Demand::Observe,
            1 => {
                let source = frozen.instances[d].source;
                let f = frozen
                    .witness
                    .functions
                    .iter_mut()
                    .find(|f| f.source == source)
                    .unwrap();
                let q = *f.quantified.first().unwrap();
                f.quantified.push(q);
            }
            2 => {
                let source = frozen.instances[d].source;
                let f = frozen
                    .witness
                    .functions
                    .iter_mut()
                    .find(|f| f.source == source)
                    .unwrap();
                f.quantified.push(usize::MAX);
            }
            _ => {
                let key = *frozen.instances[d].substitution.keys().next().unwrap();
                frozen.instances[d].substitution.insert(key, Type::Bool);
            }
        }
        install_reject(frozen, typed);
    }
}
#[test]
fn independent_k4_original_checked_program_relation_is_distinct_from_standalone_proof() {
    let (mut frozen, mut typed) = fixture(CORE);
    let source_tail = frozen
        .witness
        .nodes
        .iter_mut()
        .find(|n| n.kind == SourceKind::Int(42))
        .unwrap();
    source_tail.kind = SourceKind::Int(41);
    let Computation::Value(entry) = &mut typed.functions[typed.entry].body else {
        panic!()
    };
    let ExprKind::DiscardLet { body, .. } = &mut entry.kind else {
        panic!()
    };
    body.kind = ExprKind::Int(41);
    // A consistently changed retained program can be internally valid. It must
    // still fail correspondence to the actual CheckedProgram passed to lower.
    frozen.verify_structure(&typed).unwrap();
    let original = crate::parser::parse(CORE).unwrap();
    let checked = crate::check::analyze(&original).unwrap();
    assert!(frozen.verify_source(&checked).is_err());
    typed.logical.control = Some(std::sync::Arc::new(frozen));
    crate::typed::verify(&typed).unwrap();
    assert_eq!(crate::typed::evaluate(&typed).unwrap().to_string(), "41");
}
#[test]
fn independent_k4_returning_discard_cannot_prove_a_no_normal_successor() {
    let (frozen,mut typed)=fixture("type O<a> = None|Some(a) fn absent(){None} fn spin(){spin()} fn main(){let weak=absent();spin()}");
    let main = typed.entry;
    let Computation::NoNormal(body) = &mut typed.functions[main].body else {
        panic!()
    };
    let crate::typed::NoNormalKind::DiscardLet { value, .. } = &mut body.kind else {
        panic!()
    };
    let normal = value.as_ref().clone();
    // Remove the recursive successor from actual IR, but deliberately retain
    // the NoNormal result claim. IR flow must see the real normal computation.
    typed.functions[main].body = Computation::Discarded(normal);
    assert!(matches!(
        typed.functions[main].result,
        ResultRepr::NoNormal(_)
    ));
    let facts = IrFacts::build(&typed).unwrap();
    assert_eq!(facts.function_may_return(main), Some(true));
    install_reject(frozen, typed);
}
#[test]
fn independent_k4_frozen_evidence_charge_has_an_exact_one_short_boundary() {
    let (frozen, _) = fixture(CORE);
    let mut measured = Budget::default();
    frozen.charge(&mut measured).unwrap();
    assert!(measured.used > 0 && measured.used < MAX_WORK);
    let exact = MAX_WORK - measured.used;
    frozen.charge(&mut Budget { used: exact }).unwrap();
    assert_eq!(
        frozen.charge(&mut Budget { used: exact + 1 }).unwrap_err(),
        LIMIT
    );
}
#[test]
fn independent_k4_retained_unselected_branch_is_not_pruned_from_source() {
    let source="type O<a> = None|Some(a) fn absent(){None} fn main(){if false{let weak=absent();41}else{42}}";
    let (mut frozen, typed) = fixture(source);
    let dormant = frozen
        .witness
        .nodes
        .iter_mut()
        .find(|n| n.kind == SourceKind::Int(41))
        .unwrap();
    dormant.kind = SourceKind::Int(99);
    install_reject(frozen, typed);
}
#[test]
fn independent_k4_actual_discard_constructor_flow_does_not_use_source_no_normal_cache() {
    let (frozen,mut typed)=fixture("type O<a> = None|Some(a) fn recurse(n:i32){if n==0{None}else{recurse(n-1)}} fn main(){let weak=recurse(31);42}");
    let d = typed
        .functions
        .iter()
        .position(|f| matches!(f.body, Computation::Discarded(_)))
        .unwrap();
    let Computation::Discarded(body) = &mut typed.functions[d].body else {
        panic!()
    };
    let DiscardKind::If { yes, no, .. } = &mut body.kind else {
        panic!()
    };
    assert!(matches!(yes.kind, DiscardKind::Constructor { .. }));
    **no = DiscardExpr {
        result: yes.result.clone(),
        occurrence: no.occurrence,
        kind: yes.kind.clone(),
    };
    let facts = IrFacts::build(&typed).unwrap();
    assert_eq!(facts.function_may_return(d), Some(true));
    install_reject(frozen, typed);
}
