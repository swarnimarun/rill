use rill::{ast, check, parser, typed};
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const COMPLEXITY: &str =
    "E-MATCH-COMPLEXITY: pattern analysis exceeded 100000 work units; simplify the match";
fn checked(source: &str) -> Result<(), String> {
    check::check(&parser::parse(source)?)
}

// The deadline only contains a regressed compiler. Actual test expectations are
// semantic outcomes or exact deterministic diagnostics, never timings.
fn in_child(name: &str) -> bool {
    const CHILD: &str = "RILL_COVERAGE_CHILD";
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        return true;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(CHILD, name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "child failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return false;
        }
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("coverage exceeded subprocess safety deadline: {output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn catchalls_do_not_enumerate_nested_boolean_products() {
    if !in_child("catchalls_do_not_enumerate_nested_boolean_products") {
        return;
    }
    for depth in [5, 10, 24] {
        for pattern in ["_", "bound"] {
            let ty = format!("{}bool{}", "Pair<".repeat(depth), ">".repeat(depth));
            checked(&format!("type Pair<a> = Pair(a, a)\nfn ignored(x: {ty}) {{ match x {{ {pattern} => 42 }} }}\nfn main() {{ 42 }}")).unwrap();
        }
    }
    let mut source =
        "type Pair<a> = Pair(a, a)\nfn main() { let p0 = Pair(true, false); ".to_owned();
    for i in 1..5 {
        source.push_str(&format!("let p{i} = Pair(p{}, p{}); ", i - 1, i - 1));
    }
    source.push_str("match p4 { _ => 42 } }");
    let program = parser::parse(&source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let lowered = typed::lower_main(&checked).unwrap();
    typed::verify(&lowered).unwrap();
    assert_eq!(typed::evaluate(&lowered).unwrap().to_string(), "42");
}

#[test]
fn wide_patterns_have_a_deterministic_complexity_diagnostic() {
    if !in_child("wide_patterns_have_a_deterministic_complexity_diagnostic") {
        return;
    }
    let types = vec!["unit"; 1024].join(", ");
    let fields = vec!["()"; 1024].join(", ");
    let source = format!("type W = W({types})\nfn ignored(x: W) {{ match x {{ W({fields}) => 42 }} }}\nfn main() {{ 42 }}");
    for _ in 0..3 {
        assert_eq!(checked(&source).unwrap_err(), COMPLEXITY);
    }
    checked(&source.replace(&format!("W({fields}) =>"), "_ =>")).unwrap();
    checked(&source.replace(
        &format!("W({fields}) =>"),
        &format!("W({}) =>", vec!["_"; 1024].join(", ")),
    ))
    .unwrap();
}

#[test]
fn pigeonhole_coverage_stops_without_claiming_a_missing_case() {
    if !in_child("pigeonhole_coverage_stops_without_claiming_a_missing_case") {
        return;
    }
    // The disjunction of all violated pigeonhole constraints is exhaustive,
    // but a five-pigeon/four-hole product proof is combinatorial.
    let (pigeons, holes) = (5, 4);
    let width = pigeons * holes;
    let mut patterns = Vec::new();
    for pigeon in 0..pigeons {
        let mut fields = vec!["_"; width];
        fields[pigeon * holes..(pigeon + 1) * holes].fill("false");
        patterns.push(format!("P({}) => 42", fields.join(", ")));
    }
    for first in 0..pigeons {
        for second in first + 1..pigeons {
            for hole in 0..holes {
                let mut fields = vec!["_"; width];
                fields[first * holes + hole] = "true";
                fields[second * holes + hole] = "true";
                patterns.push(format!("P({}) => 42", fields.join(", ")));
            }
        }
    }
    let source = format!(
        "type P = P({})\nfn ignored(x: P) {{ match x {{ {} }} }}\nfn main() {{ 42 }}",
        vec!["bool"; width].join(", "),
        patterns.join(" | ")
    );
    assert_eq!(checked(&source).unwrap_err(), COMPLEXITY);
    checked(&source.replace(" => 42 }", " => 42 | _ => 42 }")).unwrap();
}

#[test]
fn every_boolean_product_subset_matches_its_truth_table() {
    let base =
        parser::parse("type P = P(bool, bool)\nfn main() { match P(true, false) { _ => 42 } }")
            .unwrap();
    for mask in 0..512 {
        let mut program = base.clone();
        let ast::Expr::Match(_, arms) = &mut program.functions[0].body else {
            panic!()
        };
        arms.clear();
        let mut covered = 0u8;
        for row in 0..9 {
            if mask & (1 << row) == 0 {
                continue;
            }
            let (a, b) = (row / 3, row % 3);
            let field = |i| match i {
                0 => ast::Pattern::Wildcard,
                1 => ast::Pattern::Bool(false),
                _ => ast::Pattern::Bool(true),
            };
            arms.push(ast::MatchArm {
                pattern: ast::Pattern::Constructor("P".into(), vec![field(a), field(b)]),
                body: ast::Expr::Int(42),
            });
            for va in 0..2 {
                for vb in 0..2 {
                    if (a == 0 || a == va + 1) && (b == 0 || b == vb + 1) {
                        covered |= 1 << (va * 2 + vb);
                    }
                }
            }
        }
        assert_eq!(check::check(&program).is_ok(), covered == 15, "mask {mask}");
    }
}

#[test]
fn nested_and_open_domains_keep_the_existing_rules() {
    for source in [
        "fn f(x) { match x { bound => 42 } }\nfn main() { 42 }",
        "fn main() { match () { () => 42 } }",
        "type T = A | B\nfn main() { match A { A => 42 | B => 0 } }",
        "type O<a> = N | S(a)\ntype P = P(O<bool>, unit, i32)\nfn f(x: P) { match x { P(N, (), _) => 0 | P(S(false), (), a) => 1 | P(S(true), (), b) => 2 } }\nfn main() { f(P(N, (), 7)) }",
        "type P<a> = P(a, a)\nfn f(x: P<P<bool>>) { match x { P(P(true, _), _) => 1 | P(P(false, x), _) => 2 } }\nfn main() { 42 }",
        "fn main() { match 42 { 1 => 0 | 2 => 0 | x => x } }",
        "type M<bool> = M(bool)\nfn f(x: M<i32>) { match x { M(_) => 42 } }\nfn main() { 42 }",
    ] { checked(source).unwrap_or_else(|e| panic!("{source}: {e}")); }
    for source in [
        "fn main() { match 42 { 42 => 0 } }",
        "fn main() { match false { true => 0 } }",
        "type O<a> = N | S(a)\nfn f(x: O<bool>) { match x { S(true) => 0 | N => 0 } }\nfn main() { 42 }",
        "type P = P(bool, i32)\nfn f(x: P) { match x { P(true, _) => 1 | P(false, 0) => 0 } }\nfn main() { 42 }",
        "type M<bool> = M(bool)\nfn f(x: M<i32>) { match x { M(0) => 42 } }\nfn main() { 42 }",
    ] { assert_eq!(checked(source).unwrap_err(), "E-MATCH: non-exhaustive match", "{source}"); }
}

#[test]
fn impossible_patterns_are_checked_even_after_a_catchall() {
    for (source, diagnostic) in [
        ("fn main() { match true { _ => 42 | 1 => 0 } }", "E-TYPE"),
        (
            "type P = P(bool)\nfn main() { match P(true) { _ => 42 | P() => 0 } }",
            "E-ARITY",
        ),
        (
            "type P = P(bool, bool)\nfn main() { match P(true, false) { _ => 42 | P(x, x) => 0 } }",
            "duplicate pattern binding",
        ),
        (
            "fn main() { match true { _ => 42 | false => missing } }",
            "E-NAME",
        ),
        ("type R = R(R)\nfn main() { 42 }", "recursive type"),
    ] {
        assert!(
            checked(source).unwrap_err().contains(diagnostic),
            "{source}"
        );
    }
    let mut program = parser::parse("type T = A\nfn main() { match A { A => 42 } }").unwrap();
    program.types[0].variants.clear();
    assert!(check::check(&program)
        .unwrap_err()
        .contains("has no constructors"));
    let mut program = parser::parse("fn f(x) { match x { _ => 42 } }\nfn main() { 42 }").unwrap();
    let ast::Expr::Match(_, arms) = &mut program.functions[0].body else {
        panic!()
    };
    arms.clear();
    assert_eq!(
        check::check(&program).unwrap_err(),
        "E-MATCH: non-exhaustive match"
    );
}
