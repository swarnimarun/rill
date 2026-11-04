//! Independent correspondence mutations that ordinary type/flow checks miss.
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

#[test]
fn independent_relation_rejects_same_type_field_redirection() {
    let (frozen, mut typed) = fixture("type Pair = { x: i32; y: i32 } fn get(read p: Pair) { p.x } fn main() { let p = record Pair { x = 3; y = 8 }; get(p) }");
    let Computation::Value(body) = &mut typed.functions[0].body else {
        panic!()
    };
    let ExprKind::FieldRead(place) = &mut body.kind else {
        panic!()
    };
    assert_eq!(place.path.len(), 1);
    place.path[0].field = 1;
    // Well typed and same flow; only the source relation establishes the field.
    crate::typed::verify(&typed).unwrap();
    assert!(frozen.verify_structure(&typed).is_err());
}

#[test]
fn independent_relation_rejects_binder_redirect_without_type_or_flow_change() {
    let (frozen, mut typed) = fixture("fn pick(a, b) { a } fn main() { pick(3, 8) }");
    let other = typed.functions[0].params[1].0;
    let Computation::Value(body) = &mut typed.functions[0].body else {
        panic!()
    };
    let ExprKind::Read(local) = &mut body.kind else {
        panic!()
    };
    *local = other;
    crate::typed::verify(&typed).unwrap();
    assert!(frozen.verify_structure(&typed).is_err());
}

#[test]
fn independent_relation_rejects_pattern_and_skipped_branch_changes() {
    let (frozen, mut typed) =
        fixture("fn pick(n) { match n { 0 => 3 | _ => 8 } } fn main() { pick(0) }");
    let Computation::Value(body) = &mut typed.functions[0].body else {
        panic!()
    };
    let ExprKind::CopyMatch { arms, .. } = &mut body.kind else {
        panic!()
    };
    arms[0].pattern = crate::typed::CopyPattern::Int(1);
    crate::typed::verify(&typed).unwrap();
    assert!(frozen.verify_structure(&typed).is_err());
}

#[test]
fn independent_relation_rejects_stale_source_and_canonical_metadata() {
    let original = "fn spin() { spin() } fn main() { let unused = spin(); 42 }";
    let (mut frozen, typed) = fixture(original);
    let changed =
        crate::parser::parse("fn spin() { spin() } fn main() { let unused = spin(); 41 }").unwrap();
    let checked = crate::check::analyze(&changed).unwrap();
    assert!(frozen.verify_source(&checked).is_err());
    let opaque = frozen.binders.first_mut().unwrap();
    opaque.ordinal = usize::MAX;
    assert!(frozen.verify_structure(&typed).is_err());
}

#[test]
fn independent_relation_retains_unused_native_configuration_and_manifest() {
    let (frozen, original) =
        fixture("@include(\"stdint.h\"); extern fn unused(x: i32) -> i32; fn main() { 42 }");
    let mut typed = original.clone();
    typed.native.functions.clear();
    crate::typed::verify(&typed).unwrap();
    assert!(frozen.verify_structure(&typed).is_err());
    let mut typed = original;
    typed.native.config.includes.clear();
    crate::typed::verify(&typed).unwrap();
    assert!(frozen.verify_structure(&typed).is_err());
}

#[test]
fn independent_retained_source_depth_128_boundary_is_checked_first() {
    for (length, complexity) in [(128, false), (129, true)] {
        let (mut frozen, typed) = fixture("fn main() { 42 }");
        let mut parent = frozen.witness.functions[0].body;
        for _ in 0..length {
            let id = NodeId(frozen.witness.nodes.len());
            frozen.witness.nodes.push(SourceNode {
                source: 0,
                parent: Some((parent, Role::Left)),
                elaborated_local: false,
                ty: Type::Int,
                kind: SourceKind::Int(0),
            });
            parent = id;
        }
        let error = frozen.verify_structure(&typed).unwrap_err();
        assert_eq!(
            error.contains("CONTROL-COMPLEXITY"),
            complexity,
            "{length}: {error}"
        );
    }
}
