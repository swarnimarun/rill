fn checked(source: &str) -> Result<(), String> {
    rill::check::check(&rill::parser::parse(source).unwrap())
}
#[test]
fn borrowed_owner_parameters_are_scoped_and_nonowning() {
    for source in [
        "let peek (read b) = box_read(b)\nlet main = let b = system_box_i32_abort(42) in peek(b)",
        "let sum (read a) (read b) = box_read(a) + box_read(b)\nlet main = let b = system_box_i32_abort(21) in sum(b, b)",
        "let peek (read b) = box_read(b)\nlet inc (inout b) = let n = peek(b) in box_write(inout b, n + 1)\nlet main = let mut b = system_box_i32_abort(41) in let done = inc(inout b) in box_read(b)",
    ] {assert!(checked(source).is_ok(),"{source}: {:?}",checked(source));}
    for source in [
        "let consume b = box_read(b)\nlet bad (read b) = consume(move b)\nlet main = let b = system_box_i32_abort(1) in bad(b)",
        "let consume b = box_read(b)\nlet bad (inout b) = consume(move b)\nlet main = let mut b = system_box_i32_abort(1) in bad(inout b)",
        "let bad (read b) = let c = move b in box_read(c)\nlet main = let b = system_box_i32_abort(1) in bad(b)",
        "let bad (read b) = box_write(inout b, 2)\nlet main = let b = system_box_i32_abort(1) in bad(b)",
        "let peek (read b) = box_read(b)\nlet main = peek(system_box_i32_abort(1))",
        "let both (read a) (inout b) = box_read(a)\nlet main = let mut b = system_box_i32_abort(1) in both(b, inout b)",
        "let both (inout a) n = n\nlet main = let mut b = system_box_i32_abort(1) in both(inout b, box_read(b))",
        "let inc (inout b) = box_write(inout b, 2)\nlet main = let b = system_box_i32_abort(1) in inc(inout b)",
    ] {assert!(checked(source).is_err(),"unexpectedly accepted: {source}");}
}

#[test]
fn owner_loan_order_and_liveness_are_checked() {
    for source in [
        // A shared borrow ends after the call, so a later move is valid.
        "let peek (read x) = box_read(x)\nlet consume x = box_read(x)\nlet main = let x = system_box_i32_abort(42) in let n = peek(x) in consume(move x)",
        // A value read before an exclusive argument is also valid.
        "let set n (inout x) = box_write(inout x, n)\nlet main = let mut x = system_box_i32_abort(42) in set(box_read(x), inout x)",
    ] {
        assert!(checked(source).is_ok(), "{source}: {:?}", checked(source));
    }
    for source in [
        "let peek (read x) = box_read(x)\nlet main = let x = system_box_i32_abort(1) in let y = move x in peek(x)",
        "let set (inout x) (inout y) = box_write(inout x, 1)\nlet main = let mut x = system_box_i32_abort(0) in set(inout x, inout x)",
        "let consume x = box_read(x)\nlet f (inout x) n = n\nlet main = let mut x = system_box_i32_abort(1) in f(inout x, consume(move x))",
    ] {
        assert!(checked(source).is_err(), "unexpectedly accepted: {source}");
    }
}
