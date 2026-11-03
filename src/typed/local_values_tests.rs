//! These assert reconstruction facts separately from the concrete IR verifier.
use super::*;
use crate::{check, parser};

const MARK: &str =
    "type Mark<a> = Mark(i32)\nfn ints(x: Mark<i32>) { 40 }\nfn bools(x: Mark<bool>) { 2 }\n";
fn lower(source: &str) -> TypedMain {
    let p = parser::parse(source).unwrap();
    lower_main(&check::analyze(&p).unwrap()).unwrap()
}
fn tail(expr: &ExprNode) -> &ExprNode {
    if let ExprKind::Let { body, .. } = &expr.kind {
        tail(body)
    } else {
        expr
    }
}
fn tail_mut(expr: &mut ExprNode) -> &mut ExprNode {
    if matches!(expr.kind, ExprKind::Let { .. }) {
        let ExprKind::Let { body, .. } = &mut expr.kind else {
            unreachable!()
        };
        tail_mut(body)
    } else {
        expr
    }
}
fn argument(expr: &ExprNode) -> &ExprNode {
    let ExprKind::Call(_, args) = &expr.kind else {
        panic!("expected call")
    };
    let [CallArg::Value(value)] = &args[..] else {
        panic!("expected one value")
    };
    value
}
fn argument_mut(expr: &mut ExprNode) -> &mut ExprNode {
    let ExprKind::Call(_, args) = &mut expr.kind else {
        panic!("expected call")
    };
    let [CallArg::Value(value)] = &mut args[..] else {
        panic!("expected one value")
    };
    value
}
fn rejected(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(evaluate(program).is_err());
    assert!(emit_c(program).is_err());
}

#[test]
fn reconstructs_one_snapshot_and_two_exact_instances_with_shared_alias_capture() {
    let ir = lower(&format!("{MARK}fn main() {{ let source = 7; let p = Mark(source); let alias = p; ints(alias) + bools(p) }}"));
    let ExprKind::Let {
        local: source,
        value,
        body,
        ..
    } = &ir.functions[ir.entry].body.kind
    else {
        panic!()
    };
    assert!(matches!(value.kind, ExprKind::Int(7)));
    let ExprKind::Let {
        local: capture,
        mutable,
        value,
        body,
    } = &body.kind
    else {
        panic!()
    };
    assert!(!mutable);
    assert_ne!(source, capture);
    let ExprKind::Record {
        adt: environment,
        fields: captures,
    } = &value.kind
    else {
        panic!()
    };
    assert_eq!(ir.adts[*environment].origin, AdtOrigin::LocalCapture);
    assert_eq!(captures.len(), 1);
    assert!(matches!(captures[0].1.kind, ExprKind::Read(id) if id == *source));
    let ExprKind::Binary(_, left, right) = &body.kind else {
        panic!("an alias must not add a new capture")
    };
    let left = argument(left);
    let right = argument(right);
    assert_ne!(left.ty, right.ty);
    for (value, parameter) in [(left, Type::Int), (right, Type::Bool)] {
        let ExprKind::Constructor {
            adt,
            variant,
            fields,
        } = &value.kind
        else {
            panic!()
        };
        assert_eq!(
            value.ty,
            Type::Adt {
                id: *adt,
                owned: false
            }
        );
        assert_eq!(ir.adts[*adt].name, "Mark");
        assert_eq!(ir.adts[*adt].arguments, vec![parameter]);
        assert_eq!(ir.adts[*adt].variants[*variant].name, "Mark");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].ty, Type::Int);
        assert!(
            matches!(&fields[0].kind, ExprKind::FieldRead(place) if place.root == *capture && place.path == vec![FieldStep { record: *environment, field: 0 }])
        );
    }
}

#[test]
fn unused_shapes_keep_written_leaf_order_and_have_no_fabricated_layout() {
    let ir = lower("type O<a> = None | Some(a)\ntype R<a,b> = { first: i32; second: i32; unused: O<a> }\nfn main() { let unused = record R { second = 20; unused = None; first = 10 }; let alias = unused; 42 }");
    assert_eq!(
        ir.adts.len(),
        1,
        "only the concrete capture product is stored"
    );
    assert_eq!(ir.adts[0].origin, AdtOrigin::LocalCapture);
    assert_eq!(ir.adts[0].arguments, vec![Type::Int, Type::Int]);
    let ExprKind::Let {
        value,
        body,
        mutable,
        ..
    } = &ir.functions[ir.entry].body.kind
    else {
        panic!()
    };
    assert!(!mutable);
    let ExprKind::Record { fields, .. } = &value.kind else {
        panic!()
    };
    assert_eq!(fields.len(), 2);
    assert!(matches!(fields[0].1.kind, ExprKind::Int(20)));
    assert!(matches!(fields[1].1.kind, ExprKind::Int(10)));
    assert!(matches!(body.kind, ExprKind::Int(42)));
}

