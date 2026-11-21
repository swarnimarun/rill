//! Author structural, schema, execution and budget checks for discarded records.
use super::*;
use crate::typed::{self, CopyLocals, EvalState};

const SIMPLE: &str = "type O<a> = None|Some(a) type R<a> = {left:i32;hole:O<a>;right:i32} fn made(){record R{right=1;hole=None;left=2}} fn main(){let weak=made();42}";
const ORDER: &str = "type O<a> = None|Some(a) type R<a> = {left:i32;hole:O<a>;right:i32} fn mark(inout n,d){n<-n*10+d;n} fn made(inout n){record R{right=mark(inout n,1);hole=None;left=mark(inout n,2)}} fn main(){let mut n=0;let weak=made(inout n);n}";
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    typed::lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn rejects(ir: &TypedMain) {
    assert!(typed::verify(ir).is_err());
    assert!(typed::evaluate(ir).is_err());
    assert!(typed::emit_c(ir).is_err());
}
fn body(ir: &mut TypedMain) -> &mut DiscardExpr {
    ir.functions
        .iter_mut()
        .find_map(|function| match &mut function.body {
            Computation::Discarded(value) if matches!(value.kind, DiscardKind::Record { .. }) => {
                Some(value)
            }
            _ => None,
        })
        .unwrap()
}
fn fields(ir: &mut TypedMain) -> &mut Vec<(usize, DiscardExpr)> {
    let DiscardKind::Record { fields, .. } = &mut body(ir).kind else {
        unreachable!()
    };
    fields
}
#[test]
fn discarded_record_order_has_schema_indices_and_no_layouts() {
    let mut ir = lower(ORDER);
    assert!(ir.adts.is_empty());
    assert_eq!(
        fields(&mut ir)
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![2, 1, 0]
    );
    assert_eq!(typed::evaluate(&ir).unwrap().to_string(), "12");
    let frozen = ir.logical.control.as_ref().unwrap();
    for function in &ir.functions {
        let Computation::Discarded(value) = &function.body else {
            continue;
        };
        let DiscardKind::Record { fields, .. } = &value.kind else {
            continue;
        };
        for (ordinal, (_, field)) in fields.iter().enumerate() {
            assert_eq!(
                frozen.witness.nodes[field.occurrence].parent,
                Some((NodeId(value.occurrence), Role::RecordField(ordinal)))
            );
        }
    }
}
#[test]
fn discarded_record_mapping_shape_and_identity_mutations_reject() {
    let original = lower(&SIMPLE.replace(
        "fn made",
        "type Other<a> = {left:i32;hole:O<a>;right:i32} fn made",
    ));
    for mode in 0..10 {
        let mut ir = original.clone();
        match mode {
            0 => {
                let DiscardKind::Record { definition, .. } = &mut body(&mut ir).kind else {
                    unreachable!()
                };
                *definition = usize::MAX;
            }
            1 => {
                let DiscardKind::Record { definition, .. } = &mut body(&mut ir).kind else {
                    unreachable!()
                };
                *definition = 2;
            }
            2 => {
                let DiscardKind::Record { definition, .. } = &mut body(&mut ir).kind else {
                    unreachable!()
                };
                *definition = 0;
            }
            3 => fields(&mut ir)[0].0 = 0,
            4 => fields(&mut ir)[0].0 = 3,
            5 => {
                fields(&mut ir).pop();
            }
            6 => {
                let extra = fields(&mut ir)[0].clone();
                fields(&mut ir).push(extra);
            }
            7 => {
                let f = fields(&mut ir);
                let (a, b) = (f[0].0, f[2].0);
                f[0].0 = b;
                f[2].0 = a;
            }
            8 => {
                let f = fields(&mut ir);
                let (a, b) = (f[0].1.clone(), f[2].1.clone());
                f[0].1 = b;
                f[2].1 = a;
            }
            _ => fields(&mut ir).swap(0, 2),
        }
        rejects(&ir);
    }
}
#[test]
fn discarded_record_child_type_brand_occurrence_and_adapter_mutations_reject() {
    let original = lower(SIMPLE);
    let mut other = lower(SIMPLE);
    for mode in 0..7 {
        let mut ir = original.clone();
        match mode {
            0 => fields(&mut ir)[1].1.result = fields(&mut other)[1].1.result.clone(),
            1 => fields(&mut ir)[0].1.occurrence += 1,
            2 => {
                let LogicalType::Nominal { arguments, .. } = &mut fields(&mut ir)[1].1.result
                else {
                    unreachable!()
                };
                arguments[0] = LogicalType::Atom(Type::Int);
            }
            3 => {
                let LogicalType::Nominal { arguments, .. } = &mut body(&mut ir).result else {
                    unreachable!()
                };
                arguments.clear();
            }
            4 => body(&mut ir).result = LogicalType::Atom(Type::Int),
            5 => {
                body(&mut ir).kind = DiscardKind::Value(Box::new(ExprNode {
                    ty: RuntimeType::Unit,
                    kind: ExprKind::Unit,
                }));
            }
            _ => {
                let copied = fields(&mut ir).iter().map(|(_, f)| f.clone()).collect();
                body(&mut ir).kind = DiscardKind::Constructor {
                    definition: 1,
                    variant: 0,
                    fields: copied,
                };
            }
        }
        rejects(&ir);
    }
}
#[test]
fn discarded_record_source_schema_and_written_roles_are_independent() {
    for mode in 0..7 {
        let mut ir = lower(SIMPLE);
        let occurrence = body(&mut ir).occurrence;
        let child = fields(&mut ir)[0].1.occurrence;
        let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
        match mode {
            0 => {
                let SourceKind::Record(name, _) = &mut proof.witness.nodes[occurrence].kind else {
                    unreachable!()
                };
                *name = "Other".into();
            }
            1 => {
                let SourceKind::Record(_, fields) = &mut proof.witness.nodes[occurrence].kind
                else {
                    unreachable!()
                };
                fields[0].0 = "left".into();
            }
            2 => {
                let SourceKind::Record(_, fields) = &mut proof.witness.nodes[occurrence].kind
                else {
                    unreachable!()
                };
                fields[0].0 = "missing".into();
            }
            3 => {
                proof.witness.nodes[child].parent = Some((NodeId(occurrence), Role::RecordField(2)))
            }
            4 => {
                proof.witness.environment.types[1]
                    .record_fields
                    .as_mut()
                    .unwrap()
                    .pop();
                ir.logical.declarations[1]
                    .record_fields
                    .as_mut()
                    .unwrap()
                    .pop();
            }
            5 => {
                let variant = proof.witness.environment.types[1].variants[0].clone();
                proof.witness.environment.types[1]
                    .variants
                    .push(variant.clone());
                ir.logical.declarations[1].variants.push(variant);
            }
            _ => {
                proof.witness.environment.types[1].record_fields = None;
                ir.logical.declarations[1].record_fields = None;
            }
        }
        rejects(&ir);
    }
}
#[test]
fn discarded_record_inactive_schema_and_source_only_suffix_purity_reject() {
    for source in [SIMPLE, "type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),record R{hole=None})}"] {
        for owner_or_native in ["SystemBoxI32","u32"] {
            let mut ir=lower(source);
            let proof=std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
            proof.witness.environment.types[0].variants[1].fields[0].name=owner_or_native.into();
            ir.logical.declarations[0].variants[1].fields[0].name=owner_or_native.into();
            rejects(&ir);
        }
    }
}
#[test]
fn discarded_record_consistent_edit_still_fails_original_checked_source() {
    let ast = crate::parser::parse(SIMPLE).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    let mut ir = typed::lower_main(&checked).unwrap();
    let child = &mut fields(&mut ir)[0].1;
    let occurrence = child.occurrence;
    let DiscardKind::Value(value) = &mut child.kind else {
        unreachable!()
    };
    value.kind = ExprKind::Int(3);
    let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
    proof.witness.nodes[occurrence].kind = SourceKind::Int(3);
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
fn discarded_record_depth_and_width_preflight_precede_consumers() {
    let original = lower(SIMPLE);
    let mut deep = original.clone();
    let mut node = fields(&mut deep)[0].1.clone();
    for _ in 0..132 {
        node = DiscardExpr {
            result: node.result.clone(),
            occurrence: node.occurrence,
            kind: DiscardKind::Record {
                definition: 1,
                fields: vec![(0, node)],
            },
        };
    }
    fields(&mut deep)[0].1 = node;
    assert!(typed::verify(&deep).unwrap_err().contains("COMPLEXITY"));
    rejects(&deep);
    let mut wide = original;
    let child = fields(&mut wide)[0].clone();
    *fields(&mut wide) = vec![child; 60_000];
    assert!(typed::verify(&wide).unwrap_err().contains("COMPLEXITY"));
    rejects(&wide);
}
#[test]
fn dead_discarded_record_still_requires_proof_with_value_only_signatures() {
    let mut ir=lower("type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn main(){if false{let weak=if true{record R{hole=None}}else{record R{hole=None}};0}else{42}}");
    assert!(ir
        .functions
        .iter()
        .all(|f| matches!(f.result, ResultRepr::Value(_))));
    ir.logical = Default::default();
    assert!(crate::typed::control::has_extension(&ir));
    rejects(&ir);
}
#[test]
fn discarded_record_flow_is_normal_including_empty_and_recursive_cases() {
    for source in [SIMPLE,"type O<a> = None|Some(a) type E = {} type R<a> = {hole:O<a>;empty:E} fn made(){record R{empty=record E{};hole=None}} fn main(){let weak=made();42}","type O<a> = None|Some(a) type R<a> = {hole:O<a>} fn rec(n){if n==0{record R{hole=None}}else{rec(n-1)}} fn main(){let weak=rec(3);42}"] {
        let ir=lower(source);
        let facts=IrFacts::build(&ir).unwrap();
        for index in 0..ir.functions.len(){assert_eq!(facts.function_may_return(index),Some(true));}
    }
}
#[test]
fn discarded_record_traps_preserve_actual_mutable_caller_slot() {
    for (first, last, expected) in [
        ("1/0", "mark(inout n,2)", 0),
        ("mark(inout n,1)", "1/0", 1),
        ("mark(inout n,1)", "ignore(spin())", 1),
    ] {
        let source=format!("type O<a> = None|Some(a) type R<a> = {{left:i32;hole:O<a>;right:i32}} fn spin(){{spin()}} fn ignore(x){{42}} fn mark(inout n,d){{n<-n*10+d;n}} fn made(inout n){{record R{{right={first};hole=None;left={last}}}}} fn main(){{let mut n=0;let weak=made(inout n);n}}");
        let ir = lower(&source);
        let ExprKind::Let {
            local,
            value,
            body,
            mutable: true,
        } = &ir.functions[ir.entry].body.value_expr().unwrap().kind
        else {
            panic!()
        };
        let mut locals = CopyLocals::default();
        let mut state = EvalState::default();
        let mut owners = HashMap::new();
        let initial =
            typed::evaluate_expr(value, &mut locals, &ir, 1, &mut state, &mut owners).unwrap();
        locals.insert(*local, initial);
        assert_eq!(
            typed::evaluate_expr(body, &mut locals, &ir, 1, &mut state, &mut owners).unwrap_err(),
            "runtime trap"
        );
        assert_eq!(
            locals.read(local).unwrap(),
            crate::interpreter::Value::Int(expected)
        );
    }
}

fn record_rule_cost(ir: &TypedMain, budget: &mut Budget, repeats: usize) -> Result<(), String> {
    let frozen = ir.logical.control.as_ref().unwrap();
    let (slot, value) = ir
        .functions
        .iter()
        .enumerate()
        .find_map(|(slot, f)| match &f.body {
            Computation::Discarded(v) if matches!(v.kind, DiscardKind::Record { .. }) => {
                Some((slot, v))
            }
            _ => None,
        })
        .unwrap();
    let DiscardKind::Record { definition, fields } = &value.kind else {
        unreachable!()
    };
    let result = frozen
        .logical_source(&value.result, &mut Budget::default(), 0)
        .unwrap();
    let facts = IrFacts::build(ir).unwrap();
    let returns = frozen.source_returns(&mut Budget::default()).unwrap();
    let owners = frozen.witness_owners(&mut Budget::default()).unwrap();
    let mut edges = vec![];
    for _ in 0..repeats {
        let mut relation = Relation {
            frozen,
            program: ir,
            instance: &frozen.instances[slot],
            budget,
            locals: HashMap::new(),
            local_ids: HashSet::new(),
            pending: vec![],
            seen: HashSet::new(),
            pattern_owners: &owners,
            source_returns: &returns,
            ir_facts: &facts,
            edges: &mut edges,
        };
        relation.discard_record(NodeId(value.occurrence), &result, *definition, fields)?;
        assert_eq!(relation.pending.len(), fields.len());
    }
    Ok(())
}
#[test]
fn discarded_record_mapping_substitution_and_queue_have_exact_cumulative_budget() {
    for source in [SIMPLE,"type O<a> = None|Some(a) type I<a> = {hole:O<a>} type R<a> = {left:i32;hole:I<O<a>>;right:i32} fn made(){record R{right=1;hole=record I{hole=Some(None)};left=2}} fn main(){let weak=made();42}"] {
        let ir=lower(source);
        let mut used=Budget::default();record_rule_cost(&ir,&mut used,1).unwrap();
        let cost=used.used;assert!(cost>20 && cost<MAX_WORK/3);
        let mut exact=Budget{used:MAX_WORK-cost};record_rule_cost(&ir,&mut exact,1).unwrap();assert_eq!(exact.used,MAX_WORK);
        let mut short=Budget{used:MAX_WORK-cost+1};assert_eq!(record_rule_cost(&ir,&mut short,1).unwrap_err(),LIMIT);
        let mut repeated=Budget::default();record_rule_cost(&ir,&mut repeated,3).unwrap();assert_eq!(repeated.used,cost*3);
        let mut repeat_short=Budget{used:MAX_WORK-cost*3+1};assert_eq!(record_rule_cost(&ir,&mut repeat_short,3).unwrap_err(),LIMIT);
    }
    // Reverse written order makes every independently resolved field scan a
    // different prefix of the full schema, even when all payload types agree.
    let mut declaration = String::new();
    let mut construction = String::new();
    for n in 0..24 {
        declaration.push_str(&format!("f{n}:i32;"));
    }
    declaration.push_str("hole:O<a>");
    for n in (0..24).rev() {
        construction.push_str(&format!("f{n}={n};"));
    }
    construction.push_str("hole=None");
    let source=format!("type O<a> = None|Some(a) type R<a> = {{{declaration}}} fn made(){{record R{{{construction}}}}} fn main(){{let weak=made();42}}");
    let ir = lower(&source);
    let mut used = Budget::default();
    record_rule_cost(&ir, &mut used, 1).unwrap();
    assert!(used.used > 1000);
    let mut short = Budget {
        used: MAX_WORK - used.used + 1,
    };
    assert_eq!(record_rule_cost(&ir, &mut short, 1).unwrap_err(), LIMIT);
}
#[test]
fn discarded_record_calls_keep_exact_observe_and_discard_instances() {
    let source = ORDER.replace(
        "let weak=made(inout n);n",
        "let first=mark(inout n,3);let weak=made(inout n);first+n",
    );
    let original = lower(&source);
    let observed = original
        .functions
        .iter()
        .position(|f| {
            f.params.len() == 2 && matches!(f.result, ResultRepr::Value(RuntimeType::Int))
        })
        .unwrap();
    let discarded = original
        .functions
        .iter()
        .position(|f| {
            f.params.len() == 2
                && matches!(
                    f.result,
                    ResultRepr::Discarded(LogicalType::Atom(Type::Int))
                )
        })
        .unwrap();
    assert_ne!(observed, discarded);
    let mut changed = original.clone();
    let DiscardKind::Call(callee, _) = &mut fields(&mut changed)[0].1.kind else {
        panic!()
    };
    *callee = observed;
    rejects(&changed);
    let mut changed = original;
    changed.functions.swap(observed, discarded);
    rejects(&changed);
}
#[test]
fn discarded_record_hidden_use_and_unused_identity_are_not_trusted() {
    for mode in 0..3 {
        let mut ir = lower(SIMPLE);
        if mode == 2 {
            let ExprKind::DiscardLet { binding, .. } =
                &mut ir.functions[ir.entry].body.value_expr_mut().unwrap().kind
            else {
                panic!()
            };
            binding.occurrence += 1;
        } else {
            let proof = std::sync::Arc::get_mut(ir.logical.control.as_mut().unwrap()).unwrap();
            let binding = proof
                .witness
                .bindings
                .iter()
                .position(|b| b.name == "weak")
                .unwrap();
            assert_eq!(proof.witness.bindings[binding].uses, 0);
            if mode == 0 {
                let node = proof
                    .witness
                    .nodes
                    .iter_mut()
                    .find(|n| matches!(n.kind, SourceKind::Int(42)))
                    .unwrap();
                node.kind = SourceKind::Var(Reference::Local(BindingId(binding)));
            } else {
                proof.witness.bindings[binding].mutable = true;
            }
        }
        rejects(&ir);
    }
}
#[test]
fn discarded_record_bare_nonreturning_field_retains_false_call_dependency() {
    let mut ir=lower("type O<a> = None|Some(a) type S<a,b> = {first:i32;hole:O<b>;stop:a;last:i32} fn spin(){spin()} fn made(){record S{first=1;hole=None;stop=spin();last=2}} fn main(){let weak=made();42}");
    let facts = IrFacts::build(&ir).unwrap();
    assert_eq!(facts.function_may_return(ir.entry), Some(false));
    assert_eq!(typed::evaluate(&ir).unwrap_err(), "runtime trap");
    let record = body(&mut ir);
    let DiscardKind::Record { fields, .. } = &mut record.kind else {
        panic!()
    };
    assert_eq!(fields.len(), 4);
    assert!(matches!(fields[2].1.kind, DiscardKind::FromNoNormal(_)));
    fields[2].1.kind = DiscardKind::Record {
        definition: 1,
        fields: vec![],
    };
    let facts = IrFacts::build(&ir).unwrap();
    assert_eq!(facts.function_may_return(ir.entry), Some(true));
    rejects(&ir);
}
