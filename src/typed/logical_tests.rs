//! Structural and source-boundary tests for frozen logical enum arguments.
use super::*;
use crate::{check, parser};
use logical::LogicalType;

fn lower(source: &str) -> TypedMain {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    lower_main(&checked).unwrap()
}
fn reject(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(evaluate(program).is_err());
    assert!(emit_c(program).is_err());
}
fn flag() -> TypedMain {
    lower("type Flag<a> = Off | On\nfn main() { Off }")
}
fn symbolic(program: &TypedMain) -> usize {
    program
        .adts
        .iter()
        .position(|adt| adt.symbolic.is_some())
        .unwrap()
}

#[test]
fn opaque_arguments_are_not_runtime_types_or_default_witnesses() {
    let program = flag();
    let id = symbolic(&program);
    let adt = &program.adts[id];
    assert!(adt.arguments.is_empty());
    assert!(!adt.owned);
    assert!(adt.variants.iter().all(|variant| variant.fields.is_empty()));
    assert_eq!(
        adt.symbolic.as_ref().unwrap().arguments,
        [LogicalType::Opaque(program.logical.opaque_id(0).unwrap())]
    );
    assert_eq!(
        program.logical.binders,
        [logical::OpaqueBinding {
            scope: 0,
            ordinal: 0
        }]
    );
    assert_eq!(
        program.functions[program.entry].result,
        Type::Adt { id, owned: false }
    );
    assert_eq!(evaluate(&program).unwrap().to_string(), "Off");
    let output = emit_c(&program).unwrap();
    assert!(!output.contains("rl_value") && !output.contains("RL_CAP"));
}

#[test]
fn complete_schema_identity_arity_and_order_are_rechecked_by_all_consumers() {
    let source = "type Flag<a> = Off | On\ntype Other<a> = Other\ntype O<a> = None | Some(a)\nfn main() { Off }";
    let original = lower(source);
    let id = symbolic(&original);
    for mutation in 0..16 {
        let mut program = original.clone();
        let definition = program.adts[id].symbolic.as_ref().unwrap().definition;
        match mutation {
            0 => program.adts[id].symbolic.as_mut().unwrap().definition = usize::MAX,
            1 => program.adts[id].symbolic.as_mut().unwrap().definition = 1,
            2 => program.adts[id]
                .symbolic
                .as_mut()
                .unwrap()
                .arguments
                .clear(),
            3 => program.adts[id]
                .symbolic
                .as_mut()
                .unwrap()
                .arguments
                .push(LogicalType::Opaque(program.logical.opaque_id(0).unwrap())),
            4 => {
                program.adts[id].variants.pop();
            }
            5 => program.adts[id].variants.swap(0, 1),
            6 => program.adts[id].variants[0].name = "Unknown".into(),
            7 => program.adts[id].variants[0].fields.push(Type::Int),
            8 => program.adts[id].arguments.push(Type::Int),
            9 => program.adts[id].origin = AdtOrigin::LocalCapture,
            10 => program.adts[id].record_fields = Some(Vec::new()),
            11 => program.adts[id].owned = true,
            12 => program.logical.declarations[definition]
                .params
                .push("extra".into()),
            13 => program.logical.declarations[definition]
                .variants
                .push(crate::ast::VariantDecl {
                    name: "Payload".into(),
                    fields: vec![crate::ast::TypeExpr {
                        name: "a".into(),
                        args: Vec::new(),
                    }],
                }),
            14 => {
                // A tag-only active None does not erase Some's unknown payload.
                program.adts[id].name = "O".into();
                program.adts[id].symbolic.as_mut().unwrap().definition = 2;
                program.adts[id].variants = vec![AdtVariant {
                    name: "None".into(),
                    fields: Vec::new(),
                }];
            }
            15 => program.adts[id].symbolic = None,
            _ => unreachable!(),
        }
        reject(&program);
    }
}

