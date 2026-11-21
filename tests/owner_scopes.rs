fn checked(source: &str) -> Result<(), String> {
    rill::check::check(&rill::parser::parse(source).unwrap())
}
#[test]
fn owner_shadowing_and_move_state_follow_lexical_scope() {
    for source in [
        "let main = let b = system_box_i32_abort(3) in let b = move b in box_read(b)",
        "let main = let mut b = system_box_i32_abort(3) in let value = (let b = 4 in b) in let done = box_write(inout b, value) in box_read(b)",
        "let main = let b = system_box_i32_abort(3) in let value = (let b = system_box_i32_abort(4) in box_read(b)) in value + box_read(b)",
    ] { assert!(checked(source).is_ok(), "{source}"); }
    let moved = "let main = let b = system_box_i32_abort(3) in let moved = move b in let value = (let b = 4 in b) in box_read(b)";
    assert!(checked(moved).unwrap_err().contains("E-OWNED"));
    let immutable = "let main = let b = system_box_i32_abort(3) in box_write(inout b, 4)";
    assert!(checked(immutable).unwrap_err().contains("E-LOAN"));
    let conflicting =
        "let main = let mut b = system_box_i32_abort(3) in box_write(inout b, box_read(b))";
    assert!(checked(conflicting).unwrap_err().contains("E-LOAN"));
    let ordered = "let main = let mut b = system_box_i32_abort(3) in let old = box_read(b) in let done = box_write(inout b, old + 1) in box_read(b)";
    assert!(checked(ordered).is_ok());
    let recursive = "let main = let b = system_box_i32_abort(3) in main()";
    assert!(checked(recursive).unwrap_err().contains("E-OWNED"));
    let loop_local_move = "let main = let done = while false do (let b = system_box_i32_abort(1) in let moved = move b in ()) in 0";
    assert!(checked(loop_local_move).is_ok());
}

#[test]
fn owner_calls_reject_cycles_generics_and_implicit_transfer() {
    let read_scalar = "let read_scalar (read x) = x + 1\nlet main = let b = system_box_i32_abort(1) in read_scalar(box_read(b))";
    assert!(checked(read_scalar).is_ok());
    assert_eq!(
        rill::interpreter::run(&rill::parser::parse(read_scalar).unwrap())
            .unwrap()
            .to_string(),
        "2"
    );
    for source in [
        "let recur n = if n == 0 then 0 else recur(n - 1)\nlet main = let b = system_box_i32_abort(1) in let x = recur(2) in box_read(b)",
        "let first n = if n == 0 then 0 else second(n - 1)\nlet second n = if n == 0 then 0 else first(n - 1)\nlet main = let b = system_box_i32_abort(1) in box_read(b)",
        "let identity x = x\nlet main = let b = system_box_i32_abort(1) in let x = identity(2) in box_read(b)",
        "let consume x = box_read(x)\nlet main = let b = system_box_i32_abort(1) in consume(b)",
        "let consume x y = box_read(x) + box_read(y)\nlet main = let b = system_box_i32_abort(1) in consume(move b, move b)",
        "let consume x n = box_read(x) + n\nlet main = let b = system_box_i32_abort(1) in consume(move b, box_read(b))",
    ] {
        let result = checked(source);
        assert!(result.is_err(), "unexpectedly accepted: {source}");
    }
}

