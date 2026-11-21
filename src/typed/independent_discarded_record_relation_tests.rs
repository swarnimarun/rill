//! Independent retained-source record proof probes.
//! Private child of typed::control::relation.
use super::*;
use crate::typed::{self, Computation, DiscardExpr, DiscardKind, ExprKind, ResultRepr, TypedMain};
const CORE:&str="type O<a> = None|Some(a) type R<a> = {left:i32;hole:O<a>;right:i32} fn made(){record R{right=11;hole=None;left=22}} fn main(){let weak=made();42}";
fn fixture(source: &str) -> (Frozen, TypedMain) {
    let ast = crate::parser::parse(source).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    let (typed, instances) = typed::lower_main_traced(&checked, None, None).unwrap();
    let frozen = SourceFacts::build(&checked)
        .unwrap()
        .freeze(&instances)
        .unwrap();
    frozen.verify_source(&checked).unwrap();
    frozen.verify_structure(&typed).unwrap();
    (frozen, typed)
}
fn reject(frozen: Frozen, mut typed: TypedMain) {
    typed.logical.control = Some(std::sync::Arc::new(frozen));
    let error = typed::verify(&typed).unwrap_err();
    assert_eq!(typed::evaluate(&typed).unwrap_err(), error);
    assert_eq!(typed::emit_c(&typed).unwrap_err(), error);
}
fn maker(typed: &TypedMain) -> usize {
    typed
        .functions
        .iter()
        .position(|f| {
            matches!(
                &f.body,
                Computation::Discarded(DiscardExpr {
                    kind: DiscardKind::Record { .. },
                    ..
                })
            )
        })
        .unwrap()
}
fn fields(typed: &mut TypedMain, index: usize) -> &mut Vec<(usize, DiscardExpr)> {
    let Computation::Discarded(DiscardExpr {
        kind: DiscardKind::Record { fields, .. },
        ..
    }) = &mut typed.functions[index].body
    else {
        panic!()
    };
    fields
}
#[test]
fn independent_k4r_full_retained_record_schema_is_not_an_active_field_summary() {
    for action in 0..7 {
        let (mut frozen, mut typed) = fixture(CORE);
        for declaration in [
            &mut frozen.witness.environment.types[1],
            &mut typed.logical.declarations[1],
        ] {
            match action {
                0 => declaration.record_fields = None,
                1 => declaration.record_fields.as_mut().unwrap()[2] = "left".into(),
                2 => declaration.record_fields.as_mut().unwrap()[2].clear(),
                3 => declaration.record_fields.as_mut().unwrap()[2] = "alien".into(),
                4 => {
                    declaration.variants[0].fields.pop();
                }
                5 => declaration.variants.push(declaration.variants[0].clone()),
                _ => declaration.params.push("unexpected".into()),
            }
        }
        reject(frozen, typed);
    }
}
#[test]
fn independent_k4r_inactive_schema_owner_native_rejects_in_body_and_omitted_suffix() {
    let internal="type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn main(){let weak=if true{record R{hole=None}}else{record R{hole=None}};42}";
    let suffix="type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),record R{hole=None})}";
    for source in [CORE, internal, suffix] {
        for forbidden in ["SystemBoxI32", "u32"] {
            let (mut frozen, mut typed) = fixture(source);
            assert!(typed.adts.is_empty());
            frozen.witness.environment.types[0].variants[1].fields[0].name = forbidden.into();
            typed.logical.declarations[0].variants[1].fields[0].name = forbidden.into();
            reject(frozen, typed);
        }
    }
}
#[test]
fn independent_k4r_record_role_is_written_ordinal_not_schema_index() {
    for action in 0..3 {
        let (mut frozen, typed) = fixture(CORE);
        let parent = frozen
            .witness
            .nodes
            .iter()
            .position(|n| matches!(n.kind, SourceKind::Record(..)))
            .unwrap();
        let SourceKind::Record(_, children) = &frozen.witness.nodes[parent].kind else {
            panic!()
        };
        let first = children[0].1;
        let last = children[2].1;
        match action {
            0 => {
                frozen.witness.nodes[first.0].parent = Some((NodeId(parent), Role::RecordField(2)))
            }
            1 => frozen.witness.nodes[first.0].parent = Some((NodeId(parent), Role::Argument(0))),
            _ => {
                let SourceKind::Record(_, children) = &mut frozen.witness.nodes[parent].kind else {
                    panic!()
                };
                children[0].1 = last;
            }
        }
        reject(frozen, typed);
    }
}
#[test]
fn independent_k4r_retained_record_source_name_labels_and_child_occurrences_are_exact() {
    for action in 0..4 {
        let (mut frozen, typed) = fixture(CORE);
        let node = frozen
            .witness
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, SourceKind::Record(..)))
            .unwrap();
        let SourceKind::Record(name, children) = &mut node.kind else {
            panic!()
        };
        match action {
            0 => *name = "O".into(),
            1 => children[0].0 = "left".into(),
            2 => children.swap(0, 2),
            _ => {
                children.pop();
            }
        }
        reject(frozen, typed);
    }
}
#[test]
fn independent_k4r_record_discard_requires_independent_lexical_nonuse() {
    for action in 0..3 {
        let (mut frozen, typed) = fixture(CORE);
        let binding = frozen
            .witness
            .bindings
            .iter()
            .position(|b| b.name == "weak")
            .unwrap();
        assert_eq!(frozen.witness.bindings[binding].uses, 0);
        let tail = frozen
            .witness
            .nodes
            .iter()
            .position(|n| n.kind == SourceKind::Int(42))
            .unwrap();
        match action {
            0 => {
                frozen.witness.nodes[tail].kind =
                    SourceKind::Var(Reference::Local(BindingId(binding)))
            }
            1 => {
                frozen.witness.nodes[tail].kind =
                    SourceKind::Field(Reference::Local(BindingId(binding)), vec!["left".into()])
            }
            _ => frozen.witness.bindings[binding].mutable = true,
        }
        reject(frozen, typed);
    }
}
#[test]
fn independent_k4r_consistent_record_pair_permutation_still_fails_original_source() {
    let (mut frozen, mut typed) = fixture(CORE);
    let made = maker(&typed);
    fields(&mut typed, made).swap(0, 2);
    let parent = frozen
        .witness
        .nodes
        .iter()
        .position(|n| matches!(n.kind, SourceKind::Record(..)))
        .unwrap();
    let SourceKind::Record(_, children) = &mut frozen.witness.nodes[parent].kind else {
        panic!()
    };
    children.swap(0, 2);
    let links = children
        .iter()
        .enumerate()
        .map(|(ordinal, (_, child))| (*child, ordinal))
        .collect::<Vec<_>>();
    for (child, ordinal) in links {
        frozen.witness.nodes[child.0].parent = Some((NodeId(parent), Role::RecordField(ordinal)));
    }
    frozen.verify_structure(&typed).unwrap();
    let ast = crate::parser::parse(CORE).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    assert!(frozen.verify_source(&checked).is_err());
    typed.logical.control = Some(std::sync::Arc::new(frozen));
    typed::verify(&typed).unwrap();
}
#[test]
fn independent_k4r_consistent_field_literal_change_still_fails_original_source() {
    let (mut frozen, mut typed) = fixture(CORE);
    let made = maker(&typed);
    let DiscardKind::Value(value) = &mut fields(&mut typed, made)[0].1.kind else {
        panic!()
    };
    value.kind = ExprKind::Int(33);
    let literal = frozen
        .witness
        .nodes
        .iter_mut()
        .find(|n| n.kind == SourceKind::Int(11))
        .unwrap();
    literal.kind = SourceKind::Int(33);
    frozen.verify_structure(&typed).unwrap();
    let ast = crate::parser::parse(CORE).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    assert!(frozen.verify_source(&checked).is_err());
    typed.logical.control = Some(std::sync::Arc::new(frozen));
    typed::verify(&typed).unwrap();
}
#[test]
fn independent_k4r_normal_record_is_positive_flow_never_nonreturn_evidence() {
    let(frozen,mut typed)=fixture("type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn made(){record R{hole=None}} fn spin(){spin()} fn main(){let weak=made();spin()}");
    let made = maker(&typed);
    let main = typed.entry;
    let before = IrFacts::build(&typed).unwrap();
    assert_eq!(before.function_may_return(made), Some(true));
    assert_eq!(before.function_may_return(main), Some(false));
    drop(before);
    let Computation::NoNormal(body) = &mut typed.functions[main].body else {
        panic!()
    };
    let crate::typed::NoNormalKind::DiscardLet { value, .. } = &mut body.kind else {
        panic!()
    };
    let normal = value.as_ref().clone();
    typed.functions[main].body = Computation::Discarded(normal);
    assert!(matches!(
        typed.functions[main].result,
        ResultRepr::NoNormal(_)
    ));
    assert_eq!(
        IrFacts::build(&typed).unwrap().function_may_return(main),
        Some(true)
    );
    reject(frozen, typed);
}
#[test]
fn independent_k4r_recursive_field_is_an_actual_flow_edge_not_an_erased_payload() {
    let(frozen,mut typed)=fixture("type O<a> = None|Some(a) type S<a,b> = {stop:a;hole:O<b>} fn spin(){spin()} fn made(){record S{stop=spin();hole=None}} fn main(){let weak=made();42}");
    let made = maker(&typed);
    assert_eq!(
        IrFacts::build(&typed).unwrap().function_may_return(made),
        Some(false)
    );
    let returning = fields(&mut typed, made)[1].1.clone();
    fields(&mut typed, made)[0].1 = returning;
    assert_eq!(
        IrFacts::build(&typed).unwrap().function_may_return(made),
        Some(true)
    );
    reject(frozen, typed);
}