#[test]
fn canonical_schema_source_relation_is_checked_separately() {
    let source = "type Flag<a> = Off | On\ntype O<a> = None | Some(a)\nfn main() { Off }";
    let parsed = parser::parse(source).unwrap();
    let checked = check::analyze(&parsed).unwrap();
    let instances = instances::Instances::discover(&checked).unwrap();
    let original = logical::Metadata::from_checked(&checked, &instances).unwrap();
    original.validate_source(&checked, &instances).unwrap();
    for mutation in 0..6 {
        let mut metadata = original.clone();
        match mutation {
            0 => {
                metadata.declarations[1].variants.pop();
            }
            1 => metadata.declarations[0].variants.swap(0, 1),
            2 => metadata.declarations[0].params[0] = "changed".into(),
            3 => metadata.source_functions[0] = "changed".into(),
            4 => metadata.scope_sources[0] = usize::MAX,
            5 => metadata.binders[0].ordinal = 2,
            _ => unreachable!(),
        }
        assert!(metadata
            .validate_source(&checked, &instances)
            .unwrap_err()
            .contains("checked source"));
    }
    // A structural verifier checks the retained schema, not an absent source.
    // Removing an unused payload variant consistently can describe a different
    // valid program. It MUST still fail the separate source relationship check.
    let mut alternate = lower(source);
    alternate.logical.declarations[1].variants.pop();
    verify(&alternate).unwrap();
    assert!(alternate
        .logical
        .validate_source(&checked, &instances)
        .is_err());
}

#[test]
fn malformed_binders_and_logical_atoms_never_reach_runtime_consumers() {
    let original = flag();
    let id = symbolic(&original);
    for mutation in 0..11 {
        let mut program = original.clone();
        match mutation {
            0 => program.logical.binders.clear(),
            1 => program.logical.binders[0].scope = usize::MAX,
            2 => program.logical.binders[0].ordinal = 1,
            3 => program.logical.function_scopes[0] = usize::MAX,
            4 => program.logical.scope_sources[0] = usize::MAX,
            5 => program.logical.source_functions[0] = "not_main".into(),
            6 => {
                program.adts[id].symbolic.as_mut().unwrap().arguments[0] = LogicalType::Opaque({
                    let mut id = program.logical.opaque_id(0).unwrap();
                    id.index = usize::MAX;
                    id
                })
            }
            7 => {
                program.adts[id].symbolic.as_mut().unwrap().arguments[0] =
                    LogicalType::Atom(InferredType::Var(0))
            }
            8 => {
                program.adts[id].symbolic.as_mut().unwrap().arguments[0] = LogicalType::Atom(
                    InferredType::Nominal("Flag".into(), vec![InferredType::Int]),
                )
            }
            9 => {
                program.adts[id].symbolic.as_mut().unwrap().arguments[0] =
                    LogicalType::Atom(InferredType::CStr)
            }
            10 => program.adts[id].variants[0]
                .fields
                .push(Type::Adt { id, owned: false }),
            _ => unreachable!(),
        }
        reject(&program);
    }
}

#[test]
fn nested_logical_arguments_preserve_known_nominal_owner_and_pointer_dependencies() {
    let mut program = lower("type Flag<a> = Off | On\ntype Inner<a,b> = Inner(a,b)\ntype Raw<a> = Raw(ptr<void>)\nfn main() { Off }");
    let id = symbolic(&program);
    let make = |pointee: &str, is_const| {
        InferredType::Nominal(
            "Inner".into(),
            vec![
                InferredType::Var(0),
                InferredType::CPointer {
                    pointee: pointee.into(),
                    is_const,
                },
            ],
        )
    };
    let mutable = program
        .logical
        .instance("Flag", &[make("void", false)])
        .unwrap();
    let constant = program
        .logical
        .instance("Flag", &[make("void", true)])
        .unwrap();
    let other = program
        .logical
        .instance("Flag", &[make("Handle", false)])
        .unwrap();
    assert_ne!(mutable, constant);
    assert_ne!(mutable, other);
    assert!(mutable.dependencies.native);
    assert!(!mutable.dependencies.owner);
    program.adts[id].symbolic = Some(mutable);
    verify(&program).unwrap();
    assert!(provenance::has_native_representation(&program));
    assert_eq!(evaluate(&program).unwrap().to_string(), "Off");
    emit_c(&program).unwrap();
    let raw = program
        .logical
        .instance(
            "Flag",
            &[InferredType::Nominal(
                "Raw".into(),
                vec![InferredType::Var(0)],
            )],
        )
        .unwrap();
    assert!(raw.dependencies.native);
    // Inject a schema into this standalone IR fixture. A source program naming
    // an owner declaration and an open main result is intentionally rejected
    // by the unchanged source owner checker.
    program.logical.declarations.push(crate::ast::TypeDecl {
        name: "Owner".into(),
        params: vec!["a".into()],
        variants: vec![crate::ast::VariantDecl {
            name: "Owner".into(),
            fields: vec![crate::ast::TypeExpr {
                name: "SystemBoxI32".into(),
                args: Vec::new(),
            }],
        }],
        record_fields: None,
    });
    let owned = program
        .logical
        .instance(
            "Flag",
            &[InferredType::Nominal(
                "Owner".into(),
                vec![InferredType::Var(0)],
            )],
        )
        .unwrap();
    assert!(owned.dependencies.owner);
    program.adts[id].symbolic = Some(owned);
    verify(&program).unwrap();
    // Known owner dependence cannot disappear merely because it is erased.
    program.native.config.includes.push("ordinary.h".into());
    reject(&program);
}

