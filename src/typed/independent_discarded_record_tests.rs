//! Independent discarded-record proof/consumer probes.
//! Private child of crate::typed.
use super::*;
const CORE: &str = "type O<a> = None|Some(a) type R<a> = {left:i32;hole:O<a>;right:i32} fn mark(inout n:i32,d:i32){n<-n*10+d;n} fn made(inout n:i32){record R{right=mark(inout n,1);hole=None;left=mark(inout n,2)}} fn main(){let mut n=0;let weak=made(inout n);n}";
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn reject(p: &TypedMain) {
    let error = verify(p).unwrap_err();
    assert_eq!(
        evaluate(p).unwrap_err(),
        error,
        "evaluate did not verify first"
    );
    assert_eq!(emit_c(p).unwrap_err(), error, "emit did not verify first");
}
fn maker(p: &TypedMain) -> usize {
    p.functions
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
fn body(p: &mut TypedMain, index: usize) -> &mut DiscardExpr {
    let Computation::Discarded(body) = &mut p.functions[index].body else {
        panic!("no discarded body")
    };
    body
}
fn fields(p: &mut TypedMain, index: usize) -> &mut Vec<(usize, DiscardExpr)> {
    let DiscardKind::Record { fields, .. } = &mut body(p, index).kind else {
        panic!("no record")
    };
    fields
}
#[test]
fn independent_k4r_written_order_and_schema_indices_are_distinct() {
    let mut p = lower(CORE);
    let made = maker(&p);
    assert_eq!(
        fields(&mut p, made)
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        [2, 1, 0]
    );
    assert!(
        p.adts.is_empty(),
        "discard must not allocate R or O layouts"
    );
    assert_eq!(evaluate(&p).unwrap().to_string(), "12");
    let c = emit_c(&p).unwrap();
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
    for function in &p.functions {
        assert!(function.params.iter().all(|(_, ty, _)| *ty == Type::Int));
    }
}
#[test]
fn independent_k4r_equal_typed_index_child_and_pair_swaps_reject() {
    let original = lower(CORE);
    let made = maker(&original);
    for action in 0..7 {
        let mut p = original.clone();
        let children = fields(&mut p, made);
        match action {
            0 => {
                children[0].0 = 0;
                children[2].0 = 2;
            } // Only equal-i32 indices.
            1 => {
                let child = children[0].1.clone();
                children[0].1 = children[2].1.clone();
                children[2].1 = child;
            }
            2 => children.swap(0, 2), // Full pairs preserve types and identities, change written order.
            3 => children[2].0 = children[0].0,
            4 => children[0].0 = usize::MAX,
            5 => {
                children.pop();
            }
            _ => children.push(children[0].clone()),
        }
        reject(&p);
    }
}
#[test]
fn independent_k4r_record_nominal_kind_arity_and_complete_child_type_reject() {
    let source = CORE.replace(
        "fn mark",
        "type Twin<a> = {left:i32;hole:O<a>;right:i32} fn mark",
    );
    let original = lower(&source);
    let made = maker(&original);
    for action in 0..8 {
        let mut p = original.clone();
        let b = body(&mut p, made);
        match action {
            0..=2 => {
                let DiscardKind::Record { definition, .. } = &mut b.kind else {
                    panic!()
                };
                *definition = match action {
                    0 => usize::MAX,
                    1 => 0,
                    _ => 2,
                };
            }
            3 => {
                let LogicalType::Nominal { arguments, .. } = &mut b.result else {
                    panic!()
                };
                arguments.clear();
            }
            4 => {
                let LogicalType::Nominal { definition, .. } = &mut b.result else {
                    panic!()
                };
                *definition = 2;
            }
            5 => {
                let DiscardKind::Record { fields, .. } = &mut b.kind else {
                    panic!()
                };
                let LogicalType::Nominal { arguments, .. } = &mut fields[1].1.result else {
                    panic!()
                };
                arguments[0] = LogicalType::Atom(crate::check::Type::Int);
            }
            6 => {
                b.kind = DiscardKind::Value(Box::new(ExprNode {
                    ty: Type::Unit,
                    kind: ExprKind::Unit,
                }))
            }
            _ => {
                let DiscardKind::Record { definition, fields } = &b.kind else {
                    panic!()
                };
                b.kind = DiscardKind::Constructor {
                    definition: *definition,
                    variant: 0,
                    fields: fields.iter().map(|(_, value)| value.clone()).collect(),
                };
            }
        }
        reject(&p);
    }
}
#[test]
fn independent_k4r_child_brand_occurrence_and_cross_demand_call_reject() {
    let original = lower(CORE);
    let made = maker(&original);
    let mut foreign = lower(CORE);
    let foreign_made = maker(&foreign);
    let mut p = original.clone();
    fields(&mut p, made)[1].1 = fields(&mut foreign, foreign_made)[1].1.clone();
    reject(&p);
    let mut p = original.clone();
    fields(&mut p, made)[0].1.occurrence = fields(&mut p, made)[2].1.occurrence;
    reject(&p);
    let source = CORE.replace(
        "let weak=made(inout n);n",
        "let observed=mark(inout n,2);let weak=made(inout n);n-observed*100",
    );
    let mut p = lower(&source);
    let made = maker(&p);
    let observed = p
        .functions
        .iter()
        .position(|f| f.params.len() == 2 && f.result == ResultRepr::Value(Type::Int))
        .unwrap();
    let DiscardKind::Call(callee, _) = &mut fields(&mut p, made)[0].1.kind else {
        panic!()
    };
    assert_ne!(*callee, observed);
    *callee = observed;
    reject(&p);
}
#[test]
fn independent_k4r_actual_record_depth_and_width_are_preflighted() {
    let original = lower(CORE);
    let made = maker(&original);
    let mut p = original.clone();
    let b = body(&mut p, made);
    let mut nested = b.clone();
    for _ in 0..130 {
        nested = DiscardExpr {
            result: b.result.clone(),
            occurrence: b.occurrence,
            kind: DiscardKind::Record {
                definition: 1,
                fields: vec![(0, nested)],
            },
        };
    }
    *b = nested;
    assert!(verify(&p).unwrap_err().contains("COMPLEXITY"));
    reject(&p);
    let mut p = original;
    let child = fields(&mut p, made)[0].1.clone();
    *fields(&mut p, made) = vec![(0, child); 50_001];
    assert!(verify(&p).unwrap_err().contains("COMPLEXITY"));
    reject(&p);
}
#[test]
fn independent_k4r_unexecuted_late_field_logical_depth_is_preflighted() {
    let original=lower("type O<a> = None|Some(a) type S<a,b> = {stop:a;hole:O<b>} fn spin(){spin()} fn made(){record S{stop=spin();hole=None}} fn main(){let weak=made();42}");
    let made = maker(&original);
    let mut p = original;
    let mut ty = LogicalType::Atom(crate::check::Type::Int);
    for _ in 0..129 {
        ty = LogicalType::Nominal {
            definition: usize::MAX,
            arguments: vec![ty],
        };
    }
    fields(&mut p, made)[1].1.result = ty;
    assert!(verify(&p).unwrap_err().contains("COMPLEXITY"));
    reject(&p);
}
#[test]
fn independent_k4r_traps_preserve_original_caller_slot_without_record_storage() {
    for (first, last, expected) in [("1/0", "mark(inout n,2)", 0), ("mark(inout n,1)", "1/0", 1)] {
        let source = CORE
            .replace("right=mark(inout n,1)", &format!("right={first}"))
            .replace("left=mark(inout n,2)", &format!("left={last}"));
        let p = lower(&source);
        let ExprKind::Let {
            local,
            value,
            body,
            mutable: true,
        } = &p.functions[p.entry].body.value_expr().unwrap().kind
        else {
            panic!()
        };
        let mut locals = CopyLocals::default();
        let mut state = EvalState::default();
        let mut owners = HashMap::new();
        let initial = evaluate_expr(value, &mut locals, &p, 1, &mut state, &mut owners).unwrap();
        locals.insert(*local, initial);
        assert_eq!(
            evaluate_expr(body, &mut locals, &p, 1, &mut state, &mut owners).unwrap_err(),
            "runtime trap"
        );
        assert_eq!(
            locals.read(local).unwrap(),
            crate::interpreter::Value::Int(expected)
        );
    }
}
#[test]
fn independent_k4r_dormant_internal_record_requires_all_metadata_and_scope() {
    let source="type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn main(){if false{let weak=if true{record R{hole=None}}else{record R{hole=None}};41}else{42}}";
    let original = lower(source);
    assert!(original.adts.is_empty());
    assert!(original
        .functions
        .iter()
        .all(|f| matches!(f.result, ResultRepr::Value(_))));
    for action in 0..6 {
        let mut p = original.clone();
        match action {
            0 => p.logical.control = None,
            1 => p.logical.domain = None,
            2 => p.logical.binders.clear(),
            3 => p.logical.declarations.clear(),
            4 => p.logical.function_scopes.clear(),
            _ => p.logical.scope_sources.clear(),
        };
        reject(&p);
    }
}
#[test]
fn independent_k4r_internal_record_cannot_use_another_functions_scope() {
    let source="type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn side(){let weak=if true{record R{hole=None}}else{record R{hole=None}};1} fn main(){let weak=if true{record R{hole=None}}else{record R{hole=None}};side()+41}";
    let mut p = lower(source);
    let main = p.entry;
    let side = (0..p.functions.len()).find(|id| *id != main).unwrap();
    fn seed(p: &mut TypedMain, id: usize) -> &mut DiscardExpr {
        let ExprKind::DiscardLet { value, .. } =
            &mut p.functions[id].body.value_expr_mut().unwrap().kind
        else {
            panic!()
        };
        value
    }
    let side_type = seed(&mut p, side).result.clone();
    let main_type = seed(&mut p, main).result.clone();
    assert_ne!(side_type, main_type);
    assert!(p
        .logical
        .verify(&p)
        .unwrap()
        .logical_in_scope(&side_type, main)
        .is_err());
    seed(&mut p, main).result = side_type;
    reject(&p);
}
#[test]
fn independent_k4r_two_generic_record_fields_do_not_share_a_void_identity() {
    let source="type O<a> = None|Some(a) type R<a,b> = {left:O<a>;right:O<b>} fn made(){record R{right=None;left=None}} fn main(){let weak=made();42}";
    let mut p = lower(source);
    let made = maker(&p);
    let children = fields(&mut p, made);
    assert_ne!(children[0].1.result, children[1].1.result);
    children[0].1.result = children[1].1.result.clone();
    reject(&p);
    let source="type O<a> = None|Some(a) type I<a> = {hole:O<a>} type R<a,b> = {left:I<a>;right:I<b>} fn made(){record R{right=record I{hole=None};left=record I{hole=None}}} fn main(){let weak=made();42}";
    let mut p = lower(source);
    let made = maker(&p);
    let children = fields(&mut p, made);
    assert_ne!(children[0].1.result, children[1].1.result);
    let sibling = children[1].1.result.clone();
    children[0].1.result = sibling;
    reject(&p);
}
#[test]
fn independent_k4r_empty_nested_record_has_real_normal_flow() {
    let source="type O<a> = None|Some(a) type Empty = {} type R<a> = {empty:Empty;hole:O<a>} fn made(){record R{hole=None;empty=record Empty{}}} fn main(){let weak=made();42}";
    let mut p = lower(source);
    let made = maker(&p);
    let children = fields(&mut p, made);
    assert!(matches!(&children[1].1.kind,DiscardKind::Record{fields,..}if fields.is_empty()));
    let flow = control::IrFacts::build(&p).unwrap();
    assert_eq!(flow.function_may_return(made), Some(true));
    assert_eq!(flow.function_may_return(p.entry), Some(true));
    assert_eq!(evaluate(&p).unwrap().to_string(), "42");
}
#[test]
fn independent_k4r_bare_generic_stop_preserves_caller_slot_and_late_field() {
    let p=lower("type O<a> = None|Some(a) type S<a,b> = {first:i32;hole:O<b>;stop:a;last:i32} fn mark(inout n:i32,d:i32){n<-n*10+d;n} fn spin(){spin()} fn made(inout n:i32){record S{first=mark(inout n,1);hole=None;stop=spin();last=mark(inout n,2)}} fn main(){let mut n=0;let weak=made(inout n);n}");
    let made = maker(&p);
    let Computation::Discarded(DiscardExpr {
        kind: DiscardKind::Record { fields, .. },
        ..
    }) = &p.functions[made].body
    else {
        panic!()
    };
    assert_eq!(fields.len(), 4);
    assert!(matches!(&fields[2].1.kind, DiscardKind::FromNoNormal(_)));
    assert!(matches!(&fields[3].1.kind, DiscardKind::Call(..)));
    let ExprKind::Let {
        local,
        value,
        body,
        mutable: true,
    } = &p.functions[p.entry].body.value_expr().unwrap().kind
    else {
        panic!()
    };
    let mut locals = CopyLocals::default();
    let mut state = EvalState::default();
    let mut owners = HashMap::new();
    let initial = evaluate_expr(value, &mut locals, &p, 1, &mut state, &mut owners).unwrap();
    locals.insert(*local, initial);
    assert_eq!(
        evaluate_expr(body, &mut locals, &p, 1, &mut state, &mut owners).unwrap_err(),
        "runtime trap"
    );
    assert_eq!(
        locals.read(local).unwrap(),
        crate::interpreter::Value::Int(1)
    );
}