#[test]
fn field_only_uses_materialize_only_their_proven_closed_subshapes() {
    let ir = lower("type O<a> = None | Some(a)\ntype R<a,b> = { fixed: i32; value: O<a> }\nfn use(x: O<i32>) { 40 }\nfn main() { let p = record R { value = None; fixed = 2 }; use(p.value) + p.fixed }");
    assert_eq!(ir.adts.len(), 2);
    assert_eq!(ir.adts[1].origin, AdtOrigin::LocalCapture);
    assert_eq!(ir.adts[0].name, "O");
    assert_eq!(ir.adts[0].arguments, [Type::Int]);
    let ExprKind::Let {
        local, value, body, ..
    } = &ir.functions[ir.entry].body.kind
    else {
        panic!()
    };
    let ExprKind::Record { fields, .. } = &value.kind else {
        panic!()
    };
    assert!(matches!(fields[0].1.kind, ExprKind::Int(2)));
    let ExprKind::Binary(_, call, read) = &body.kind else {
        panic!()
    };
    assert!(
        matches!(argument(call).kind, ExprKind::Constructor { adt: 0, variant: 0, ref fields } if fields.is_empty())
    );
    assert!(
        matches!(&read.kind, ExprKind::FieldRead(place) if place.root == *local && place.path.len() == 1)
    );
}

#[test]
fn source_binding_identities_and_use_types_survive_shadowing_and_projection() {
    let source = "type O<a> = None | Some(a)\ntype R<a> = { value: O<a> }\nfn integer(x: O<i32>) { 40 }\nfn boolean(x: O<bool>) { true }\nfn main() { let p = record R { value = None }; let old = p; let n = { let p = record R { value = None }; integer(p.value) }; if boolean(old.value) { n + 2 } else { 0 } }";
    let p = parser::parse(source).unwrap();
    let checked = check::analyze(&p).unwrap();
    let Expr::Let(_, _, outer_body) = &p.functions.last().unwrap().body else {
        panic!()
    };
    let outer = checked
        .polymorphic_local(&p.functions.last().unwrap().body)
        .unwrap();
    let Expr::Let(_, alias_value, alias_body) = &**outer_body else {
        panic!()
    };
    let alias = checked.polymorphic_local(outer_body).unwrap();
    assert_ne!(outer, alias);
    assert_eq!(checked.local_use(alias_value), Some(outer));
    let Expr::Let(_, inner, after) = &**alias_body else {
        panic!()
    };
    let inner_identity = checked.polymorphic_local(inner).unwrap();
    assert_ne!(inner_identity, outer);
    assert_ne!(inner_identity, alias);
    let Expr::Let(_, _, inner_call) = &**inner else {
        panic!()
    };
    let Expr::Call(_, inner_args) = &**inner_call else {
        panic!()
    };
    assert_eq!(checked.local_use(&inner_args[0]), Some(inner_identity));
    assert_eq!(
        checked.expression_type(&inner_args[0]),
        Some(&InferredType::Nominal("O".into(), vec![InferredType::Int]))
    );
    let Expr::If(condition, _, _) = &**after else {
        panic!()
    };
    let Expr::Call(_, outer_args) = &**condition else {
        panic!()
    };
    assert_eq!(checked.local_use(&outer_args[0]), Some(alias));
    assert_eq!(
        checked.expression_type(&outer_args[0]),
        Some(&InferredType::Nominal("O".into(), vec![InferredType::Bool]))
    );
    let ir = lower_main(&checked).unwrap();
    assert_eq!(evaluate(&ir), crate::interpreter::run_reference(&p));
}

#[test]
fn concrete_verifier_rejects_forged_capture_type_scope_and_nominal_instance() {
    let ir = lower(&format!(
        "{MARK}fn main() {{ let p = Mark(7); let alias = p; ints(alias) + bools(p) }}"
    ));
    let mut bad = ir.clone();
    let ExprKind::Let { value, .. } = &mut bad.functions[bad.entry].body.kind else {
        panic!()
    };
    **value = ExprNode {
        ty: Type::Bool,
        kind: ExprKind::Bool(true),
    };
    rejected(&bad);
    let mut bad = ir.clone();
    let ExprKind::Binary(_, left, _) = &mut tail_mut(&mut bad.functions[bad.entry].body).kind
    else {
        panic!()
    };
    let ExprKind::Constructor { fields, .. } = &mut argument_mut(left).kind else {
        panic!()
    };
    fields[0].kind = ExprKind::Read(LocalId(u32::MAX));
    rejected(&bad);
    let mut bad = ir.clone();
    let ExprKind::Binary(_, left, right) = &mut tail_mut(&mut bad.functions[bad.entry].body).kind
    else {
        panic!()
    };
    let target = argument(right).ty;
    argument_mut(left).ty = target;
    rejected(&bad);
    let mut bad = ir.clone();
    let ExprKind::Let { value, .. } = &mut bad.functions[bad.entry].body.kind else {
        panic!()
    };
    value.ty = Type::OwnedI32;
    rejected(&bad);
}