#[test]
fn nested_logical_arity_identity_dependencies_and_depth_are_independently_checked() {
    let mut original =
        lower("type Flag<a> = Off | On\ntype Inner<a,b> = Inner(a,b)\nfn main() { Off }");
    let id = symbolic(&original);
    original.adts[id].symbolic = Some(
        original
            .logical
            .instance(
                "Flag",
                &[InferredType::Nominal(
                    "Inner".into(),
                    vec![
                        InferredType::Var(0),
                        InferredType::CPointer {
                            pointee: "void".into(),
                            is_const: true,
                        },
                    ],
                )],
            )
            .unwrap(),
    );
    verify(&original).unwrap();
    for mutation in 0..6 {
        let mut program = original.clone();
        let instance = program.adts[id].symbolic.as_mut().unwrap();
        match mutation {
            0 => instance.dependencies.native = false,
            1 => instance.dependencies.owner = true,
            2 => {
                let LogicalType::Nominal { arguments, .. } = &mut instance.arguments[0] else {
                    panic!()
                };
                arguments.pop();
            }
            3 => {
                let LogicalType::Nominal { definition, .. } = &mut instance.arguments[0] else {
                    panic!()
                };
                *definition = usize::MAX;
            }
            4 => {
                let LogicalType::Nominal { arguments, .. } = &mut instance.arguments[0] else {
                    panic!()
                };
                arguments[1] = LogicalType::Atom(InferredType::CPointer {
                    pointee: "bad;pointee".into(),
                    is_const: false,
                });
            }
            5 => {
                let mut value = LogicalType::Opaque(original.logical.opaque_id(0).unwrap());
                for _ in 0..130 {
                    value = LogicalType::Nominal {
                        definition: 0,
                        arguments: vec![value],
                    };
                }
                instance.arguments[0] = value;
            }
            _ => unreachable!(),
        }
        reject(&program);
    }
}

#[test]
fn repeated_calls_deduplicate_and_distinct_weak_slots_keep_distinct_binders() {
    let program = parser::parse("type Flag<a> = Off | On\nfn make() { Off }\nfn id(x) { x }\nfn main() { let f = make(); let g = make(); let a = id(f); let b = id(f); let c = id(g); 42 }").unwrap();
    let checked = check::analyze(&program).unwrap();
    let instances = instances::Instances::discover(&checked).unwrap();
    assert_eq!(
        instances
            .list
            .iter()
            .filter(|i| i.source == instances.functions["id"])
            .count(),
        2
    );
    assert_eq!(
        instances
            .list
            .iter()
            .filter(|i| i.source == instances.functions["make"])
            .count(),
        2
    );
    assert_eq!(instances.binders.len(), 2);
    let mut typed = lower_main(&checked).unwrap();
    let symbolic = typed
        .adts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.symbolic.is_some())
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(symbolic.len(), 2);
    assert_ne!(
        typed.adts[symbolic[0]].symbolic,
        typed.adts[symbolic[1]].symbolic
    );
    typed.adts[symbolic[0]].symbolic = typed.adts[symbolic[1]].symbolic.clone();
    reject(&typed);
}

