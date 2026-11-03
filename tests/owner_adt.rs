use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn traced(source: &str, expected: Option<i32>, trace: &str) {
    compare(source, expected.unwrap(), expected, trace);
}

fn compare(source: &str, interpreter_value: i32, expected: Option<i32>, trace: &str) {
    let p = rill::parser::parse(source).unwrap();
    let checked = rill::check::analyze(&p).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    rill::typed::verify(&typed).unwrap();
    let actual = rill::interpreter::run(&p);
    assert_eq!(actual.unwrap().to_string(), interpreter_value.to_string());
    assert_eq!(
        rill::typed::evaluate(&typed).unwrap().to_string(),
        interpreter_value.to_string()
    );
    // Allocator failure is injected only into C, separately from interpreter values.
    let dir = std::env::temp_dir().join(format!(
        "rill-owner-adt-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let fixture = dir.join("fixture.c");
    let object = dir.join("program.o");
    let binary = dir.join("program");
    let emitted = rill::backend::emit(&p).unwrap();
    assert_eq!(emitted, rill::typed::emit_c(&typed).unwrap());
    fs::write(&c, emitted).unwrap();
    fs::write(
        &fixture,
        r#"#include <stdio.h>
#include <stdlib.h>
static void *allocated[32];
static int count;
void *fixture_malloc(size_t size) {
    int n = ++count;
    if (getenv("RILL_FAIL_SECOND") && n == 2) { fputs("A2-fail\n", stderr); return NULL; }
    void *p = malloc(size);
    if (!p) return NULL;
    allocated[n] = p;
    fprintf(stderr, "A%d\n", n);
    return p;
}
void fixture_free(void *p) {
    int n = 1;
    while (n <= count && allocated[n] != p) ++n;
    fprintf(stderr, "F%d\n", n);
    allocated[n] = NULL;
    free(p);
}
int rl_generated_main(void);
int main(void) { return rl_generated_main(); }
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args([
                "-std=c11",
                opt,
                "-Dmain=rl_generated_main",
                "-Dmalloc=fixture_malloc",
                "-Dfree=fixture_free",
                "-c",
            ])
            .arg(&c)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let link = Command::new("cc")
            .arg(&object)
            .arg(&fixture)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let mut command = Command::new(&binary);
        if expected.is_none() {
            command.env("RILL_FAIL_SECOND", "1");
        }
        let result = command.output().unwrap();
        match expected {
            Some(value) => {
                assert!(result.status.success());
                assert_eq!(result.stdout, format!("{value}\n").as_bytes());
                assert_eq!(result.stderr, trace.as_bytes());
            }
            None => {
                assert!(!result.status.success());
                assert!(result.stdout.is_empty());
                let err = String::from_utf8(result.stderr).unwrap();
                assert!(err.starts_with(trace), "{err}");
                assert!(err.contains("rill: runtime trap"), "{err}");
                assert!(
                    !err.lines().any(|line| line.starts_with('F')),
                    "abort unexpectedly cleaned up: {err}"
                );
                assert!(!err.contains("A2\n"), "callee should not run: {err}");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ordinary_result_transfers_success_and_preserves_error() {
    let source = include_str!("../examples/owned/result.rill");
    traced(source, Some(42), "A1\nF1\n");
    traced(
        "type Result<a> = Ok(a) | Err(i32)\nlet make n = if n > 0 then (let owner = system_box_i32_abort(n) in Ok(move owner)) else Err(42)\nlet main = let result = make(0) in match move result { Ok(owner) => box_read(owner) | Err(code) => code + code - 42 }",
        Some(42),
        "",
    );
}

#[test]
fn three_variant_nested_sums_drop_fields_and_locals_in_reverse_order() {
    traced(
        "type Inner<a> = Empty | One(a) | Pair(a, a, i32)\ntype Outer<a> = Absent | Wrapped(Inner<a>, a) | Code(i32)\nlet main = let a = system_box_i32_abort(10) in let b = system_box_i32_abort(20) in let c = system_box_i32_abort(30) in let value = Wrapped(Pair(move a, move b, 42), move c) in match move value { Absent => 0 | Wrapped(inner, _) => (let scratch = system_box_i32_abort(99) in match move inner { Empty => 0 | One(owner) => box_read(owner) | Pair(_, _, n) => n + n - 42 }) | Code(n) => n }",
        Some(42),
        "A1\nA2\nA3\nA4\nF2\nF1\nF4\nF3\n",
    );
}

#[test]
fn every_variant_and_whole_value_patterns_preserve_ownership() {
    let prefix = "type Choice<a> = Empty | One(a) | Pair(a, a, i32)\nlet make n = if n == 0 then Empty else if n == 1 then (let a = system_box_i32_abort(42) in One(move a)) else (let a = system_box_i32_abort(20) in let b = system_box_i32_abort(22) in Pair(move a, move b, 0))\n";
    let body = " in match move value { Empty => 42 | One(a) => box_read(a) | Pair(a, b, n) => box_read(a) + box_read(b) + n }";
    for (n, trace) in [(0, ""), (1, "A1\nF1\n"), (2, "A1\nA2\nF2\nF1\n")] {
        traced(
            &format!("{prefix}let main = let value = make({n}){body}"),
            Some(42),
            trace,
        );
    }
    traced(
        "type Wrap<a> = Wrap(a) | Empty\nlet main = let a = system_box_i32_abort(42) in let value = Wrap(move a) in match move value { whole => (match move whole { Wrap(owner) => box_read(owner) | Empty => 0 }) }",
        Some(42),
        "A1\nF1\n",
    );
    traced(
        "type Wrap<a> = Wrap(a) | Empty\nlet main = let a = system_box_i32_abort(42) in let value = Wrap(move a) in match move value { _ => 42 }",
        Some(42),
        "A1\nF1\n",
    );
}

#[test]
fn unconsumed_nested_aggregate_drops_without_hidden_allocations() {
    traced(
        "type Inner<a> = Empty | Pair(a, a) | Number(i32)\ntype Outer<a> = No | Some(Inner<a>, a) | Other(i32)\nlet main = let a = system_box_i32_abort(1) in let b = system_box_i32_abort(2) in let c = system_box_i32_abort(3) in let unused = Some(Pair(move a, move b), move c) in 42",
        Some(42),
        "A1\nA2\nA3\nF3\nF2\nF1\n",
    );
}

#[test]
fn match_returns_owner_payload_and_owner_result() {
    traced(
        "type Result<a> = Ok(a) | Err(i32)\nlet unwrap n = let owner = system_box_i32_abort(n) in let result = Ok(move owner) in match move result { Ok(payload) => move payload | Err(code) => (let fallback = system_box_i32_abort(code) in move fallback) }\nlet wrap n = let payload = unwrap(n) in let result = Ok(move payload) in match move result { Ok(owner) => Ok(move owner) | Err(code) => Err(code) }\nlet main = let result = wrap(42) in match move result { Ok(owner) => box_read(owner) | Err(code) => code }",
        Some(42),
        "A1\nF1\n",
    );
}

#[test]
fn bytes_payload_moves_and_drops_once() {
    traced(
        "type Storage<a> = Missing | Present(a, i32) | Failed(i32)\nlet make n = let bytes = system_bytes_zeroed_abort(n) in Present(move bytes, 42)\nlet main = let storage = make(2) in match move storage { Missing => 0 | Present(mutbytes, n) => owned_bytes_len(mutbytes) + n - 2 | Failed(code) => code }",
        Some(42),
        "A1\nF1\n",
    );
}

#[test]
fn second_payload_allocation_failure_aborts_without_cleanup() {
    compare(
        "type Pair<a> = Both(a, a) | Neither\nlet main = let first = system_box_i32_abort(1) in let second = system_box_i32_abort(2) in let value = Both(move first, move second) in 42",
        42,
        None,
        "A1\nA2-fail\n",
    );
}

#[test]
fn owner_sums_reject_affine_and_layout_violations_at_every_entry_point() {
    let cases = [
        // Construction must move owner fields explicitly.
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(owner) in 0",
        // Aggregates themselves cannot be copied.
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(move owner) in let copy = value in 0",
        // Consuming an aggregate invalidates its old root.
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(move owner) in let n = match move value { Wrap(x) => box_read(x) } in match move value { Wrap(x) => box_read(x) }",
        // Two fields cannot consume the same owner.
        "type Pair<a> = Pair(a, a)\nlet main = let owner = system_box_i32_abort(42) in let value = Pair(move owner, move owner) in 0",
        // Borrowed roots cannot be moved or decomposed into escaping owners.
        "type Wrap<a> = Wrap(a)\nlet escape (read value) = match move value { Wrap(x) => (let n = box_read(x) in move x) }\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(move owner) in let escaped = escape(value) in box_read(escaped)",
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(move owner) in match value { Wrap(x) => box_read(x) }",
        // Joins cannot leave an outer owner moved on just one branch.
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = if true then Wrap(move owner) else (let other = system_box_i32_abort(1) in Wrap(move other)) in 0",
        "type Wrap<a> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in Wrap(move owner)",
        // An unused type parameter still requires a concrete owner instance.
        "type Wrap<a, b> = Wrap(a)\nlet main = let owner = system_box_i32_abort(42) in let value = Wrap(move owner) in 0",
        // Inline recursive layouts require an indirection unsupported here.
        "type Chain<a> = End(a) | Link(Chain<a>)\nlet main = let owner = system_box_i32_abort(42) in let value = End(move owner) in 0",
        // Ownership does not weaken nominal identity or exhaustiveness.
        "type First<a> = First(a)\ntype Second<a> = Second(a)\nlet main = let owner = system_box_i32_abort(42) in let value = First(move owner) in match move value { Second(x) => box_read(x) }",
        "type Result<a> = Ok(a) | Err(i32)\nlet main = let owner = system_box_i32_abort(42) in let value = Ok(move owner) in match move value { Ok(x) => box_read(x) }",
    ];
    for source in cases {
        let program = rill::parser::parse(source).unwrap();
        assert!(rill::check::check(&program).is_err(), "accepted {source}");
        assert!(
            rill::interpreter::run(&program).is_err(),
            "executed {source}"
        );
        assert!(rill::backend::emit(&program).is_err(), "emitted {source}");
    }
}

#[test]
fn equivalent_outer_owner_moves_and_copy_sum_fields_are_supported() {
    traced(
        "type Wrap<a> = Wrap(a)\nlet choose yes = let owner = system_box_i32_abort(42) in if yes then Wrap(move owner) else Wrap(move owner)\nlet main = let value = choose(false) in match move value { Wrap(owner) => box_read(owner) }",
        Some(42),
        "A1\nF1\n",
    );
    traced(
        "type Count = Zero | Count(i32)\ntype Wrap<a> = Wrap(a, Count) | Empty\nlet main = let owner = system_box_i32_abort(40) in let value = Wrap(move owner, Count(1)) in match move value { Wrap(owner, count) => box_read(owner) + (match count { Zero => 0 | Count(n) => n }) + (match count { Zero => 0 | Count(n) => n }) | Empty => 0 }",
        Some(42),
        "A1\nF1\n",
    );
}

#[test]
fn phantom_owner_arguments_emit_complete_layout_runtime_without_allocating() {
    for owner in ["SystemBoxI32", "SystemBytes"] {
        traced(
            &format!("type Owned = Own({owner})\ntype Phantom<a> = P\nlet inspect (x: Phantom<Owned>) = 42\nlet main = inspect(P)"),
            Some(42),
            "",
        );
    }
}

#[test]
fn lexical_owner_initializers_preserve_nested_cleanup() {
    traced(
        "type Wrap<a> = W(a)\nlet main = let wrapped = let item = system_box_i32_abort(42) in W(move item) in match move wrapped { W(item) => box_read(item) }",
        Some(42),
        "A1\nF1\n",
    );
    traced(
        "type Wrap<a> = W(a)\nlet main = let wrapped = let mut item = system_box_i32_abort(1) in let scratch = system_box_i32_abort(7) in let updated = box_write(inout item, 42) in W(move item) in match move wrapped { W(item) => box_read(item) }",
        Some(42),
        "A1\nA2\nF2\nF1\n",
    );
}

#[test]
fn direct_box_moves_in_both_branches_transfer_once() {
    for condition in ["true", "false"] {
        traced(
            &format!("let main = let x = system_box_i32_abort(42) in let y = if {condition} then move x else move x in box_read(y)"),
            Some(42),
            "A1\nF1\n",
        );
    }
}

#[test]
fn copy_patterns_inside_owner_programs_preserve_moves_and_cleanup() {
    for flag in ["true", "false"] {
        traced(
            &format!("fn main() {{ let first = system_box_i32_abort(1); let second = system_box_i32_abort(42); let result = match {flag} {{ true => move second | false => move second }}; box_read(result) }}"),
            Some(42),
            "A1\nA2\nF2\nF1\n",
        );
        traced(
            &format!("type Option<a> = None | Some(a)\nfn main() {{ let owner = system_box_i32_abort(42); let input = Some(Some({flag})); let result = match input {{ Some(Some(true)) => move owner | Some(Some(false)) => move owner | Some(None) => move owner | None => move owner }}; box_read(result) }}"),
            Some(42),
            "A1\nF1\n",
        );
    }
    traced(
        "fn main() { let owner = system_box_i32_abort(40); let value = match 2 { owner => owner }; value + box_read(owner) }",
        Some(42),
        "A1\nF1\n",
    );
    traced(
        "fn make(flag) { match flag { true => { let b = system_box_i32_abort(42); move b } | false => { let b = system_box_i32_abort(0); move b } } }\nfn main() { let b = make(true); box_read(b) }",
        Some(42),
        "A1\nF1\n",
    );
    traced(
        "fn main() { let b = system_box_i32_abort(42); match 7 { _ => box_read(b) | 7 => { let skipped = system_box_i32_abort(0); box_read(skipped) } } }",
        Some(42),
        "A1\nF1\n",
    );
}
