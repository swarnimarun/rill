//! Independent source-main, logical identity and malformed-consumer regressions.
use super::*;
use crate::interpreter::Value;

fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn rejects(program: &TypedMain) {
    assert!(verify(program).is_err());
    assert!(evaluate(program).is_err());
    assert!(emit_c(program).is_err());
}

fn main_local_after_trap(source: &str, expected: i32) {
    let program = lower(source);
    verify(&program).unwrap();
    let Computation::Value(entry) = &program.functions[program.entry].body else {
        panic!("concrete source result lost")
    };
    let ExprKind::Let {
        local,
        mutable: true,
        value,
        body,
    } = &entry.kind
    else {
        panic!("source mutable binding lost")
    };
    // Evaluate the exact checked main initializer, then its exact remainder at
    // main depth 1, retaining access to that same source local after the trap.
    let mut locals = CopyLocals::default();
    let mut state = EvalState::default();
    let mut owners = std::collections::HashMap::new();
    let initial = evaluate_expr(value, &mut locals, &program, 1, &mut state, &mut owners).unwrap();
    locals.insert(*local, initial);
    let result = evaluate_expr(body, &mut locals, &program, 1, &mut state, &mut owners);
    assert_eq!(result.unwrap_err(), "runtime trap");
    assert_eq!(locals.read(local).unwrap(), Value::Int(expected));
    assert_eq!(
        program.functions[program.entry].result,
        ResultRepr::Value(Type::Int)
    );
}

#[test]
fn independent_source_main_depth_keeps_31_effects_and_real_caller_slot() {
    main_local_after_trap("fn spin(inout n) { n <- n + 1; spin(inout n) } fn main() { let mut n = 0; let unused = spin(inout n); 42 }", 31);
}

#[test]
fn independent_value_and_read_snapshots_precede_later_argument_mutation() {
    for computed_read in [false, true] {
        let mode = if computed_read { "read " } else { "" };
        let first = if computed_read { "{ x + 0 }" } else { "x" };
        let initial = if computed_read { "{ n + 0 }" } else { "n" };
        let source = format!("fn spin({mode}before, ignored, inout x) {{ let check = 1 / (if x == before + 1 {{ 1 }} else {{ 0 }}); spin({first}, {{ x <- x + 1; 0 }}, inout x) }} fn main() {{ let mut n = 0; let unused = spin({initial}, {{ n <- n + 1; 0 }}, inout n); 42 }}");
        // The attempted call at depth 32 evaluates all arguments before trapping.
        // Incorrect earlier snapshots trigger division by zero after n=1 instead.
        main_local_after_trap(&source, 32);
    }
}

#[test]
fn independent_disjoint_read_inout_fields_preserve_exact_main_slot() {
    let program = lower("type Pair = { x: i32; y: i32 } fn spin(read before, inout n) { n <- n + before; spin(before, inout n) } fn main() { let mut p = record Pair { x = 1; y = 0 }; spin(p.x, inout p.y) }");
    verify(&program).unwrap();
    let Computation::NoNormal(entry) = &program.functions[program.entry].body else {
        panic!()
    };
    let NoNormalKind::Let {
        local, value, body, ..
    } = &entry.kind
    else {
        panic!()
    };
    let NoNormalKind::Call(_, args) = &body.kind else {
        panic!()
    };
    let CallArg::ReadField(read) = &args[0] else {
        panic!()
    };
    let CallArg::InoutField(write) = &args[1] else {
        panic!()
    };
    let mut locals = CopyLocals::default();
    let mut state = EvalState::default();
    let mut owners = std::collections::HashMap::new();
    let initial = evaluate_expr(value, &mut locals, &program, 1, &mut state, &mut owners).unwrap();
    locals.insert(*local, initial);
    let outcome =
        results::evaluate_no_normal(body, &mut locals, &program, 1, &mut state, &mut owners);
    assert_eq!(outcome.unwrap_err(), "runtime trap");
    assert_eq!(locals.read_field(read).unwrap(), Value::Int(1));
    assert_eq!(locals.read_field(write).unwrap(), Value::Int(31));
}