#[test]
fn generalized_read_is_an_exact_read_temporary_not_a_forged_root_loan() {
    let ir = lower(
        "type M<a> = M(i32)\nfn take(read x: M<i32>) { 42 }\nfn main() { let p = M(7); take(p) }",
    );
    let ExprKind::Let { local, body, .. } = &ir.functions[ir.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &body.kind else {
        panic!()
    };
    let [CallArg::ReadTemporary(value)] = &args[..] else {
        panic!()
    };
    assert!(matches!(value.kind, ExprKind::Constructor { .. }));
    for operand in [CallArg::ReadCopy(*local), CallArg::InoutCopy(*local)] {
        let mut bad = ir.clone();
        let ExprKind::Call(_, args) = &mut tail_mut(&mut bad.functions[bad.entry].body).kind else {
            panic!()
        };
        args[0] = operand;
        rejected(&bad);
    }
}

#[test]
fn snapshots_are_not_eliminated_when_uses_are_unreachable() {
    let ir = lower(&format!(
        "{MARK}fn main() {{ let p = Mark(7); if false {{ ints(p) + bools(p) }} else {{ 42 }} }}"
    ));
    let ExprKind::Let { value, body, .. } = &ir.functions[ir.entry].body.kind else {
        panic!()
    };
    let ExprKind::Record { fields, .. } = &value.kind else {
        panic!()
    };
    assert!(matches!(fields[0].1.kind, ExprKind::Int(7)));
    assert!(matches!(tail(body).kind, ExprKind::If(..)));
}

#[test]
fn capture_layout_origin_is_explicit_and_storage_identity_is_not_layout_identity() {
    let ir = lower("type M<a> = M(i32)\nfn ints(x: M<i32>) { match x { M(n) => n } }\nfn bools(x: M<bool>) { match x { M(n) => n } }\nfn main() { let p = M(20); let old = p; let p = M(1); ints(old) + bools(old) + ints(p) + bools(p) }");
    let environments: Vec<_> = ir
        .adts
        .iter()
        .enumerate()
        .filter(|(_, adt)| adt.origin == AdtOrigin::LocalCapture)
        .collect();
    assert_eq!(
        environments.len(),
        1,
        "equal capture products can share a layout"
    );
    let (environment, definition) = environments[0];
    assert_eq!(definition.arguments, [Type::Int]);
    let ExprKind::Let {
        local: old, body, ..
    } = &ir.functions[ir.entry].body.kind
    else {
        panic!()
    };
    let ExprKind::Let { local: new, .. } = &body.kind else {
        panic!()
    };
    assert_ne!(old, new, "shadowed bindings never share storage identity");
    assert_eq!(evaluate(&ir).unwrap().to_string(), "42");
    let mut bad = ir.clone();
    bad.adts[environment].origin = AdtOrigin::Source;
    rejected(&bad);
    let mut bad = ir.clone();
    bad.adts[environment].name = "SourceRecord".into();
    rejected(&bad);
    let mut bad = ir.clone();
    bad.adts[environment].arguments = vec![Type::Bool];
    rejected(&bad);
    let mut bad = ir.clone();
    bad.adts[environment].arguments = vec![Type::OwnedI32];
    bad.adts[environment].variants[0].fields = vec![Type::OwnedI32];
    bad.adts[environment].owned = true;
    rejected(&bad);
}

#[test]
fn capture_verifier_rejects_owner_phantoms_hidden_behind_nominal_fields() {
    let phantom = AdtType {
        symbolic: None,
        origin: AdtOrigin::Source,
        name: "Phantom".into(),
        arguments: vec![Type::OwnedI32],
        owned: false,
        record_fields: None,
        variants: vec![AdtVariant {
            name: "Marker".into(),
            fields: vec![],
        }],
    };
    let outer = AdtType {
        symbolic: None,
        origin: AdtOrigin::Source,
        name: "Outer".into(),
        arguments: vec![],
        owned: false,
        record_fields: None,
        variants: vec![AdtVariant {
            name: "Outer".into(),
            fields: vec![Type::Adt {
                id: 0,
                owned: false,
            }],
        }],
    };
    // Both source layouts are physically Copy, but local value restriction
    // deliberately excludes the transitive owner-valued phantom dependency.
    adts::verify(&[phantom.clone(), outer.clone()]).unwrap();
    let payload = Type::Adt {
        id: 1,
        owned: false,
    };
    let capture = AdtType {
        symbolic: None,
        origin: AdtOrigin::LocalCapture,
        name: local_values::CAPTURE_NAME.into(),
        arguments: vec![payload],
        owned: false,
        record_fields: Some(vec!["capture_0".into()]),
        variants: vec![AdtVariant {
            name: local_values::CAPTURE_NAME.into(),
            fields: vec![payload],
        }],
    };
    assert!(adts::verify(&[phantom, outer, capture])
        .unwrap_err()
        .contains("owner-dependent"));
}
