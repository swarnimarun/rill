use super::trace::RawOrigin;
use super::*;
use crate::typed::LocalId;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn source(text: &str) -> SourceProgram {
    let path = std::env::temp_dir().join(format!(
        "rill-body-private-{}-{}.rill",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, text).unwrap();
    let result = crate::modules::compile::load_program_with_sources(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    result
}
fn corrupted(text: &str, change: impl FnOnce(&mut Trace<'_>)) {
    let sources = source(text);
    let checked = check::analyze(sources.program()).unwrap();
    let budget = BodyBudget::default();
    let mut requests =
        instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    let (typed, instances) =
        crate::typed::lower_main_traced(&checked, Some(&mut requests), Some(&mut trace)).unwrap();
    trace.check().unwrap();
    let data =
        super::super::finalize_function_data(&sources, &checked, &typed, &instances, &mut requests)
            .unwrap();
    change(&mut trace);
    let error = match finalize(&sources, &checked, &typed, &instances, &data, &mut trace) {
        Err(error) => error,
        Ok(_) => panic!("corrupt metadata accepted"),
    };
    assert!(error.starts_with("E-TYPED-SOURCE-MAP"), "{error}");
    assert!(crate::typed::lower_main(&checked).is_ok());
}
const EQUAL: &str = "fn main() { 1+1 }";
const CAPTURE:&str="type M<a> = M(i32)\nfn ints(x:M<i32>) { 40 }\nfn bools(x:M<bool>) { 2 }\nfn main() { let x=M(7); ints(x)+bools(x) }";
#[test]
fn output_fragments_must_be_complete_unique_and_reachable() {
    corrupted(EQUAL, |t| {
        t.fragments.pop();
    });
    corrupted(EQUAL, |t| {
        t.fragments.push(t.fragments[0].clone());
    });
    corrupted(EQUAL, |t| {
        let parent = t
            .fragments
            .iter_mut()
            .find(|f| {
                f.children
                    .iter()
                    .any(|(r, _)| *r == TypedChildRole::BinaryLeft)
            })
            .unwrap();
        parent.children[1].1 = parent.children[0].1;
    });
    corrupted(EQUAL, |t| {
        let parent = t
            .fragments
            .iter_mut()
            .find(|f| {
                f.children
                    .iter()
                    .any(|(r, _)| *r == TypedChildRole::BinaryLeft)
            })
            .unwrap();
        parent.children[0].0 = TypedChildRole::LetValue;
    });
}
#[test]
fn equal_sibling_source_swaps_and_foreign_instances_fail() {
    corrupted(EQUAL, |t| {
        let parent = t
            .fragments
            .iter_mut()
            .find(|f| {
                f.children
                    .iter()
                    .any(|(r, _)| *r == TypedChildRole::BinaryLeft)
            })
            .unwrap();
        parent.children.swap(0, 1);
    });
    corrupted(EQUAL, |t| {
        let ids: Vec<_> = t
            .fragments
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                matches!(
                    f.fingerprint,
                    structure::Fingerprint::Expression(_, structure::ExpressionFingerprint::Int(1))
                )
                .then_some(i)
            })
            .collect();
        let origin = t.fragments[ids[0]].origin.clone();
        t.fragments[ids[0]].origin = t.fragments[ids[1]].origin.clone();
        t.fragments[ids[1]].origin = origin;
    });
    corrupted(EQUAL, |t| {
        t.fragments[0].instance = usize::MAX;
    });
    corrupted(EQUAL, |t| {
        if let RawOrigin::Source { source, .. } = &mut t.fragments[0].origin {
            *source = usize::MAX;
        }
    });
}
#[test]
fn local_roots_record_indices_patterns_and_callee_identity_are_checked() {
    corrupted("fn main() {let x=1;let y=1;x+y}", |t| {
        let f = t
            .fragments
            .iter_mut()
            .find(|f| {
                matches!(
                    f.fingerprint,
                    structure::Fingerprint::Expression(
                        _,
                        structure::ExpressionFingerprint::Read(_)
                    )
                )
            })
            .unwrap();
        if let structure::Fingerprint::Expression(_, structure::ExpressionFingerprint::Read(id)) =
            &mut f.fingerprint
        {
            *id = LocalId(999);
        }
    });
    corrupted(
        "type P={x:i32;y:i32}\nfn main(){let p=record P{y=1;x=1};p.x}",
        |t| {
            let f = t
                .fragments
                .iter_mut()
                .find(|f| matches!(f.fingerprint, structure::Fingerprint::RecordField { .. }))
                .unwrap();
            if let structure::Fingerprint::RecordField { declaration, .. } = &mut f.fingerprint {
                *declaration = 99;
            }
        },
    );
    corrupted(
        "type O<a> =None|Some(a)\nfn main(){match Some(42){Some(x)=>x|None=>0}}",
        |t| {
            let f = t
                .fragments
                .iter_mut()
                .find(|f| {
                    matches!(
                        f.fingerprint,
                        structure::Fingerprint::Pattern(
                            structure::PatternFingerprint::Constructor { .. }
                        )
                    )
                })
                .unwrap();
            if let structure::Fingerprint::Pattern(structure::PatternFingerprint::Constructor {
                variant,
                ..
            }) = &mut f.fingerprint
            {
                *variant = 99;
            }
        },
    );
    corrupted("fn f(x){x}\nfn main(){f(42)}", |t| {
        let f = t
            .fragments
            .iter_mut()
            .find(|f| {
                matches!(
                    f.fingerprint,
                    structure::Fingerprint::Expression(
                        _,
                        structure::ExpressionFingerprint::Call { .. }
                    )
                )
            })
            .unwrap();
        if let structure::Fingerprint::Expression(
            _,
            structure::ExpressionFingerprint::Call { function, .. },
        ) = &mut f.fingerprint
        {
            *function = 99;
        }
    });
}
#[test]
fn inferred_results_never_claim_a_written_source_type() {
    corrupted(EQUAL, |t| {
        let source = t.source_index.root(AstDeclaration::Function(0)).unwrap();
        let f = t
            .fragments
            .iter_mut()
            .find(|f| matches!(f.fingerprint, structure::Fingerprint::Result(_)))
            .unwrap();
        f.origin = RawOrigin::Source {
            source,
            rule: DirectRule::Expression,
        };
    });
}
#[test]
fn erased_shape_alias_and_capacity_dispositions_are_mandatory() {
    for text in [
        "type O<a> =None|Some(a)\nfn main(){let x=None;42}",
        "type O<a> =None|Some(a)\nfn main(){let x=None;let y=x;42}",
        "fn main(){bytes_len(bytes_new(4))}",
    ] {
        corrupted(text, |t| {
            let index = t
                .dispositions
                .iter()
                .position(|d| {
                    matches!(
                        d.disposition,
                        RawDisposition::StaticShapeOnly { .. }
                            | RawDisposition::AliasOfShape { .. }
                            | RawDisposition::EmbeddedOperand(_)
                    )
                })
                .unwrap();
            t.dispositions.remove(index);
        });
    }
}
#[test]
fn capture_root_field_shape_use_and_generated_relations_are_checked() {
    corrupted(CAPTURE, |t| {
        t.captures[0].environment.as_mut().unwrap().root = LocalId(999);
    });
    corrupted(CAPTURE, |t| {
        t.captures[0].environment.as_mut().unwrap().field = 999;
    });
    corrupted(CAPTURE, |t| {
        t.uses[0].selected = Some(usize::MAX);
    });
    corrupted(CAPTURE, |t| {
        let f = t
            .fragments
            .iter_mut()
            .find(|f| {
                matches!(
                    f.origin,
                    RawOrigin::Generated {
                        reason: GenerationReason::CaptureProjection,
                        ..
                    }
                )
            })
            .unwrap();
        if let RawOrigin::Generated { relations, .. } = &mut f.origin {
            relations.clear();
        }
    });
}
#[test]
fn body_budget_limits_are_exact_and_fail_before_growth() {
    for dimension in 0..3 {
        let mut budget = BodyBudget::with_limits(2, 2, 2);
        let charge = |b: &mut BodyBudget, n| match dimension {
            0 => b.records(n),
            1 => b.edges(n),
            _ => b.work(n),
        };
        assert!(charge(&mut budget, 2).is_ok());
        assert!(charge(&mut budget, 1)
            .unwrap_err()
            .starts_with("E-TYPED-SOURCE-MAP-LIMIT"));
    }
    let sources = source(EQUAL);
    let mut trace = Trace::new(&sources, BodyBudget::with_limits(100, 0, 100));
    let mut edges = Vec::new();
    trace.edge(&mut edges, TypedChildRole::LetValue, Some(0));
    assert!(edges.is_empty());
    assert!(trace.check().is_err());
    trace.edge(&mut edges, TypedChildRole::LetBody, Some(0));
    assert!(edges.is_empty());
}
#[test]
fn tiny_metadata_limits_never_mask_unresolved_or_semantic_shape_limits() {
    let sources =
        source("type O<a> =None|Some(a)\nfn main(){let n=None;match n{None=>42|Some(_)=>0}}");
    let checked = check::analyze(sources.program()).unwrap();
    let ordinary = crate::typed::lower_main(&checked).unwrap_err();
    drop(checked);
    assert_eq!(
        lower_with_budget(sources, BodyBudget::with_limits(0, 0, 0)).unwrap_err(),
        ordinary
    );
    let count = 257;
    let text = format!(
        "type Many<a> = Many({})\nfn main() {{let unused=Many({});42}}",
        vec!["i32"; count].join(","),
        vec!["1"; count].join(",")
    );
    let sources = source(&text);
    let checked = check::analyze(sources.program()).unwrap();
    let ordinary = crate::typed::lower_main(&checked).unwrap_err();
    assert!(ordinary.starts_with("E-TYPED-LOCAL-COMPLEXITY"));
    drop(checked);
    assert_eq!(
        lower_with_budget(sources, BodyBudget::with_limits(0, 0, 0)).unwrap_err(),
        ordinary
    );
    let sources = source(EQUAL);
    assert!(lower_with_budget(sources, BodyBudget::with_limits(0, 0, 0))
        .unwrap_err()
        .starts_with("E-TYPED-SOURCE-MAP-LIMIT"));
}

// Corrupt valid IR and refresh every construction fingerprint deliberately.
// Rejection must come from source/correspondence checks, not receipt equality.
fn corrupted_valid_ir(text: &str, change: impl FnOnce(&mut TypedMain)) -> String {
    let sources = source(text);
    let checked = check::analyze(sources.program()).unwrap();
    let budget = BodyBudget::default();
    let mut requests =
        instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    let (mut typed, instances) =
        crate::typed::lower_main_traced(&checked, Some(&mut requests), Some(&mut trace)).unwrap();
    trace.check().unwrap();
    let data =
        super::super::finalize_function_data(&sources, &checked, &typed, &instances, &mut requests)
            .unwrap();
    change(&mut typed);
    crate::typed::verify(&typed).expect("mutation must remain ordinary verified IR");
    let actual = structure::enumerate(&typed, &mut trace.budget).unwrap();
    let mut fragments: Vec<usize> = Vec::new();
    for occurrence in &actual {
        let fragment = if let Some(parent) = occurrence.parent {
            trace.fragments[fragments[parent]]
                .children
                .iter()
                .find(|(role, _)| *role == occurrence.role)
                .unwrap()
                .1
        } else {
            trace.roots[occurrence.instance]
        };
        trace.fragments[fragment].fingerprint =
            structure::fingerprint(occurrence.node, &mut trace.budget).unwrap();
        fragments.push(fragment);
    }
    let error = match finalize(&sources, &checked, &typed, &instances, &data, &mut trace) {
        Err(error) => error,
        Ok(_) => panic!("valid but source-inconsistent IR accepted"),
    };
    assert!(error.starts_with("E-TYPED-SOURCE-MAP"), "{error}");
    assert!(!error.contains("construction attributes"), "{error}");
    error
}
#[test]
fn valid_same_typed_binder_swaps_and_unused_receipts_fail() {
    for text in [
        "fn main(){let x=1;let y=2;x+y}",
        "type P = P(i32,i32)\nfn main(){match P(1,2){P(x,y)=>x+y}}",
    ] {
        corrupted(text, |trace| {
            assert_eq!(trace.bindings.len(), 2);
            let local = trace.bindings[0].local;
            trace.bindings[0].local = trace.bindings[1].local;
            trace.bindings[1].local = local;
        });
    }
    corrupted(
        "type O<a> = None|Some(a)\nfn main(){let unused=None;42}",
        |trace| {
            let source = trace
                .source_index
                .sources()
                .nodes()
                .iter()
                .enumerate()
                .find(|(i, _)| {
                    matches!(
                        trace.source_index.node(*i),
                        Some(AstNode::Expression(Expr::Let(..)))
                    )
                })
                .unwrap()
                .0;
            trace.bindings.push(trace::BindingReceipt {
                instance: 0,
                source,
                local: LocalId(100),
            });
        },
    );
    let error = corrupted_valid_ir("fn main(){let x=1;let y=2;x+y}", |typed| {
        let crate::typed::ExprKind::Let { body, .. } = &mut typed.functions[typed.entry]
            .body
            .value_expr_mut()
            .unwrap()
            .kind
        else {
            panic!()
        };
        let crate::typed::ExprKind::Let { local, body, .. } = &mut body.kind else {
            panic!()
        };
        let crate::typed::ExprKind::Binary(_, left, _) = &mut body.kind else {
            panic!()
        };
        left.kind = crate::typed::ExprKind::Read(*local);
    });
    assert!(error.contains("active lexical source binding"), "{error}");
}
#[test]
fn valid_result_only_instance_substitution_fails_source_correspondence() {
    let error = corrupted_valid_ir("type P<a> = P\nfn make(x){P}\nfn one(x:P<i32>){1}\nfn two(x:P<bool>){2}\nfn main(){one(make(0))+two(make(0))}", |typed| {
        let makes = &typed.functions[..2];
        assert_eq!(makes.len(),2);
        assert_eq!(makes[0].params.iter().map(|(_,t,_)|t).collect::<Vec<_>>(), makes[1].params.iter().map(|(_,t,_)|t).collect::<Vec<_>>());
        assert_ne!(makes[0].result,makes[1].result);
        let crate::typed::ExprKind::Binary(_,left,right) = &mut typed.functions[typed.entry].body.value_expr_mut().unwrap().kind else {panic!()};
        **left = (**right).clone();
    });
    assert!(error.contains("source rule mismatch"), "{error}");
}
#[test]
fn both_observers_stop_on_either_shared_failure() {
    let sources = source(EQUAL);
    // Source indexing exhausts first; request discovery cannot grow afterwards.
    let budget = BodyBudget::with_limits(0, 100, 100);
    let mut requests =
        instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    assert!(!requests.charge(1));
    assert!(requests.requests.is_empty());
    assert!(trace.observe(|_| Ok(())).is_none());
    // A large failed request charge leaves work, but still disables body tracing.
    let budget = BodyBudget::with_limits(1000, 1000, 1000);
    let mut requests =
        instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    let remaining = trace.budget.work.get();
    assert!(!requests.charge(remaining + 1));
    assert_eq!(trace.budget.work.get(), remaining);
    let mut entered = false;
    assert!(trace
        .observe(|_| {
            entered = true;
            Ok(())
        })
        .is_none());
    assert!(!entered);
    assert!(trace.fragments.is_empty());
}

#[test]
fn shared_stop_signal_preserves_actual_nonlimit_diagnostic() {
    let sources = source(EQUAL);
    let budget = BodyBudget::default();
    let requests = instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    trace.observe::<()>(|_| Err(fail("injected correspondence failure")));
    assert_eq!(
        check_traces(&requests, &trace).unwrap_err(),
        fail("injected correspondence failure")
    );
}

#[test]
fn disposition_relation_scans_and_binding_lookups_reserve_work_first() {
    for (text, kind) in [
        (CAPTURE, 0),
        ("type O<a> = None|Some(a)\nfn main(){let x=None;42}", 1),
        (
            "type O<a> = None|Some(a)\nfn main(){let x=None;let y=x;42}",
            2,
        ),
    ] {
        let sources = source(text);
        let checked = check::analyze(sources.program()).unwrap();
        let mut trace = Trace::new(&sources, BodyBudget::default());
        let (_, instances) =
            crate::typed::lower_main_traced(&checked, None, Some(&mut trace)).unwrap();
        trace.check().unwrap();
        let index = trace.dispositions.iter().position(|receipt| match (&receipt.disposition, kind) {
            (RawDisposition::Output(fragment), 0) => matches!(&trace.fragments[*fragment].origin, RawOrigin::Generated { relations, .. } if !relations.is_empty()),
            (RawDisposition::StaticShapeOnly {..}, 1) | (RawDisposition::AliasOfShape {..}, 2) => true,
            _ => false,
        }).unwrap();
        let receipt = trace.dispositions.remove(index);
        trace.dispositions.clear();
        trace.dispositions.push(receipt);
        // Exactly enough for the outer receipt. The relation scan or indexed
        // binding lookup must fail its reservation before touching later data.
        trace.budget.work.set(1);
        let error = validate_dispositions(&mut trace, &instances, &[]).unwrap_err();
        assert!(error.starts_with("E-TYPED-SOURCE-MAP-LIMIT"), "{error}");
        assert_eq!(trace.budget.work.get(), 0);
    }
}

#[test]
fn complete_native_gate_precedes_latched_metadata_and_function_finalization() {
    for text in [
        "type E = Empty|Has(u64) fn main(){Empty}",
        "type Marker<a> = Marker fn take(x:Marker<u32>){42} fn main(){take(Marker)}",
        "type Marker<a> = Marker fn take(x:Marker<ptr<void>>){42} fn main(){take(Marker)}",
        "fn take(x:ptr<void>){is_null(x)} fn main(){take(c_null())}",
        "fn main(){u8(42)}",
    ] {
        let sources = source(text);
        let checked = check::analyze(sources.program()).unwrap();
        crate::typed::lower_main(&checked).unwrap();
        drop(checked);
        let error = lower_with_budget(sources, BodyBudget::with_limits(0, 0, 0)).unwrap_err();
        assert!(
            error.starts_with("E-TYPED-UNSUPPORTED") && error.contains("does not map native"),
            "{text}: {error}"
        );
    }
}