#[test]
fn recursive_forwarding_never_alpha_renames_instance_keys() {
    for (source, functions) in [
        ("type Flag<a> = Off | On\nfn again(x) { if false { x } else { again(x) } }\nfn main() { again(Off) }", 2),
        ("type Flag<a> = Off | On\nfn a(x) { if false { x } else { b(x) } }\nfn b(x) { a(x) }\nfn main() { a(Off) }", 3),
        ("type Flag<a> = Off | On\nfn again() { if false { Off } else { again() } }\nfn main() { again() }", 2),
    ] {
        let parsed = parser::parse(source).unwrap();
        let checked = check::analyze(&parsed).unwrap();
        let instances = instances::Instances::discover(&checked).unwrap();
        assert_eq!(instances.list.len(), functions);
        assert_eq!(instances.binders.len(), 1);
        let typed = lower_main(&checked).unwrap();
        verify(&typed).unwrap();
        emit_c(&typed).unwrap();
        // The separate source suite covers the bounded call-depth trap. This
        // identity test does not execute a divergent body.
    }
}

#[test]
fn local_opaque_classes_are_scoped_to_each_source_function_instance() {
    let mut program = lower("type Flag<a> = Off | On\nfn independent(x) { let f = if true { Off } else { On }; match f { Off => x | On => x } }\nfn main() { if independent(true) { independent(42) } else { 0 } }");
    let symbols = program
        .adts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.symbolic.is_some())
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 2);
    let first = &program.adts[symbols[0]]
        .symbolic
        .as_ref()
        .unwrap()
        .arguments[0];
    let second = &program.adts[symbols[1]]
        .symbolic
        .as_ref()
        .unwrap()
        .arguments[0];
    let (LogicalType::Opaque(first), LogicalType::Opaque(second)) = (first, second) else {
        panic!()
    };
    assert_ne!(
        program.logical.binders[first.index].scope,
        program.logical.binders[second.index].scope
    );
    let own_scope = program.logical.binders[first.index].scope;
    let function = program
        .logical
        .function_scopes
        .iter()
        .position(|scope| *scope == own_scope)
        .unwrap();
    let ExprKind::Let { value, .. } = &mut program.functions[function].body.kind else {
        panic!()
    };
    let ExprKind::If(_, yes, _) = &mut value.kind else {
        panic!()
    };
    yes.ty = Type::Adt {
        id: symbols[1],
        owned: false,
    };
    let ExprKind::Constructor { adt, .. } = &mut yes.kind else {
        panic!()
    };
    *adt = symbols[1];
    assert!(verify(&program).unwrap_err().contains("scope"));
    reject(&program);
}

#[test]
fn borrowed_symbolic_slots_require_the_exact_nominal_instance() {
    let mut program = lower("type Flag<a> = Off | On\nfn inspect(read f) { match f { Off => 21 | On => 0 } }\nfn main() { let mut f = Off; let mut g = Off; inspect(f) + inspect(g) }");
    let ids = program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.params.len() == 1)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_ne!(
        program.functions[ids[0]].params[0].1,
        program.functions[ids[1]].params[0].1
    );
    let ExprKind::Let { body, .. } = &mut program.functions[program.entry].body.kind else {
        panic!()
    };
    let ExprKind::Let { body, .. } = &mut body.kind else {
        panic!()
    };
    let ExprKind::Binary(_, left, _) = &mut body.kind else {
        panic!()
    };
    let ExprKind::Call(callee, _) = &mut left.kind else {
        panic!()
    };
    *callee = if *callee == ids[0] { ids[1] } else { ids[0] };
    reject(&program);
}

#[test]
fn source_nested_logical_arguments_and_private_captures_are_both_verified() {
    let source = "type Flag<a> = Off | On\ntype Link<a,b> = { input: a; tag: Flag<a>; unused: Flag<b> }\nfn tag(x) { let r = record Link { input = x; tag = Off; unused = Off }; r.tag }\nfn main() { tag(Off) }";
    let program = lower(source);
    assert!(program.adts.iter().all(|adt| adt.name != "Link"));
    assert!(program
        .adts
        .iter()
        .any(|adt| adt.origin == AdtOrigin::LocalCapture));
    assert!(program
        .adts
        .iter()
        .filter_map(|adt| adt.symbolic.as_ref())
        .any(|instance| matches!(
            instance.arguments.first(),
            Some(LogicalType::Nominal { .. })
        )));
    verify(&program).unwrap();
    assert_eq!(evaluate(&program).unwrap().to_string(), "Off");
    emit_c(&program).unwrap();
}

