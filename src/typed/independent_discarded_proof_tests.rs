//! Independent discarded-result verification and retained-source regressions.
use super::*;
const CORE: &str = "type O<a> = None | Some(a) fn absent(){None} fn main(){let weak=absent();42}";
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn reject(program: &TypedMain) {
    let verify_error = verify(program).unwrap_err();
    assert_eq!(
        evaluate(program).unwrap_err(),
        verify_error,
        "evaluate must reject before running"
    );
    assert_eq!(
        emit_c(program).unwrap_err(),
        verify_error,
        "emit must reject before producing C"
    );
}
fn discard_function(program: &TypedMain) -> usize {
    program
        .functions
        .iter()
        .position(|f| matches!(f.body, Computation::Discarded(_)))
        .unwrap()
}
fn discard_body(program: &mut TypedMain, index: usize) -> &mut DiscardExpr {
    let Computation::Discarded(body) = &mut program.functions[index].body else {
        panic!("discard body absent")
    };
    body
}
fn seed(program: &mut TypedMain) -> (&mut NoStorageBinding, &mut DiscardExpr, &mut ExprNode) {
    let Computation::Value(body) = &mut program.functions[program.entry].body else {
        panic!()
    };
    let ExprKind::DiscardLet {
        binding,
        value,
        body,
    } = &mut body.kind
    else {
        panic!("seed was not an exact discard let")
    };
    (binding, value, body)
}
#[test]
fn independent_k4_only_discard_seed_still_requires_source_metadata() {
    let mut bad = lower(CORE);
    bad.logical.control = None;
    for function in &mut bad.functions {
        if matches!(function.result, ResultRepr::Discarded(_)) {
            function.result = ResultRepr::Value(Type::Int);
            function.body = Computation::Value(ExprNode {
                ty: Type::Int,
                kind: ExprKind::Int(0),
            });
        }
    }
    let error = verify(&bad).unwrap_err();
    assert!(error.contains("canonical source proof"), "{error}");
    reject(&bad);
}
#[test]
fn independent_k4_missing_logical_only_catalog_and_foreign_brand_reject() {
    let original = lower(CORE);
    assert!(
        original.adts.is_empty(),
        "logical-only fixture accidentally built a physical O layout"
    );
    for action in 0..6 {
        let mut bad = original.clone();
        match action {
            0 => bad.logical.declarations.clear(),
            1 => bad.logical.binders.clear(),
            2 => bad.logical.domain = None,
            3 => bad.logical.function_scopes.clear(),
            4 => bad.logical.scope_sources.clear(),
            _ => bad.logical.source_functions.clear(),
        }
        reject(&bad);
    }
    let mut foreign = lower(CORE);
    let foreign_index = discard_function(&foreign);
    let foreign_body = discard_body(&mut foreign, foreign_index).clone();
    let mut bad = original.clone();
    let index = discard_function(&bad);
    *discard_body(&mut bad, index) = foreign_body;
    reject(&bad);
    let mut bad = original;
    let foreign_binding = seed(&mut foreign).0.clone();
    *seed(&mut bad).0 = foreign_binding;
    reject(&bad);
}
#[test]
fn independent_k4_constructor_full_schema_and_occurrence_are_checked() {
    let original = lower(CORE);
    let index = discard_function(&original);
    for action in 0..5 {
        let mut bad = original.clone();
        let node = discard_body(&mut bad, index);
        match action {
            0 => node.occurrence = usize::MAX,
            1 => {
                let DiscardKind::Constructor { definition, .. } = &mut node.kind else {
                    panic!()
                };
                *definition = usize::MAX;
            }
            2 => {
                let DiscardKind::Constructor { variant, .. } = &mut node.kind else {
                    panic!()
                };
                *variant = 1;
            }
            3 => {
                let copied = node.clone();
                let DiscardKind::Constructor { fields, .. } = &mut node.kind else {
                    panic!()
                };
                fields.push(copied);
            }
            _ => node.result = LogicalType::Atom(crate::check::Type::Var(0)),
        }
        reject(&bad);
    }
    let mut bad = original;
    bad.logical.declarations[0].variants[1].fields[0].name = "SystemBoxI32".into();
    reject(&bad);
}
#[test]
fn independent_k4_void_is_neither_value_nor_no_normal_or_observed_entry() {
    let original = lower(CORE);
    let index = discard_function(&original);
    for concrete in [Type::Int, Type::Unit] {
        let mut bad = original.clone();
        bad.functions[index].result = ResultRepr::Value(concrete);
        bad.functions[index].body = Computation::Value(ExprNode {
            ty: concrete,
            kind: if concrete == Type::Int {
                ExprKind::Int(0)
            } else {
                ExprKind::Unit
            },
        });
        reject(&bad);
    }
    let mut bad = original.clone();
    bad.functions[bad.entry].result = bad.functions[index].result.clone();
    bad.functions[bad.entry].body = bad.functions[index].body.clone();
    reject(&bad);
    let mut bad = original;
    let identity = bad.logical.opaque_id(0).unwrap();
    bad.functions[index].result = ResultRepr::NoNormal(identity.clone());
    bad.functions[index].body = Computation::NoNormal(NoNormalExpr {
        result: identity,
        occurrence: 0,
        kind: NoNormalKind::Call(index, vec![]),
    });
    reject(&bad);
}
#[test]
fn independent_k4_no_storage_identity_and_continuation_are_not_optional() {
    let original = lower(CORE);
    let mut bad = original.clone();
    seed(&mut bad).0.occurrence = usize::MAX;
    reject(&bad);
    let mut bad = original.clone();
    seed(&mut bad).2.kind = ExprKind::Int(41);
    reject(&bad);
    let mut bad = original;
    seed(&mut bad).1.occurrence = usize::MAX;
    reject(&bad);
}
#[test]
fn independent_k4_independent_weak_classes_do_not_merge_under_void_abi() {
    let original = lower(
        "type O<a> = None|Some(a) fn absent(){None} fn main(){let a=absent();let b=absent();42}",
    );
    let mut bad = original;
    let (_, first, continuation) = seed(&mut bad);
    let DiscardKind::Call(first_target, _) = &first.kind else {
        panic!()
    };
    let first_target = *first_target;
    let ExprKind::DiscardLet { value: second, .. } = &mut continuation.kind else {
        panic!()
    };
    assert_ne!(
        first.result, second.result,
        "two expansive calls must keep distinct source weak classes"
    );
    let DiscardKind::Call(second_target, _) = &mut second.kind else {
        panic!()
    };
    assert_ne!(
        first_target, *second_target,
        "void ABI is not instance identity"
    );
    *second_target = first_target;
    reject(&bad);
}
#[test]
fn independent_k4_same_signature_observe_discard_target_swap_is_rejected() {
    let mut bad=lower("type O<a> = None|Some(a) type W<a> = W(i32,O<a>) fn mark(inout n:i32,d:i32){n<-n+d;n} fn made(inout n:i32){W(mark(inout n,2),None)} fn main(){let mut n=0;let first=mark(inout n,1);let weak=made(inout n);first*30+n}");
    let observe = bad
        .functions
        .iter()
        .position(|f| f.params.len() == 2 && f.result == Type::Int)
        .unwrap();
    let discarded = bad
        .functions
        .iter()
        .position(|f| f.params.len() == 2 && matches!(f.result, ResultRepr::Discarded(_)))
        .unwrap();
    assert_ne!(observe, discarded);
    let maker = bad
        .functions
        .iter()
        .position(|f| {
            matches!(
                &f.body,
                Computation::Discarded(DiscardExpr {
                    kind: DiscardKind::Constructor { .. },
                    ..
                })
            )
        })
        .unwrap();
    let DiscardKind::Constructor { fields, .. } = &mut discard_body(&mut bad, maker).kind else {
        panic!()
    };
    let DiscardKind::Call(callee, _) = &mut fields[0].kind else {
        panic!()
    };
    assert_eq!(*callee, discarded);
    *callee = observe;
    reject(&bad);
}
#[test]
fn independent_k4_constructor_operand_order_and_selected_schema_must_agree() {
    let original=lower("type O<a> = None|Some(a) type W<a> = W(i32,O<a>,i32) fn mark(inout n:i32,d:i32){n<-n*10+d;n} fn made(inout n:i32){W(mark(inout n,1),None,mark(inout n,2))} fn main(){let mut n=0;let weak=made(inout n);n+30}");
    let maker=original.functions.iter().position(|f|matches!(&f.body,Computation::Discarded(DiscardExpr{kind:DiscardKind::Constructor{fields,..},..})if fields.len()==3)).unwrap();
    let mut bad = original.clone();
    let DiscardKind::Constructor { fields, .. } = &mut discard_body(&mut bad, maker).kind else {
        panic!()
    };
    fields.swap(0, 2);
    reject(&bad);
    let mut bad = original;
    let DiscardKind::Constructor { fields, .. } = &mut discard_body(&mut bad, maker).kind else {
        panic!()
    };
    fields.pop();
    reject(&bad);
}
#[test]
fn independent_k4_nullary_discard_has_true_ir_flow_even_when_tag_is_void() {
    let program = lower(CORE);
    let absent = discard_function(&program);
    let facts = control::IrFacts::build(&program).unwrap();
    assert_eq!(facts.function_may_return(absent), Some(true));
    assert_eq!(facts.function_may_return(program.entry), Some(true));
}
#[test]
fn independent_k4_actual_logical_depth_precedes_recursive_equality_and_hashing() {
    // Root result depth is zero. Unknown definitions at the boundary are invalid,
    // but exceeding the structural boundary must be a complexity error first.
    for (depth, complexity) in [(128, false), (129, true)] {
        let mut bad = lower(CORE);
        let index = discard_function(&bad);
        let mut ty = LogicalType::Atom(crate::check::Type::Int);
        for _ in 0..depth {
            ty = LogicalType::Nominal {
                definition: usize::MAX,
                arguments: vec![ty],
            };
        }
        bad.functions[index].result = ResultRepr::Discarded(ty);
        let error = verify(&bad).unwrap_err();
        assert_eq!(
            error.contains("COMPLEXITY"),
            complexity,
            "depth {depth}: {error}"
        );
        reject(&bad);
    }
}
