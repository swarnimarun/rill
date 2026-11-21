use super::*;
fn sample() -> TypedMain {
    let source=crate::parser::parse("type Inner<a> = Payload(a) | Missing\ntype Outer<a> = Wrapped(Inner<a>, i32) | Empty\nlet main = let owner = system_box_i32_abort(42) in let wrapped = Wrapped(Payload(move owner), 7) in match move wrapped { Wrapped(inner, n) => match move inner { Payload(value) => box_read(value) | Missing => n } | Empty => 0 }").unwrap();
    let checked = crate::check::analyze(&source).unwrap();
    lower_main(&checked).unwrap()
}
fn rejects(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(emit_c(program).is_err());
    assert!(evaluate(program).is_err());
}
#[test]
fn metadata_is_independently_validated_before_execution_or_emission() {
    let original = sample();
    verify(&original).unwrap();
    let mut forged = original.clone();
    forged.adts[0].owned = false;
    rejects(&forged);
    let mut forged = original.clone();
    forged.adts[0].variants[0].fields[0] = Type::Adt { id: 0, owned: true };
    rejects(&forged);
    let mut forged = original.clone();
    forged.adts[0].variants[0].fields[0] = Type::Adt {
        id: 999,
        owned: true,
    };
    rejects(&forged);
    let mut forged = original.clone();
    forged.adts[0].variants[0].name = forged.adts[0].variants[1].name.clone();
    rejects(&forged);
    let mut forged = original.clone();
    forged.adts.push(forged.adts[0].clone());
    rejects(&forged);
    let mut forged = original;
    forged.adts[1].variants[0].fields[0] = Type::Adt {
        id: 0,
        owned: false,
    };
    rejects(&forged);
}
fn constructor(program: &mut TypedMain) -> &mut ExprNode {
    let ExprKind::OwnerLet { body, .. } = &mut program.functions[program.entry]
        .body
        .value_expr_mut()
        .unwrap()
        .kind
    else {
        panic!()
    };
    let ExprKind::OwnerLet {
        init: OwnerInit::Expr(value),
        ..
    } = &mut body.kind
    else {
        panic!()
    };
    value
}
#[test]
fn constructor_tags_fields_and_declared_types_are_validated() {
    let original = sample();
    let mut forged = original.clone();
    let ExprKind::Constructor { variant, .. } = &mut constructor(&mut forged).kind else {
        panic!()
    };
    *variant = 999;
    rejects(&forged);
    let mut forged = original.clone();
    let ExprKind::Constructor { fields, .. } = &mut constructor(&mut forged).kind else {
        panic!()
    };
    fields.pop();
    rejects(&forged);
    let mut forged = original.clone();
    let ExprKind::Constructor { fields, .. } = &mut constructor(&mut forged).kind else {
        panic!()
    };
    fields[1] = ExprNode {
        ty: Type::Bool,
        kind: ExprKind::Bool(true),
    };
    rejects(&forged);
    let mut forged = original;
    let node = constructor(&mut forged);
    let Type::Adt { id, .. } = node.ty else {
        panic!()
    };
    node.ty = Type::Adt { id, owned: false };
    rejects(&forged);
}
fn match_node(program: &mut TypedMain) -> &mut ExprNode {
    let ExprKind::OwnerLet { body, .. } = &mut program.functions[program.entry]
        .body
        .value_expr_mut()
        .unwrap()
        .kind
    else {
        panic!()
    };
    let ExprKind::OwnerLet { body, .. } = &mut body.kind else {
        panic!()
    };
    body
}
#[test]
fn match_coverage_payload_cleanup_and_scope_are_validated() {
    let original = sample();
    let mut forged = original.clone();
    let ExprKind::Match { arms, .. } = &mut match_node(&mut forged).kind else {
        panic!()
    };
    arms[1].variant = arms[0].variant;
    rejects(&forged);
    let mut forged = original.clone();
    let ExprKind::Match { arms, .. } = &mut match_node(&mut forged).kind else {
        panic!()
    };
    arms.pop();
    rejects(&forged);
    let mut forged = original.clone();
    let ExprKind::Match { arms, .. } = &mut match_node(&mut forged).kind else {
        panic!()
    };
    arms[0].fields[0] = None;
    rejects(&forged);
    let mut forged = original.clone();
    let ExprKind::Match { arms, .. } = &mut match_node(&mut forged).kind else {
        panic!()
    };
    let ExprKind::Match { arms, .. } = &mut arms[0].body.kind else {
        panic!()
    };
    arms[0].cleanup.clear();
    rejects(&forged);
    let mut forged = original;
    let ExprKind::Match { arms, .. } = &mut match_node(&mut forged).kind else {
        panic!()
    };
    arms[1].fields = arms[0].fields.clone();
    rejects(&forged);
}
#[test]
fn inline_layout_cost_rejects_exponential_expansion() {
    let mut types = vec![AdtType {
        symbolic: None,
        origin: super::AdtOrigin::Source,
        record_fields: None,
        name: "Leaf".into(),
        arguments: vec![],
        variants: vec![AdtVariant {
            name: "Leaf".into(),
            fields: vec![Type::Int],
        }],
        owned: false,
    }];
    for id in 1..30 {
        types.push(AdtType {
            symbolic: None,
            origin: super::AdtOrigin::Source,
            record_fields: None,
            name: format!("Tree{id}"),
            arguments: vec![],
            variants: vec![AdtVariant {
                name: format!("Branch{id}"),
                fields: vec![
                    Type::Adt {
                        id: id - 1,
                        owned: false
                    };
                    2
                ],
            }],
            owned: false,
        });
    }
    assert!(adts::verify(&types).unwrap_err().contains("layout"));
}
#[test]
fn independent_nominal_layouts_emit_deterministically() {
    let source="type A = A(i32)\ntype B = B(bool)\nlet first = A(7)\nlet second = B(true)\nlet main = let owner=system_box_i32_abort(42) in let a=first() in let b=second() in box_read(owner)";
    let expected = crate::compile(source).unwrap();
    for _ in 0..12 {
        assert_eq!(crate::compile(source).unwrap(), expected);
    }
}

#[test]
fn metadata_names_cannot_escape_c_string_literals() {
    let mut program = sample();
    program.adts.push(AdtType {
        symbolic: None,
        origin: super::AdtOrigin::Source,
        record_fields: None,
        name: "Copy".into(),
        arguments: vec![],
        variants: vec![AdtVariant {
            name: "unsafe\"\nname".into(),
            fields: vec![],
        }],
        owned: false,
    });
    let c = emit_c(&program).unwrap();
    assert!(!c.contains("unsafe\"\nname"));
    assert!(c.contains("\\165\\156\\163\\141\\146\\145\\042\\012\\156\\141\\155\\145"));
}

#[test]
fn primitive_matches_use_the_shared_typed_path() {
    for source in [
        "let main = match 42 { 42 => 7 | _ => 0 }",
        "let main = match true { true => 7 | false => 0 }",
        "let main = match () { () => 7 }",
    ] {
        let program = crate::parser::parse(source).unwrap();
        let checked = crate::check::analyze(&program).unwrap();
        assert!(!checked.has_owned());
        let typed = lower_main(&checked).unwrap();
        assert_eq!(
            evaluate(&typed),
            crate::interpreter::run_reference(&program)
        );
        assert_eq!(
            crate::backend::emit(&program).unwrap(),
            emit_c(&typed).unwrap()
        );
    }
}