#[test]
fn opaque_identities_cannot_be_spliced_between_compilations() {
    let original = flag();
    let foreign = flag();
    let first = symbolic(&original);
    let other = symbolic(&foreign);
    assert_ne!(original.adts[first].symbolic, foreign.adts[other].symbolic);
    assert_eq!(emit_c(&original).unwrap(), emit_c(&foreign).unwrap());
    verify(&original.clone()).unwrap();
    let mut forged = original.clone();
    forged.adts[first].symbolic = foreign.adts[other].symbolic.clone();
    reject(&forged);
    let mut forged = original.clone();
    forged.logical.domain = foreign.logical.domain.clone();
    reject(&forged);
    let parsed = parser::parse("type Flag<a> = Off | On\nfn main() { Off }").unwrap();
    let checked = check::analyze(&parsed).unwrap();
    let instances = instances::Instances::discover(&checked).unwrap();
    assert!(original
        .logical
        .validate_source(&checked, &instances)
        .is_err());
}

#[test]
fn product_proof_reconstructs_closed_descendants_not_just_their_names() {
    let original = lower("type Fixed = Fixed(i32)\ntype Middle = Middle(Fixed)\ntype Mark<a> = Mark(Middle)\nfn main() { Mark(Middle(Fixed(42))) }");
    for target in ["Fixed", "Middle"] {
        let mut forged = original.clone();
        let id = forged.adts.iter().position(|a| a.name == target).unwrap();
        forged.adts[id].variants[0].fields[0] = Type::Bool;
        assert!(verify(&forged).unwrap_err().contains("E-TYPED-LOGICAL"));
        reject(&forged);
    }
}

#[test]
fn product_kind_fields_variants_and_copy_leaves_are_canonical() {
    let original = lower(
        "type Stamp<a> = { n: i32; b: bool }\nfn main() { record Stamp { b = false; n = 42 } }",
    );
    let id = symbolic(&original);
    for mutation in 0..9 {
        let mut forged = original.clone();
        match mutation {
            0 => forged.adts[id].record_fields = None,
            1 => forged.adts[id].record_fields.as_mut().unwrap().swap(0, 1),
            2 => forged.adts[id].variants[0].fields.swap(0, 1),
            3 => {
                forged.adts[id].variants[0].fields.pop();
            }
            4 => forged.adts[id].variants[0].name = "NotStamp".into(),
            5 => forged.adts[id].variants[0].fields[0] = Type::OwnedI32,
            6 => forged.adts[id].variants[0].fields[0] = Type::Adt { id, owned: false },
            7 => forged.adts[id].symbolic = None,
            8 => forged.adts[id].origin = AdtOrigin::LocalCapture,
            _ => unreachable!(),
        }
        reject(&forged);
    }
}

#[test]
fn product_fields_preserve_nested_logical_and_closed_phantom_identity() {
    for source in [
        "type Flag<a> = Off | On\ntype Link<a> = Link(a, Flag<a>)\nfn main() { Link(Off, On) }",
        "type Flag<a> = Off | On\ntype Pair<a> = Pair(Flag<i32>, Flag<bool>)\nfn main() { Pair(Off, On) }",
    ] {
        let original = lower(source);
        let id = original.adts.iter().position(|a| a.symbolic.is_some() && !a.variants[0].fields.is_empty()).unwrap();
        assert_ne!(original.adts[id].variants[0].fields[0], original.adts[id].variants[0].fields[1]);
        let mut forged = original.clone();
        forged.adts[id].variants[0].fields.swap(0, 1);
        assert!(verify(&forged).unwrap_err().contains("E-TYPED-LOGICAL"));
        reject(&forged);
        let mut forged = original.clone();
        forged.adts[id].symbolic = None;
        reject(&forged);
    }
}

