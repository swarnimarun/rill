//! Source-only coverage for the control-proof foundation. These tests do not
//! expand layout admission or rely on a no-normal result annotation.

use crate::ast::{Expr, Program};
use crate::typed::{control, lower_main, ExprKind, ExprNode, Type, TypedMain};

fn parse(source: &str) -> Program {
    crate::parser::parse(source).unwrap_or_else(|error| panic!("{error}\n{source}"))
}

fn function(program: &Program, name: &str) -> usize {
    program
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn body<'a>(program: &'a Program, name: &str) -> &'a Expr {
    &program.functions[function(program, name)].body
}

// Preorder traversal preserves source order and deliberately visits every child,
// including short-circuit operands, loop bodies and unreachable continuations.
fn expressions(root: &Expr) -> Vec<&Expr> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        result.push(expression);
        match expression {
            Expr::Binary(_, left, right)
            | Expr::Let(_, left, right)
            | Expr::LetMut(_, left, right)
            | Expr::While(left, right) => {
                pending.push(right);
                pending.push(left);
            }
            Expr::If(condition, yes, no) => {
                pending.push(no);
                pending.push(yes);
                pending.push(condition);
            }
            Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => pending.push(value),
            Expr::Record(_, fields) => {
                pending.extend(fields.iter().rev().map(|(_, value)| value));
            }
            Expr::Call(_, arguments) => pending.extend(arguments.iter().rev()),
            Expr::Match(value, arms) => {
                pending.extend(arms.iter().rev().map(|arm| &arm.body));
                pending.push(value);
            }
            Expr::Int(_)
            | Expr::Str(_)
            | Expr::CStr(_)
            | Expr::Bool(_)
            | Expr::Unit
            | Expr::Var(_)
            | Expr::Move(_)
            | Expr::Inout(_)
            | Expr::InoutField(_, _)
            | Expr::Field(_, _) => {}
        }
    }
    result
}

fn source_flow(source: &str, expected: &[(&str, Option<bool>)]) {
    let program = parse(source);
    let checked =
        crate::check::analyze(&program).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let facts = control::SourceFacts::build(&checked).unwrap();
    facts.verify_source(&checked).unwrap();
    for &(name, expected) in expected {
        assert_eq!(
            facts.function_may_return(function(&program, name)),
            expected,
            "function {name}\n{source}"
        );
        assert_eq!(facts.expression_may_return(body(&program, name)), expected);
        if expected.is_some() {
            for expression in expressions(body(&program, name)) {
                assert!(
                    facts.expression_may_return(expression).is_some(),
                    "missing retained child {expression:?}\n{source}"
                );
            }
        }
    }
    assert_eq!(facts.function_may_return(program.functions.len()), None);
}

fn binding_uses(source: &str, name: &str, expected: &[bool]) {
    let program = parse(source);
    let checked =
        crate::check::analyze(&program).unwrap_or_else(|error| panic!("{error}\n{source}"));
    let facts = control::SourceFacts::build(&checked).unwrap();
    let bindings: Vec<_> = expressions(body(&program, "main"))
        .into_iter()
        .filter(|expression| {
            matches!(expression, Expr::Let(binding, _, _) | Expr::LetMut(binding, _, _) if binding == name)
        })
        .map(|expression| facts.binding_used(expression))
        .collect();
    assert_eq!(
        bindings,
        expected.iter().copied().map(Some).collect::<Vec<_>>(),
        "binding {name}\n{source}"
    );
}

fn witness_rejects_change(original: &str, changed: &str) {
    let original_program = parse(original);
    let original_checked = crate::check::analyze(&original_program)
        .unwrap_or_else(|error| panic!("original: {error}\n{original}"));
    let facts = control::SourceFacts::build(&original_checked).unwrap();
    facts.verify_source(&original_checked).unwrap();

    let changed_program = parse(changed);
    let changed_checked = crate::check::analyze(&changed_program)
        .unwrap_or_else(|error| panic!("changed: {error}\n{changed}"));
    assert!(
        facts.verify_source(&changed_checked).is_err(),
        "accepted changed checked source\noriginal: {original}\nchanged: {changed}"
    );
}

