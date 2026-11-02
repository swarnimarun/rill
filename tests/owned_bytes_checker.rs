fn check(source: &str) -> Result<(), String> {
    rill::check::check(&rill::parser::parse(source).unwrap())
}

#[test]
fn owned_bytes_are_affine_and_borrowed_in_argument_order() {
    for source in [
        "let main = let b = system_bytes_zeroed_abort(10) in owned_bytes_len(b)",
        "let main = let mut b = system_bytes_zeroed_abort(10) in let done = owned_bytes_set(inout b, 2, 42) in owned_bytes_get(b, 2)",
        "let get (read b) = owned_bytes_get(b, 0)\nlet set (inout b) = owned_bytes_set(inout b, 0, 42)\nlet main = let mut b = system_bytes_zeroed_abort(1) in let done = set(inout b) in get(b)",
        "let consume b = owned_bytes_len(b)\nlet main = let b = system_bytes_zeroed_abort(1) in consume(move b)",
    ] {
        assert!(check(source).is_ok(), "{source}: {:?}", check(source));
    }
    for source in [
        "let main = let b = system_bytes_zeroed_abort(1) in let c = b in owned_bytes_len(c)",
        "let main = let b = system_bytes_zeroed_abort(1) in let c = move b in owned_bytes_len(b)",
        "let main = let b = system_bytes_zeroed_abort(1) in owned_bytes_set(inout b, 0, 1)",
        "let main = let mut b = system_bytes_zeroed_abort(1) in owned_bytes_set(inout b, owned_bytes_len(b), 1)",
        "let main = let mut b = system_bytes_zeroed_abort(1) in owned_bytes_get(b, (let done = owned_bytes_set(inout b, 0, 1) in 0))",
        "let main = let b = system_bytes_zeroed_abort(1) in box_read(b)",
        "let main = let b = system_box_i32_abort(1) in owned_bytes_len(b)",
        "let main = owned_bytes_len(system_bytes_zeroed_abort(1))",
        "let main = let n = 1 in let bad = move n in bad",
    ] {
        assert!(check(source).is_err(), "unexpectedly accepted: {source}");
    }
}