#[test]
fn optional_payload_proof_never_uses_the_selected_constructor() {
    let original = lower("type Flag<a> = Off | On\ntype Option<a> = None | Some(a)\nfn main() { if true { None } else { Some(Off) } }");
    let id = original
        .adts
        .iter()
        .position(|a| a.name == "Option")
        .unwrap();
    for mutation in 0..5 {
        let mut forged = original.clone();
        match mutation {
            0 => {
                forged.adts[id].variants.pop();
            }
            1 => forged.adts[id].variants[1].fields.clear(),
            2 => forged.adts[id].variants[1].fields[0] = Type::Unit,
            3 => {
                forged.adts[id].symbolic.as_mut().unwrap().arguments[0] =
                    LogicalType::Opaque(forged.logical.opaque_id(0).unwrap());
                forged.adts[id].variants[1].fields[0] = Type::Unit;
            }
            4 => forged.adts[id].variants.swap(0, 1),
            _ => unreachable!(),
        }
        assert!(verify(&forged).unwrap_err().contains("E-TYPED-LOGICAL"));
        reject(&forged);
    }
}

#[test]
fn physical_native_fields_keep_intrinsic_dependency_and_pointer_identity() {
    let original =
        lower("type Raw<a> = Raw(u32, const_ptr<void>)\nfn main() { Raw(u32(42), c_null()) }");
    let id = symbolic(&original);
    assert!(
        original.adts[id]
            .symbolic
            .as_ref()
            .unwrap()
            .dependencies
            .native
    );
    let mut forged = original.clone();
    forged.adts[id]
        .symbolic
        .as_mut()
        .unwrap()
        .dependencies
        .native = false;
    reject(&forged);
    for mutation in 0..3 {
        let mut forged = original.clone();
        let Type::RawPointer(pointer) = forged.adts[id].variants[0].fields[1] else {
            panic!()
        };
        match mutation {
            0 => forged.native.pointers[pointer.0 as usize].is_const = false,
            1 => forged.native.pointers[pointer.0 as usize].pointee = "Other".into(),
            2 => forged.adts[id].variants[0].fields[1] = Type::RawPointer(PointerTypeId(u32::MAX)),
            _ => unreachable!(),
        }
        reject(&forged);
    }
}

#[test]
fn product_body_binders_do_not_escape_through_physical_fields() {
    let mut program = lower("type Stamp<a> = { n: i32 }\nfn independent(x) { let r = if true { record Stamp { n = 1 } } else { record Stamp { n = 2 } }; x }\nfn main() { if independent(true) { independent(42) } else { 0 } }");
    let ids: Vec<_> = program
        .adts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.symbolic.is_some())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(ids.len(), 2);
    let scope = match &program.adts[ids[0]].symbolic.as_ref().unwrap().arguments[0] {
        LogicalType::Opaque(binder) => program.logical.binders[binder.index].scope,
        _ => panic!(),
    };
    let function = program
        .logical
        .function_scopes
        .iter()
        .position(|s| *s == scope)
        .unwrap();
    let ExprKind::Let { value, .. } = &mut program.functions[function].body.kind else {
        panic!()
    };
    let ExprKind::If(_, yes, _) = &mut value.kind else {
        panic!()
    };
    yes.ty = Type::Adt {
        id: ids[1],
        owned: false,
    };
    let ExprKind::Record { adt, .. } = &mut yes.kind else {
        panic!()
    };
    *adt = ids[1];
    assert!(verify(&program).unwrap_err().contains("scope"));
    reject(&program);
}

#[test]
fn consistent_product_schema_rewrite_still_fails_checked_source_correspondence() {
    let parsed = parser::parse("type Mark<a> = Mark(i32)\nfn main() { Mark(42) }").unwrap();
    let checked = check::analyze(&parsed).unwrap();
    let (mut program, instances) = lower_main_traced(&checked, None, None).unwrap();
    program
        .logical
        .validate_source(&checked, &instances)
        .unwrap();
    let id = symbolic(&program);
    let definition = program.adts[id].symbolic.as_ref().unwrap().definition;
    program.logical.declarations[definition].variants[0].fields[0].name = "bool".into();
    program.adts[id].variants[0].fields[0] = Type::Bool;
    let ExprKind::Constructor { fields, .. } = &mut program.functions[program.entry].body.kind
    else {
        panic!()
    };
    fields[0] = ExprNode {
        ty: Type::Bool,
        kind: ExprKind::Bool(true),
    };
    // This describes a different, internally valid finite Copy program.
    verify(&program).unwrap();
    assert_eq!(evaluate(&program).unwrap().to_string(), "Mark(true)");
    assert!(program
        .logical
        .validate_source(&checked, &instances)
        .unwrap_err()
        .contains("checked source"));
}
