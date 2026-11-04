use rill::{backend, check, interpreter, parser};

fn rejects_at_each_entry_point(source: &str, expected: &str) {
    let program = parser::parse(source).unwrap();
    // Each call reconstructs the checker's hashed tables. The private unit
    // regression also forces an opposing hash order, without depending on luck.
    for _ in 0..3 {
        assert_eq!(
            check::analyze(&program).err().unwrap(),
            expected,
            "{source}"
        );
        assert_eq!(check::check(&program).unwrap_err(), expected, "{source}");
        assert_eq!(
            interpreter::run(&program).unwrap_err(),
            expected,
            "{source}"
        );
        assert_eq!(
            interpreter::run_reference(&program).unwrap_err(),
            expected,
            "{source}"
        );
        assert_eq!(backend::emit(&program).unwrap_err(), expected, "{source}");
        assert_eq!(
            backend::emit_reference(&program).unwrap_err(),
            expected,
            "{source}"
        );
        assert_eq!(rill::compile(source).unwrap_err(), expected, "{source}");
    }
}

#[test]
fn mutual_cycle_witness_follows_declaration_order_at_every_entry_point() {
    let declarations = [
        "type Zebra = Zebra(Alpha)",
        "type Alpha = Alpha(Middle)",
        "type Middle = Middle(Zebra)",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut source = order
            .iter()
            .map(|&index| declarations[index])
            .collect::<Vec<_>>()
            .join("\n");
        source.push_str("\nfn main() { 42 }");
        let first = ["Zebra", "Alpha", "Middle"][order[0]];
        rejects_at_each_entry_point(
            &source,
            &format!("E-TYPE: recursive type '{first}' requires indirection"),
        );
    }
}

#[test]
fn generic_cycle_retains_the_same_error_and_source_order() {
    for (source, first) in [
        (
            "type A<a> = A(B<a>)\ntype B<a> = B(A<a>)\nfn main() { 42 }",
            "A",
        ),
        (
            "type B<a> = B(A<a>)\ntype A<a> = A(B<a>)\nfn main() { 42 }",
            "B",
        ),
        (
            "type Flag<a> = Off | On\ntype Loop<a> = Loop(Flag<Loop<a>>)\nfn main() { 42 }",
            "Loop",
        ),
    ] {
        rejects_at_each_entry_point(
            source,
            &format!("E-TYPE: recursive type '{first}' requires indirection"),
        );
    }
}

#[test]
fn first_cyclic_declaration_wins_over_other_cycles_but_not_acyclic_dependents() {
    for source in [
        "type Entry = Entry(Zebra)\ntype Zebra = Zebra(Alpha)\ntype Alpha = Alpha(Zebra)\ntype Solo = Solo(Solo)\nfn main() { 42 }",
        "type Leaf = Leaf(i32)\ntype Zebra = Zebra(Zebra)\ntype Alpha = Alpha(Alpha)\nfn main() { 42 }",
        "type Entry = { value: Zebra }\ntype Zebra = { value: Alpha }\ntype Alpha = { value: Zebra }\nfn main() { 42 }",
    ] {
        rejects_at_each_entry_point(
            source,
            "E-TYPE: recursive type 'Zebra' requires indirection",
        );
    }
}

#[test]
fn declaration_errors_still_precede_cycle_diagnostics() {
    for (suffix, expected) in [
        (
            "type Zebra = Other",
            "E-NAME: duplicate or reserved type 'Zebra'",
        ),
        (
            "type Other = Zebra",
            "E-NAME: duplicate constructor 'Zebra'",
        ),
        (
            "type Other = Other(Missing)",
            "E-TYPE: unknown type 'Missing'",
        ),
        (
            "type Other<a> = Other(a)\ntype Bad = Bad(Other)",
            "E-ARITY: type 'Other' expects 1 arguments, got 0",
        ),
    ] {
        rejects_at_each_entry_point(
            &format!("type Zebra = Zebra(Zebra)\n{suffix}\nfn main() {{ 42 }}"),
            expected,
        );
    }
}

#[test]
fn cycle_diagnostics_still_precede_function_validation() {
    for suffix in [
        "",
        "fn main(x) { x }",
        "fn main() { missing }",
        "fn main() { 42 }\nfn main() { 0 }",
    ] {
        rejects_at_each_entry_point(
            &format!("type Zebra = Zebra(Alpha)\ntype Alpha = Alpha(Zebra)\n{suffix}"),
            "E-TYPE: recursive type 'Zebra' requires indirection",
        );
    }
}

#[test]
fn finite_forward_dependencies_and_shadowed_type_parameters_remain_valid() {
    for source in [
        "type Outer = Outer(Inner)\ntype Inner = Inner(i32)\nfn main() { Outer(Inner(42)) }",
        "type A = A(i32)\ntype Wrap<A> = Wrap(A)\nfn main() { Wrap(42) }",
        "type Pair<a> = Pair(a, a)\ntype Outer = Outer(Pair<Pair<i32>>)\nfn main() { 42 }",
        "fn main() { 42 }",
    ] {
        let program = parser::parse(source).unwrap();
        check::check(&program).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}