fn lower(source: &str) -> TypedMain {
    let program = parse(source);
    let checked =
        crate::check::analyze(&program).unwrap_or_else(|error| panic!("{error}\n{source}"));
    lower_main(&checked).unwrap_or_else(|error| panic!("{error}\n{source}"))
}

#[test]
fn source_self_and_mutual_recursion_have_no_finite_normal_return() {
    source_flow(
        "fn forever() { forever() } fn main() { forever() }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
    source_flow(
        "fn first(n) { second(n) } fn second(n) { first(n) } fn main() { first(0) }",
        &[
            ("first", Some(false)),
            ("second", Some(false)),
            ("main", Some(false)),
        ],
    );
}

#[test]
fn a_base_branch_propagates_through_the_entire_recursive_group() {
    // Literal false does not erase the finite return derivation.
    source_flow(
        "fn first(n) { if false { 42 } else { second(n) } } fn second(n) { first(n) } fn main() { second(0) }",
        &[
            ("first", Some(true)),
            ("second", Some(true)),
            ("main", Some(true)),
        ],
    );
    source_flow(
        "fn again(n) { if n == 0 { 42 } else { again(n - 1) } } fn main() { again(3) }",
        &[("again", Some(true)), ("main", Some(true))],
    );
}

#[test]
fn source_calls_require_their_arguments_and_exact_callee_to_return() {
    source_flow(
        "fn forever() { forever() } fn answer(unused) { 42 } fn main() { answer(forever()) }",
        &[
            ("forever", Some(false)),
            ("answer", Some(true)),
            ("main", Some(false)),
        ],
    );
    source_flow(
        "fn forever(n) { forever(n) + 1 } fn identity(n) { n } fn main() { forever(identity(0)) }",
        &[
            ("forever", Some(false)),
            ("identity", Some(true)),
            ("main", Some(false)),
        ],
    );
    source_flow(
        "fn forever(n) { forever(n) + 1 } fn observe(read n: i32) { n } fn main() { observe(forever(0)) }",
        &[
            ("forever", Some(false)),
            ("observe", Some(true)),
            ("main", Some(false)),
        ],
    );
}

#[test]
fn strict_operations_constructors_records_and_builtins_keep_all_operands() {
    for tail in [
        "forever(0) + 1",
        "1 + forever(0)",
        "Pair(1, forever(0))",
        "record R { y = forever(0); x = 1 }",
        "byte_at(\"abc\", forever(0))",
        "bytes_set(bytes_new(2), 0, forever(0))",
    ] {
        let source = format!(
            "type Pair = Pair(i32, i32) type R = {{ x: i32; y: i32 }} fn forever(n) {{ forever(n) + 1 }} fn main() {{ {tail} }}"
        );
        source_flow(&source, &[("forever", Some(false)), ("main", Some(false))]);
    }
    // A possible trap is not a seed proving no normal return.
    for tail in ["1 / 0", "byte_at(\"\", 0)", "bytes_get(bytes_new(0), 0)"] {
        source_flow(&format!("fn main() {{ {tail} }}"), &[("main", Some(true))]);
    }
}

#[test]
fn short_circuit_operators_preserve_rhs_without_requiring_it_to_return() {
    for tail in [
        "false && forever()",
        "true && forever()",
        "false || forever()",
        "true || forever()",
    ] {
        source_flow(
            &format!("fn forever() {{ forever() }} fn main() {{ {tail} }}"),
            &[("forever", Some(false)), ("main", Some(true))],
        );
    }
    for tail in ["forever() && true", "forever() || false"] {
        source_flow(
            &format!("fn forever() {{ forever() }} fn main() {{ {tail} }}"),
            &[("forever", Some(false)), ("main", Some(false))],
        );
    }
}

#[test]
fn loops_allow_zero_iterations_but_require_the_condition_to_return() {
    for condition in ["true", "false"] {
        source_flow(
            &format!(
                "fn forever() {{ forever() }} fn main() {{ while {condition} {{ forever(); }} }}"
            ),
            &[("forever", Some(false)), ("main", Some(true))],
        );
    }
    source_flow(
        "fn forever() { forever() } fn main() { while forever() { () } }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
}

#[test]
fn if_requires_its_condition_and_keeps_both_result_arms() {
    source_flow(
        "fn forever() { forever() } fn main() { if forever() { 1 } else { 2 } }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
    source_flow(
        "fn forever() { forever() } fn main() { if true { forever() } else { forever() } }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
    source_flow(
        "fn forever() { forever() } fn main() { if true { forever() } else { 42 } }",
        &[("forever", Some(false)), ("main", Some(true))],
    );
}

#[test]
fn exhaustive_matches_keep_all_arms_and_the_scrutinee() {
    source_flow(
        "type Tag = First | Second | Third fn forever() { forever() } fn main() { match First { First => forever() | Second => forever() | Third => forever() } }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
    source_flow(
        "type Tag = First | Second | Third fn forever() { forever() } fn main() { match First { First => forever() | Second => forever() | Third => 42 } }",
        &[("forever", Some(false)), ("main", Some(true))],
    );
    source_flow(
        "fn forever() { forever() } fn main() { match forever() { true => 1 | false => 2 } }",
        &[("forever", Some(false)), ("main", Some(false))],
    );
    source_flow(
        "fn forever() { forever() } fn main() { match true { _ => forever() | false => 42 } }",
        &[("forever", Some(false)), ("main", Some(true))],
    );
}

#[test]
fn a_nonreturning_initializer_retains_its_full_continuation_and_calls() {
    let source = "fn forever() { forever() } fn later() { 42 } fn main() { let unused = forever(); if false { later() } else { 7 } }";
    source_flow(
        source,
        &[
            ("forever", Some(false)),
            ("later", Some(true)),
            ("main", Some(false)),
        ],
    );
    let program = parse(source);
    let checked = crate::check::analyze(&program).unwrap();
    let facts = control::SourceFacts::build(&checked).unwrap();
    let binding = body(&program, "main");
    let Expr::Let(_, value, continuation) = binding else {
        panic!("expected retained source let")
    };
    assert_eq!(facts.binding_used(binding), Some(false));
    assert_eq!(facts.expression_may_return(value), Some(false));
    assert_eq!(facts.expression_may_return(continuation), Some(true));
    assert_eq!(facts.binding_used(continuation), None);
}

#[test]
fn unrelated_definitions_are_not_given_reachable_source_facts() {
    source_flow(
        "fn orphan() { orphan() } fn unused(n) { n + 1 } fn main() { 42 }",
        &[("orphan", None), ("unused", None), ("main", Some(true))],
    );
}

#[test]
fn binding_uses_distinguish_shadowing_and_initializer_scope() {
    binding_uses(
        "fn main() { let item = 0; let item = 1; item }",
        "item",
        &[false, true],
    );
    binding_uses(
        "fn main() { let item = 0; let item = item + 1; item }",
        "item",
        &[true, true],
    );
    binding_uses(
        "fn main() { let item = 0; let ignored = { let item = 1; item }; item }",
        "item",
        &[true, true],
    );
    binding_uses(
        "fn main() { let item = 0; let item_suffix = 1; item_suffix }",
        "item",
        &[false],
    );
}

#[test]
fn pattern_and_branch_scopes_do_not_leak_binding_identity() {
    binding_uses(
        "type Pair = Pair(i32, i32) fn main() { let item = 0; match Pair(1, 2) { Pair(item, _) => item } }",
        "item",
        &[false],
    );
    binding_uses(
        "fn main() { let item = 0; match true { true => { let item = 1; item } | false => item } }",
        "item",
        &[true, true],
    );
    binding_uses(
        "fn main() { let item = 0; if true { let item = 1; item } else { item } }",
        "item",
        &[true, true],
    );
    binding_uses(
        "fn main() { let item = true; match item { item => item } }",
        "item",
        &[true],
    );
}

#[test]
fn writes_and_root_loans_count_as_binding_uses_without_reads() {
    binding_uses("fn main() { let mut item = 0; item <- 1 }", "item", &[true]);
    binding_uses(
        "fn set(inout n: i32) { n <- 1 } fn main() { let mut item = 0; set(inout item) }",
        "item",
        &[true],
    );
    binding_uses(
        "fn inspect(read n: i32) { 42 } fn main() { let item = 0; inspect(item) }",
        "item",
        &[true],
    );
    binding_uses(
        "fn main() { let mut item = 0; let mut item = 1; item <- 2 }",
        "item",
        &[false, true],
    );
}

#[test]
fn projected_reads_writes_and_loans_count_the_exact_root_binding() {
    for tail in [
        "item.x",
        "item.x <- 2",
        "set(inout item.x)",
        "inspect(item.x)",
    ] {
        binding_uses(
            &format!("type R = {{ x: i32 }} fn set(inout n: i32) {{ n <- 1 }} fn inspect(read n: i32) {{ 42 }} fn main() {{ let mut item = record R {{ x = 0 }}; {tail} }}"),
            "item",
            &[true],
        );
    }
    binding_uses(
        "type R = { x: i32 } fn main() { let mut item = record R { x = 0 }; let mut item = record R { x = 1 }; item.x <- 2 }",
        "item",
        &[false, true],
    );
}

#[test]
fn uses_in_unreachable_tails_short_circuit_rhs_and_loop_bodies_are_retained() {
    for source in [
        "fn forever() { forever() } fn main() { let item = forever(); item }",
        "fn main() { let item = true; false && item }",
        "fn main() { let item = true; true || item }",
        "fn main() { let item = 1; while false { let ignored = item; () } }",
        "fn main() { let item = 1; if false { item } else { 42 } }",
    ] {
        binding_uses(source, "item", &[true]);
    }
}

#[test]
fn source_witness_uses_structure_not_expression_addresses() {
    let source = "fn plus(n) { n + 1 } fn main() { let n = 41; plus(n) }";
    let program = parse(source);
    let checked = crate::check::analyze(&program).unwrap();
    let facts = control::SourceFacts::build(&checked).unwrap();
    let reparsed = parse(source);
    let rechecked = crate::check::analyze(&reparsed).unwrap();
    facts.verify_source(&rechecked).unwrap();
    // Occurrence queries still belong to the exact input whose facts were built.
    assert_eq!(facts.expression_may_return(body(&reparsed, "main")), None);
    assert_eq!(facts.binding_used(body(&reparsed, "main")), None);
}

#[test]
fn source_witness_rejects_type_preserving_literals_operators_and_branches() {
    for (original, changed) in [
        ("fn main() { 41 }", "fn main() { 42 }"),
        ("fn main() { true }", "fn main() { false }"),
        ("fn main() { \"first\" }", "fn main() { \"other\" }"),
        ("fn main() { 40 + 2 }", "fn main() { 40 - 2 }"),
        ("fn main() { true && false }", "fn main() { true || false }"),
        (
            "fn main() { if true { 1 } else { 2 } }",
            "fn main() { if false { 1 } else { 2 } }",
        ),
        (
            "fn main() { if true { 1 } else { 2 } }",
            "fn main() { if true { 2 } else { 1 } }",
        ),
        ("fn main() { let unused = 1 / 0; 42 }", "fn main() { 42 }"),
    ] {
        witness_rejects_change(original, changed);
    }
}

#[test]
fn source_witness_rejects_call_target_argument_order_modes_and_binding_changes() {
    for (original, changed) in [
        (
            "fn first(n) { n + 1 } fn second(n) { n + 2 } fn main() { let x = first(1); second(x) }",
            "fn first(n) { n + 1 } fn second(n) { n + 2 } fn main() { let x = first(1); first(x) }",
        ),
        (
            "fn sub(a, b) { a - b } fn main() { sub(1, 2) }",
            "fn sub(a, b) { a - b } fn main() { sub(2, 1) }",
        ),
        (
            "fn sub(a, b) { a - b } fn main() { let a = 1; let b = 2; sub(a, b) }",
            "fn sub(a, b) { a - b } fn main() { let a = 1; let b = 2; sub(b, a) }",
        ),
        (
            "fn get(read n: i32) { n } fn main() { let n = 1; get(n) }",
            "fn get(n: i32) { n } fn main() { let n = 1; get(n) }",
        ),
        (
            "fn main() { let first = 1; let second = 2; second }",
            "fn main() { let first = 1; let second = 2; first }",
        ),
        ("fn main() { let n = 1; n }", "fn main() { let mut n = 1; n }"),
    ] {
        witness_rejects_change(original, changed);
    }
}

#[test]
fn source_witness_rejects_pattern_constructor_and_field_order_changes() {
    for (original, changed) in [
        (
            "fn main() { match 1 { 0 => 42 | _ => 7 } }",
            "fn main() { match 1 { 1 => 42 | _ => 7 } }",
        ),
        (
            "fn main() { match true { true => 1 | false => 2 } }",
            "fn main() { match true { false => 2 | true => 1 } }",
        ),
        (
            "type Tag = First | Second fn main() { First }",
            "type Tag = First | Second fn main() { Second }",
        ),
        (
            "type R = { x: i32; y: i32 } fn main() { record R { x = 1; y = 2 } }",
            "type R = { x: i32; y: i32 } fn main() { record R { y = 2; x = 1 } }",
        ),
        (
            "type R = { x: i32; y: i32 } fn main() { let p = record R { x = 1; y = 2 }; p.x }",
            "type R = { x: i32; y: i32 } fn main() { let p = record R { x = 1; y = 2 }; p.y }",
        ),
        (
            "type R = { x: i32; y: i32 } fn main() { let mut p = record R { x = 1; y = 2 }; p.x <- 3 }",
            "type R = { x: i32; y: i32 } fn main() { let mut p = record R { x = 1; y = 2 }; p.y <- 3 }",
        ),
        (
            "type R = { x: i32; y: i32 } fn set(inout n: i32) { n <- 3 } fn main() { let mut p = record R { x = 1; y = 2 }; set(inout p.x) }",
            "type R = { x: i32; y: i32 } fn set(inout n: i32) { n <- 3 } fn main() { let mut p = record R { x = 1; y = 2 }; set(inout p.y) }",
        ),
    ] {
        witness_rejects_change(original, changed);
    }
}

#[test]
fn source_witness_retains_changes_in_nonreturning_tails_and_skipped_children() {
    for (original_tail, changed_tail) in [
        ("let unused = forever(); 1", "let unused = forever(); 2"),
        (
            "if false { forever() } else { 1 }",
            "if false { forever() } else { 2 }",
        ),
        (
            "false && { let unused = 1; forever() }",
            "false && { let unused = 2; forever() }",
        ),
        (
            "while false { let unused = 1; forever(); }",
            "while false { let unused = 2; forever(); }",
        ),
    ] {
        witness_rejects_change(
            &format!("fn forever() {{ forever() }} fn main() {{ {original_tail} }}"),
            &format!("fn forever() {{ forever() }} fn main() {{ {changed_tail} }}"),
        );
    }
}

#[test]
fn ir_reconstructs_self_and_mutual_recursion_without_result_type_seeds() {
    for source in [
        "fn forever(n) { forever(n) + 1 } fn main() { forever(0) }",
        "fn first(n) { second(n) + 1 } fn second(n) { first(n) + 1 } fn main() { first(0) }",
    ] {
        let typed = lower(source);
        let facts = control::IrFacts::build(&typed).unwrap();
        for index in 0..typed.functions.len() {
            assert_eq!(facts.function_may_return(index), Some(false), "{source}");
        }
        assert_eq!(facts.function_may_return(typed.functions.len()), None);
    }
}

#[test]
fn ir_reconstructs_returning_recursive_groups_and_conservative_control() {
    for source in [
        "fn again(n) { if false { 42 } else { again(n) } } fn main() { again(0) }",
        "fn first(n) { second(n) } fn second(n) { if false { 42 } else { first(n) } } fn main() { first(0) }",
    ] {
        let typed = lower(source);
        let facts = control::IrFacts::build(&typed).unwrap();
        for index in 0..typed.functions.len() {
            assert_eq!(facts.function_may_return(index), Some(true), "{source}");
        }
    }
    for (source, expected) in [
        ("fn forever(n) { forever(n) && true } fn main() { false && forever(0) }", true),
        ("fn forever(n) { forever(n) && true } fn main() { true || forever(0) }", true),
        ("fn forever(n) { forever(n) && true } fn main() { forever(0) && false }", false),
        ("fn forever(n) { forever(n) + 1 } fn main() { while true { forever(0); } }", true),
        ("fn forever(n) { forever(n) && true } fn main() { while forever(0) { () } }", false),
        ("fn forever(n) { forever(n) + 1 } fn main() { match true { true => forever(0) | false => forever(1) } }", false),
        ("fn forever(n) { forever(n) + 1 } fn main() { match true { true => forever(0) | false => 42 } }", true),
        ("fn forever(n) { forever(n) + 1 } fn answer(n: i32) { 42 } fn main() { answer(forever(0)) }", false),
        ("fn forever(n) { forever(n) + 1 } fn observe(read n: i32) { n } fn main() { observe(forever(0)) }", false),
    ] {
        let typed = lower(source);
        let facts = control::IrFacts::build(&typed).unwrap();
        assert_eq!(facts.function_may_return(typed.entry), Some(expected), "{source}");
    }
}

#[test]
fn rebuilding_ir_facts_observes_actual_body_and_exact_call_target_changes() {
    let mut typed = lower("fn forever(n) { forever(n) + 1 } fn main() { forever(0) }");
    let original = control::IrFacts::build(&typed).unwrap();
    assert_eq!(original.function_may_return(typed.entry), Some(false));
    let ExprKind::Call(callee, _) = &typed.functions[typed.entry].body.value_expr().unwrap().kind
    else {
        panic!("expected closed recursive call")
    };
    let callee = *callee;
    typed.functions[callee].body = (ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(42),
    })
    .into();
    let rebuilt = control::IrFacts::build(&typed).unwrap();
    assert_eq!(rebuilt.function_may_return(callee), Some(true));
    assert_eq!(rebuilt.function_may_return(typed.entry), Some(true));

    let mut typed = lower("fn forever(n) { forever(n) + 1 } fn answer(n: i32) { 42 } fn main() { let unused = answer(0); forever(0) }");
    assert_eq!(
        control::IrFacts::build(&typed)
            .unwrap()
            .function_may_return(typed.entry),
        Some(false)
    );
    let ExprKind::Let { value, body, .. } = &mut typed.functions[typed.entry]
        .body
        .value_expr_mut()
        .unwrap()
        .kind
    else {
        panic!("expected retained closed let")
    };
    let ExprKind::Call(returning, _) = &value.kind else {
        panic!("expected returning prefix call")
    };
    let ExprKind::Call(target, _) = &mut body.kind else {
        panic!("expected recursive tail call")
    };
    *target = *returning;
    assert_eq!(
        control::IrFacts::build(&typed)
            .unwrap()
            .function_may_return(typed.entry),
        Some(true)
    );
}

#[test]
fn ir_root_and_projected_loans_do_not_create_nonreturning_seeds() {
    for source in [
        "fn set(inout n: i32) { n <- 1 } fn main() { let mut n = 0; set(inout n) }",
        "fn inspect(read n: i32) { 42 } fn main() { let n = 0; inspect(n) }",
        "type R = { x: i32; y: i32 } fn set(inout n: i32, read other: i32) { n <- other } fn main() { let mut p = record R { x = 0; y = 1 }; set(inout p.x, p.y) }",
    ] {
        let typed = lower(source);
        let facts = control::IrFacts::build(&typed).unwrap();
        for index in 0..typed.functions.len() {
            assert_eq!(facts.function_may_return(index), Some(true), "{source}");
        }
    }
}

#[test]
fn malformed_ir_entry_and_call_targets_are_errors_not_nonreturn_proofs() {
    let original = lower("fn forever(n) { forever(n) + 1 } fn main() { forever(0) }");
    let mut invalid_entry = original.clone();
    invalid_entry.entry = invalid_entry.functions.len();
    assert!(control::IrFacts::build(&invalid_entry).is_err());

    let mut invalid_call = original;
    let ExprKind::Call(target, _) = &mut invalid_call.functions[invalid_call.entry]
        .body
        .value_expr_mut()
        .unwrap()
        .kind
    else {
        panic!("expected recursive call")
    };
    *target = usize::MAX;
    assert!(control::IrFacts::build(&invalid_call).is_err());
}

#[test]
fn source_witness_retains_unused_declarations_and_native_configuration() {
    for (original, changed) in [
        (
            "type Flag = Off | On fn main() { 1 }",
            "type Flag = Off | Other fn main() { 1 }",
        ),
        ("@include(\"stdint.h\"); fn main() { 1 }", "fn main() { 1 }"),
        (
            "@link(\"m\"); fn main() { 1 }",
            "@link(\"other\"); fn main() { 1 }",
        ),
        ("@pkg_config(\"sdl3\"); fn main() { 1 }", "fn main() { 1 }"),
        (
            "extern fn unused(x: i32) -> i32; fn main() { 1 }",
            "fn main() { 1 }",
        ),
        (
            "@include(\"stdint.h\"); extern const UNUSED: i32; fn main() { 1 }",
            "@include(\"stdint.h\"); fn main() { 1 }",
        ),
        (
            "fn unused() { 2 } fn main() { 1 }",
            "fn replacement() { 2 } fn main() { 1 }",
        ),
    ] {
        witness_rejects_change(original, changed);
    }
}

#[test]
fn source_witness_pins_signature_quantifiers_independently_of_expression_types() {
    let original = parse("fn again(x) { again(x) } fn main() { again(1) }");
    let checked = crate::check::analyze(&original).unwrap();
    let facts = control::SourceFacts::build(&checked).unwrap();
    let mut altered = crate::check::analyze(&original).unwrap();
    let signature = altered.functions.get_mut("again").unwrap();
    assert!(!signature.quantified.is_empty());
    signature.quantified.clear();
    assert!(facts.verify_source(&altered).is_err());
}

#[test]
fn ir_no_normal_annotations_never_seed_nonreturning_facts() {
    use crate::typed::{Computation, NoNormalExpr, NoNormalKind, ResultRepr};
    let mut typed =
        lower("type Flag<a> = Off | On fn returning() { Off } fn main() { returning() }");
    let identity = typed.logical.opaque_id(0).unwrap();
    let returning = 0;
    let entry = typed.entry;
    typed.functions[entry].result = ResultRepr::NoNormal(identity.clone());
    typed.functions[entry].body = Computation::NoNormal(NoNormalExpr {
        occurrence: 0,
        result: identity.clone(),
        kind: NoNormalKind::Call(returning, vec![]),
    });
    let facts = control::IrFacts::build(&typed).unwrap();
    let Computation::NoNormal(body) = &typed.functions[entry].body else {
        unreachable!()
    };
    assert_eq!(facts.function_may_return(entry), Some(true));
    assert_eq!(facts.no_normal_may_return(body), Some(true));
    typed.functions[entry].body = Computation::NoNormal(NoNormalExpr {
        occurrence: 0,
        result: identity,
        kind: NoNormalKind::Call(entry, vec![]),
    });
    assert_eq!(
        control::IrFacts::build(&typed)
            .unwrap()
            .function_may_return(entry),
        Some(false)
    );
}

#[test]
fn ir_no_storage_initializer_has_its_own_fact_even_when_enclosing_body_cannot_return() {
    use crate::typed::{Computation, NoNormalExpr, NoNormalKind, NoStorageBinding};
    let mut typed =
        lower("type Flag<a> = Off | On fn returning() { Off } fn main() { returning() }");
    let identity = typed.logical.opaque_id(0).unwrap();
    let entry = typed.entry;
    let call = |callee| NoNormalExpr {
        occurrence: 0,
        result: identity.clone(),
        kind: NoNormalKind::Call(callee, vec![]),
    };
    typed.functions[entry].body = Computation::NoNormal(NoNormalExpr {
        occurrence: 0,
        result: identity.clone(),
        kind: NoNormalKind::NoStorageLet {
            binding: NoStorageBinding {
                domain: identity.domain.clone(),
                occurrence: 0,
            },
            value: Box::new(call(0)),
            body: Box::new(call(entry)),
        },
    });
    let facts = control::IrFacts::build(&typed).unwrap();
    let Computation::NoNormal(body) = &typed.functions[entry].body else {
        unreachable!()
    };
    let NoNormalKind::NoStorageLet {
        value,
        body: continuation,
        ..
    } = &body.kind
    else {
        unreachable!()
    };
    assert_eq!(facts.function_may_return(entry), Some(false));
    assert_eq!(facts.no_normal_may_return(value), Some(true));
    assert_eq!(facts.no_normal_may_return(continuation), Some(false));
}

#[test]
fn ir_no_normal_if_retains_literal_false_base_branch() {
    use crate::typed::{Computation, NoNormalExpr, NoNormalKind};
    let mut typed =
        lower("type Flag<a> = Off | On fn returning() { Off } fn main() { returning() }");
    let identity = typed.logical.opaque_id(0).unwrap();
    let entry = typed.entry;
    let call = |callee| NoNormalExpr {
        occurrence: 0,
        result: identity.clone(),
        kind: NoNormalKind::Call(callee, vec![]),
    };
    typed.functions[entry].body = Computation::NoNormal(NoNormalExpr {
        occurrence: 0,
        result: identity.clone(),
        kind: NoNormalKind::If {
            condition: Box::new(ExprNode {
                ty: Type::Bool,
                kind: ExprKind::Bool(false),
            }),
            yes: Box::new(call(0)),
            no: Box::new(call(entry)),
        },
    });
    assert_eq!(
        control::IrFacts::build(&typed)
            .unwrap()
            .function_may_return(entry),
        Some(true)
    );
}

#[test]
fn new_control_preflight_keeps_unused_native_declarations_but_rejects_reached_native_tails() {
    for source in [
        "fn forever() { forever() } fn main() { let unused = forever(); 42 }",
        "@include(\"stdint.h\"); extern fn unused(x: i32) -> i32; fn forever() { forever() } fn main() { forever() }",
    ] {
        let program = parse(source);
        let checked = crate::check::analyze(&program).unwrap();
        control::SourceFacts::build(&checked).unwrap().check_pure_subset().unwrap();
    }
    for source in [
        "@include(\"stdint.h\"); extern fn reached(x: i32) -> i32; fn forever() { forever() } fn main() { let unused = forever(); reached(42) }",
        "fn forever() { forever() } fn main() { let unused = forever(); u32(1) }",
        "type Flag<a> = Off | On fn forever() { forever() } fn main() { let flag = Off; let unused = forever(); flag }",
    ] {
        let program = parse(source);
        let checked = crate::check::analyze(&program).unwrap();
        let error = control::SourceFacts::build(&checked).unwrap().check_pure_subset().unwrap_err();
        assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}\n{source}");
    }
}

#[test]
fn no_storage_binding_selection_requires_immutable_unused_and_own_initializer_false_flow() {
    let domain = crate::typed::logical::Domain::default();
    for (source, accepted) in [
        (
            "fn forever() { forever() } fn main() { let unused = forever(); 42 }",
            true,
        ),
        (
            "fn forever() { forever() } fn main() { let used = forever(); used }",
            false,
        ),
        (
            "fn forever() { forever() } fn main() { let mut unused = forever(); 42 }",
            false,
        ),
        (
            "fn forever() { forever() } fn main() { let unused = 42; forever() }",
            false,
        ),
    ] {
        let program = parse(source);
        let checked = crate::check::analyze(&program).unwrap();
        let facts = control::SourceFacts::build(&checked).unwrap();
        let main = body(&program, "main");
        assert_eq!(
            facts.no_storage_binding(main, &domain).is_ok(),
            accepted,
            "{source}"
        );
    }
}
