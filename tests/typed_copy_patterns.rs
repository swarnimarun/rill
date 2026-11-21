use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: Option<&str>) {
    let program = rill::parser::parse(source).unwrap();
    let checked = rill::check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    let reference = rill::interpreter::run_reference(&program);
    match expected {
        Some(expected) => assert_eq!(
            reference.as_ref().unwrap().to_string(),
            expected,
            "{source}"
        ),
        None => assert!(reference.is_err(), "{source}"),
    }
    assert_eq!(rill::typed::evaluate(&typed), reference, "{source}");
    assert_eq!(rill::interpreter::run(&program), reference, "{source}");
    let c = rill::typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(&program).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    let dir = std::env::temp_dir().join(format!(
        "rill-typed-copy-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let source_path = dir.join("main.c");
    let binary = dir.join("main");
    fs::write(&source_path, c).unwrap();
    for opt in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", "-pedantic-errors", opt])
            .arg(&source_path)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        match expected {
            Some(expected) => {
                assert!(
                    output.status.success(),
                    "{source}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    output.stdout,
                    format!("{expected}\n").as_bytes(),
                    "{source}"
                );
                assert!(output.stderr.is_empty());
            }
            None => {
                assert!(!output.status.success(), "{source}");
                assert_eq!(output.stderr, b"rill: runtime trap\n", "{source}");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nested_patterns_preserve_first_match_and_binding_identity() {
    for (source, expected) in [
        ("fn main() { match 42 { 42 => 7 | _ => 0 } }", "7"),
        ("fn main() { match -2147483648 { -2147483648 => 42 | _ => 0 } }", "42"),
        ("fn main() { match true { true => 42 | false => 0 } }", "42"),
        ("fn main() { match () { () => 42 } }", "42"),
        ("fn main() { match 42 { _ => 42 | 42 => 1 / 0 } }", "42"),
        ("fn main() { match 42 { 42 => 42 | 42 => 1 / 0 | _ => 0 } }", "42"),
        ("type Option<a> = None | Some(a)\nfn main() { match Some(Some(42)) { Some(Some(x)) => x | Some(None) => 1 | None => 0 } }", "42"),
        ("type Option<a> = None | Some(a)\nfn main() { match Some(Some(42)) { Some(Some(1)) => 0 | Some(Some(x)) => x | _ => 1 } }", "42"),
        ("type Pair = Pair(bool, bool)\nfn main() { match Pair(true, false) { Pair(true, true) => 1 | Pair(false, _) => 2 | Pair(true, false) => 42 } }", "42"),
        ("type Option<a> = None | Some(a)\nfn main() { let x = 40; let y = match Some(Some(2)) { Some(Some(x)) => x | _ => 0 }; x + y }", "42"),
        ("type Option<a> = None | Some(a)\nfn main() { let old = Some(40); let saved = match old { Some(_) => old | None => Some(0) }; match saved { Some(x) => x + 2 | None => 0 } }", "42"),
        ("type Pair = Pair(i32, i32)\nfn main() { let mut n = 0; let p = Pair({ n <- n + 1; n }, { n <- n + 1; n }); match p { Pair(a, b) => a * 10 + b } }", "12"),
        ("fn main() { let mut n = 0; let value = match { n <- n + 1; n } { 0 => 100 | 1 => 40 | _ => 0 }; value + n + n }", "42"),
        ("type Answer = Answer(i32)\nfn answer(a: Answer) { match a { Answer(n) => Answer(n + 1) } }\nfn main() { answer(Answer(41)) }", "Answer(42)"),
        ("type Color = Red | Green | Blue\nfn main() { Green }", "Green"),
        ("type Text = Text(str) | Empty\nfn main() { Text(\"hé\\0x\") }", "Text(hé\0x)"),
        ("type Option<a> = None | Some(a)\nfn main() { let mut b = bytes_new(3); let old = Some(b); b <- bytes_set(b, 1, 42); match old { Some(x) => bytes_get(x, 1) + bytes_get(b, 1) | None => 0 } }", "42"),
        ("fn main() { match \"hé\\0x\" { s => byte_len(s) } }", "5"),
        ("fn main() { match bytes_new(0) { b => bytes_len(b) } }", "0"),
        ("type N = N(i32)\nfn count(n: N) { match n { N(0) => 42 | N(x) => count(N(x - 1)) } }\nfn main() { count(N(15)) }", "42"),
    ] { compare(source, Some(expected)); }
}

#[test]
fn only_reached_scrutinee_constructor_fields_and_arms_trap() {
    for source in [
        "fn main() { match 1 / 0 { _ => 42 } }",
        "fn main() { match false { true => 42 | false => 1 / 0 } }",
        "type Pair = Pair(i32, i32)\nfn main() { match Pair(1 / 0, 42) { _ => 42 } }",
        "type N = N(i32)\nfn count(n: N) { match n { N(0) => 42 | N(x) => count(N(x - 1)) } }\nfn main() { count(N(31)) }",
    ] { compare(source, None); }
    compare("type Option<a> = None | Some(a)\nfn main() { match Some(Some(42)) { Some(None) => 1 / 0 | Some(Some(x)) => x | None => 1 / 0 } }", Some("42"));
}

#[test]
fn typed_c_emits_compact_nominal_layouts_without_empty_c_aggregates() {
    let c = rill::compile("type Color = Red | Green\nfn main() { Red }").unwrap();
    assert!(c.contains("typedef struct { int tag; } rl_adt_0;"));
    assert!(!c.contains("union {"));
    let source = "type Pair<a> = Pair(a, a)\nfn main() { let p = Pair(1, 2); let q = Pair(true, false); match p { Pair(x, y) => if match q { Pair(a, b) => a && b } { 0 } else { x + y } } }";
    compare(source, Some("3"));
}

#[test]
fn reference_oracle_rejects_owners_and_checks_source_first() {
    let program =
        rill::parser::parse("fn main() { let b = system_box_i32_abort(42); box_read(b) }").unwrap();
    assert!(rill::interpreter::run_reference(&program)
        .unwrap_err()
        .contains("E-REFERENCE"));
    let program = rill::parser::parse("fn main() { true + 1 }").unwrap();
    assert!(rill::interpreter::run_reference(&program)
        .unwrap_err()
        .contains("E-TYPE"));
}

#[test]
fn compact_values_avoid_program_wide_capacity_inflation() {
    let mut source = String::from("type Pair<a> = Pair(a, a)\nfn main() { ");
    for i in 0..10 {
        let previous = if i == 0 {
            "0".to_string()
        } else {
            format!("v{}", i - 1)
        };
        source.push_str(&format!("let v{i} = Pair({previous}, {previous}); "));
    }
    source.push_str("0 }");
    compare(&source, Some("0"));
}
