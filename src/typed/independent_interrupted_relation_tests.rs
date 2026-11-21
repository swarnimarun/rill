//! Intended child module of crate::typed::control::relation; parent integration only.
use super::*;
use crate::typed::{Computation, ExprKind, TypedMain};
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
const SOURCE: &str = "fn spin(){spin()} fn ignore(a,b,c){42} fn main(){ignore(3,spin(),8)}";
fn entry(typed: &mut TypedMain) -> &mut crate::typed::InterruptedCall {
    let Computation::Value(value) = &mut typed.functions[typed.entry].body else {
        panic!()
    };
    let ExprKind::InterruptedCall(call) = &mut value.kind else {
        panic!()
    };
    call
}
#[test]
fn independent_interrupted_source_occurrence_target_and_signature_corruption() {
    let (frozen, typed) = fixture(SOURCE);
    let mut bad = typed.clone();
    entry(&mut bad).evidence.occurrence = NodeId(usize::MAX);
    assert!(frozen.verify_structure(&bad).is_err());
    let mut bad = typed.clone();
    entry(&mut bad).evidence.target = usize::MAX;
    assert!(frozen.verify_structure(&bad).is_err());
    let mut bad = typed.clone();
    entry(&mut bad).evidence.parameters.swap(0, 1);
    assert!(frozen.verify_structure(&bad).is_err());
    let mut bad = typed.clone();
    entry(&mut bad).evidence.result = Type::Bool;
    assert!(frozen.verify_structure(&bad).is_err());
    let mut bad = typed;
    entry(&mut bad).evidence.syntax.clear();
    assert!(frozen.verify_structure(&bad).is_err());
}
#[test]
fn independent_omitted_suffix_literal_is_retained_on_both_sides() {
    let (mut frozen, typed) = fixture(SOURCE);
    let suffix = frozen
        .witness
        .nodes
        .iter_mut()
        .find(|n| n.kind == SourceKind::Int(8))
        .unwrap();
    suffix.kind = SourceKind::Int(9);
    assert!(frozen.verify_structure(&typed).is_err());
}
#[test]
fn independent_interrupted_original_checked_source_stays_distinct_from_frozen_relation() {
    let (frozen, typed) = fixture(SOURCE);
    frozen.verify_structure(&typed).unwrap();
    for changed in [
        SOURCE.replace("spin(),8", "spin(),9"),
        SOURCE.replace("ignore(a,b,c){42}", "ignore(a,b,c){41}"),
    ] {
        let ast = crate::parser::parse(&changed).unwrap();
        let checked = crate::check::analyze(&ast).unwrap();
        assert!(frozen.verify_source(&checked).is_err());
    }
}
#[test]
fn independent_interrupted_unused_native_manifest_still_matches() {
    let (frozen,mut typed)=fixture("@include(\"stdint.h\"); extern fn keep(x:i32)->i32; fn spin(){spin()} fn ignore(x){42} fn main(){ignore(spin())}");
    typed.native.functions.clear();
    assert!(frozen.verify_structure(&typed).is_err());
}
#[test]
fn independent_interrupted_evidence_depth_and_exact_charge() {
    let (frozen, mut typed) = fixture(SOURCE);
    let evidence = entry(&mut typed).evidence.clone();
    let mut measured = Budget::default();
    evidence.charge(&mut measured).unwrap();
    let exact = MAX_WORK - measured.used;
    evidence.charge(&mut Budget { used: exact }).unwrap();
    assert_eq!(
        evidence
            .charge(&mut Budget { used: exact + 1 })
            .unwrap_err(),
        LIMIT
    );
    for (depth, complexity) in [(128, false), (129, true)] {
        let mut bad = typed.clone();
        let mut ty = Type::Int;
        for _ in 0..depth {
            ty = Type::Nominal("X".into(), vec![ty]);
        }
        entry(&mut bad).evidence.parameters[0] = ty;
        let error = frozen.verify_structure(&bad).unwrap_err();
        assert_eq!(
            error.contains("CONTROL-COMPLEXITY"),
            complexity,
            "{depth}: {error}"
        );
    }
}

#[test]
fn independent_never_instantiated_target_rejects_noncanonical_quantifiers() {
    for duplicate in [true, false] {
        let (mut frozen, mut typed) = fixture(SOURCE);
        let evidence = entry(&mut typed).evidence.clone();
        let definition = frozen
            .witness
            .functions
            .iter_mut()
            .find(|f| f.source == evidence.target)
            .unwrap();
        let invalid = if duplicate {
            definition.quantified[0]
        } else {
            usize::MAX
        };
        definition.quantified.push(invalid);
        // Keep both copies aligned so ordinary evidence equality cannot be the
        // reason for rejection. The skipped target has no executable instance.
        entry(&mut typed).evidence = super::super::interrupted::InterruptedEvidence::capture(
            &frozen.witness,
            &frozen.domain,
            evidence.occurrence,
            evidence.parameters,
            evidence.result,
            &mut Budget::default(),
        )
        .unwrap();
        assert!(frozen.verify_structure(&typed).is_err());
    }
}