#[test]
fn branch_and_loop_local_owners_drop_each_time() {
    use std::{fs, process::Command};
    let source = "let consume b = let moved = move b in box_read(moved)\nlet consume_bytes b = owned_bytes_len(b)\nlet main = let mut i = 0 in let loop = while i < 3 do (let b = system_box_i32_abort(i) in let moved = move b in let n = consume(move moved) in let bytes = system_bytes_zeroed_abort(n + 1) in let moved_bytes = move bytes in let length = consume_bytes(move moved_bytes) in i <- length) in if true then (let b = system_box_i32_abort(i) in let moved = move b in consume(move moved)) else (let b = system_box_i32_abort(0) in consume(move b))";
    let program = rill::parser::parse(source).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "3");
    let dir = std::env::temp_dir().join(format!("rill-owned-scopes-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let generated = dir.join("generated.c");
    let fixture = dir.join("fixture.c");
    let object = dir.join("generated.o");
    let exe = dir.join("program");
    fs::write(&generated, rill::backend::emit(&program).unwrap()).unwrap();
    fs::write(&fixture, "#include <stdio.h>\n#include <stdlib.h>\nvoid *fixture_malloc(size_t n) { fputc('A', stderr); return malloc(n); }\nvoid fixture_free(void *p) { fputc('F', stderr); free(p); }\nint rl_generated_main(void); int main(void) { return rl_generated_main(); }\n").unwrap();
    for opt in ["-O0", "-O2"] {
        let c = Command::new("cc")
            .args([
                "-std=c11",
                opt,
                "-Dmain=rl_generated_main",
                "-Dmalloc=fixture_malloc",
                "-Dfree=fixture_free",
                "-c",
            ])
            .arg(&generated)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        let c = Command::new("cc")
            .arg(&object)
            .arg(&fixture)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        let run = Command::new(&exe).output().unwrap();
        assert!(run.status.success());
        assert_eq!(run.stdout, b"3\n");
        assert_eq!(run.stderr, b"AFAFAFAFAFAFAF");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn conditional_moves_cannot_consume_enclosing_owners() {
    for source in [
        "let main = let b = system_box_i32_abort(1) in if true then (let moved = move b in box_read(moved)) else 0",
        "let consume b = box_read(b)\nlet main = let b = system_box_i32_abort(1) in if true then consume(move b) else 0",
        "let main = let b = system_box_i32_abort(1) in let done = while false do (let moved = move b in ()) in box_read(b)",
        "let main = if true then (let b = system_box_i32_abort(1) in if true then (let moved = move b in box_read(moved)) else 0) else 0",
        "let main = let done = while false do (let b = system_box_i32_abort(1) in let inner = while false do (let moved = move b in ()) in ()) in 0",
        "let consume b = box_read(b) == 1\nlet main = let b = system_box_i32_abort(1) in false && consume(move b)",
    ] {
        assert!(checked(source).unwrap_err().contains("E-OWNED"), "{source}");
    }
}

#[test]
fn conditional_local_moves_preserve_use_after_move_and_borrow_rules() {
    for source in [
        "let main = if true then (let b = system_box_i32_abort(1) in let moved = move b in box_read(b)) else 0",
        "let consume b = box_read(b)\nlet main = if true then (let b = system_box_i32_abort(1) in let n = consume(move b) in consume(move b)) else 0",
        "let consume b = box_read(b)\nlet inspect (read b) = consume(move b)\nlet main = if true then (let b = system_box_i32_abort(1) in inspect(b)) else 0",
    ] {
        assert!(checked(source).is_err(), "{source}");
    }
    for source in [
        "let main = false && (let b = system_box_i32_abort(1) in let moved = move b in box_read(moved) == 1)",
        "let main = if (let b = system_box_i32_abort(1) in let moved = move b in box_read(moved) == 1) then 1 else 0",
        "let main = let mut i = 0 in let done = while (let b = system_box_i32_abort(i) in let moved = move b in box_read(moved) < 2) do (i <- i + 1) in i",
        "let consume b = owned_bytes_len(b)\nlet main = if true then (let b = system_bytes_zeroed_abort(3) in let moved = move b in consume(move moved)) else 0",
        "let main = if true then (let b = system_box_i32_abort(1) in let n = (if true then (let b = system_box_i32_abort(2) in let moved = move b in box_read(moved)) else 0) in box_read(b) + n) else 0",
    ] {
        let program = rill::parser::parse(source).unwrap();
        checked(source).unwrap();
        rill::interpreter::run(&program).unwrap();
        rill::backend::emit(&program).unwrap();
    }
}

#[test]
fn copy_match_arms_still_require_compatible_outer_ownership() {
    for source in [
        "fn main() { let b = system_box_i32_abort(42); match true { true => { let moved = move b; box_read(moved) } | false => 0 } }",
        "type N = N(i32)\nfn main() { let b = system_box_i32_abort(42); match N(7) { N(0) => { let moved = move b; box_read(moved) } | N(n) => n } }",
        "fn main() { let b = system_box_i32_abort(42); let x = match true { true => move b | false => move b }; box_read(b) }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&program).unwrap_err().contains("E-OWNED"), "{source}");
        assert!(rill::interpreter::run(&program).is_err(), "{source}");
        assert!(rill::backend::emit(&program).is_err(), "{source}");
    }
}