#[test]
fn independent_no_normal_foreign_brand_and_occurrence_are_rejected_by_consumers() {
    let source = "fn spin() { spin() } fn main() { spin() }";
    let first = lower(source);
    let second = lower(source);
    let Computation::NoNormal(foreign) = &second.functions[second.entry].body else {
        panic!()
    };
    let mut forged = first.clone();
    let Computation::NoNormal(body) = &mut forged.functions[first.entry].body else {
        panic!()
    };
    body.result = foreign.result.clone();
    rejects(&forged);
    let mut forged = first.clone();
    let Computation::NoNormal(body) = &mut forged.functions[first.entry].body else {
        panic!()
    };
    body.occurrence = usize::MAX;
    rejects(&forged);
}

#[test]
fn independent_two_weak_results_do_not_collapse_to_one_void_identity() {
    let program =
        lower("fn spin() { spin() } fn main() { let first = spin(); let second = spin(); 42 }");
    let ids: Vec<_> = program
        .functions
        .iter()
        .filter_map(|function| {
            if let ResultRepr::NoNormal(id) = &function.result {
                Some(id)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        ids.len(),
        2,
        "two independent weak result classes must make two exact instances"
    );
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn independent_retained_literal_and_binder_change_fail_all_public_consumers() {
    let source = "fn spin() { spin() } fn main() { let unused = spin(); 42 }";
    let program = lower(source);
    let mut forged = program.clone();
    let Computation::Value(entry) = &mut forged.functions[program.entry].body else {
        panic!()
    };
    let ExprKind::NoStorageLet { body, .. } = &mut entry.kind else {
        panic!()
    };
    body.kind = ExprKind::Int(41);
    rejects(&forged);
    let mut forged = program.clone();
    let Computation::Value(entry) = &mut forged.functions[program.entry].body else {
        panic!()
    };
    let ExprKind::NoStorageLet { binding, .. } = &mut entry.kind else {
        panic!()
    };
    binding.occurrence = usize::MAX;
    rejects(&forged);
}

#[test]
fn independent_unselected_no_storage_branch_keeps_source_proof_obligations() {
    let mut program =
        lower("fn spin() { spin() } fn main() { if true { 42 } else { let unused = spin(); 7 } }");
    assert_eq!(evaluate(&program).unwrap(), Value::Int(42));
    let Computation::Value(entry) = &mut program.functions[program.entry].body else {
        panic!()
    };
    let ExprKind::If(_, _, skipped) = &mut entry.kind else {
        panic!()
    };
    let ExprKind::NoStorageLet { value, .. } = &mut skipped.kind else {
        panic!()
    };
    value.occurrence = usize::MAX;
    rejects(&program);
}

#[test]
fn independent_actual_depth_128_boundary_precedes_recursive_consumers() {
    let original = lower("fn spin() { spin() } fn main() { let unused = spin(); 42 }");
    for (wrappers, complexity) in [(127, false), (128, true)] {
        let mut program = original.clone();
        let Computation::Value(entry) = &mut program.functions[program.entry].body else {
            panic!()
        };
        let ExprKind::NoStorageLet { body, .. } = &mut entry.kind else {
            panic!()
        };
        let mut tail = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(42),
        };
        for _ in 0..wrappers {
            tail = ExprNode {
                ty: Type::Int,
                kind: ExprKind::ByteLen(Box::new(tail)),
            };
        }
        **body = tail;
        // Body starts at depth 1. Depth 128 reaches correspondence rejection;
        // depth 129 must fail the iterative preflight before generic recursion.
        let error = verify(&program).unwrap_err();
        assert_eq!(
            error.contains("CONTROL-COMPLEXITY"),
            complexity,
            "{wrappers}: {error}"
        );
        rejects(&program);
    }
}

#[test]
fn independent_removed_proof_cannot_hide_in_an_unselected_concrete_branch() {
    for source in [
        "fn spin() { spin() } fn main() { spin() }",
        "fn spin() { spin() } fn main() { if true { 42 } else { let unused = spin(); 7 } }",
    ] {
        let mut program = lower(source);
        assert!(program.logical.control.take().is_some());
        rejects(&program);
    }
}
