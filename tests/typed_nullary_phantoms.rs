//! Source regressions for the bounded all-nullary phantom representation.
//! These assertions are an acceptance suite, not evidence that it has been run.
use rill::{
    check, interpreter,
    modules::compile::load_program_with_sources,
    parser, typed,
    typed::provenance::{
        lower_main_with_function_origins, nodes::lower_main_with_copy_node_origins,
    },
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const FLAG: &str = "type Flag<a> = Off | On\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-nullary-phantoms-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only files created by this individual test are removed.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compare(source: &str, expected: Option<&str>) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    compare_program(&program, source, expected);
}

fn compare_program(program: &rill::ast::Program, source: &str, expected: Option<&str>) {
    let checked = check::analyze(program).unwrap_or_else(|e| panic!("{source}: {e}"));
    let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    typed::verify(&lowered).unwrap();
    let actual = typed::evaluate(&lowered);
    match expected {
        Some(expected) => assert_eq!(actual.as_ref().unwrap().to_string(), expected, "{source}"),
        None => assert_eq!(actual.as_ref().unwrap_err(), "runtime trap", "{source}"),
    }
    assert_eq!(interpreter::run(program), actual, "{source}");
    // The explicit reference oracle deliberately excludes owner programs.
    if !checked.has_owned() {
        assert_eq!(interpreter::run_reference(program), actual, "{source}");
    }
    let c = typed::emit_c(&lowered).unwrap();
    assert_eq!(rill::backend::emit(program).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert_eq!(
        typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
        c,
        "repeated lowering must preserve deterministic output: {source}"
    );

    let fixture = Fixture::new();
    let c_path = fixture.write("main.c", &c);
    let binary = fixture.0.join("main");
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", "-pedantic-errors", optimization])
            .arg(&c_path)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{source} ({optimization}): {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        match expected {
            Some(expected) => {
                assert!(output.status.success(), "{source} ({optimization})");
                assert_eq!(
                    output.stdout,
                    format!("{expected}\n").as_bytes(),
                    "{source}"
                );
                assert!(output.stderr.is_empty(), "{source}");
            }
            None => {
                assert!(!output.status.success(), "{source} ({optimization})");
                assert!(output.stdout.is_empty(), "{source}");
                assert_eq!(output.stderr, b"rill: runtime trap\n", "{source}");
            }
        }
    }
}

fn rejects_source(source: &str, prefix: &str) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let error = check::analyze(&program).err().unwrap();
    assert!(error.starts_with(prefix), "{source}: {error}");
    assert_eq!(interpreter::run(&program).unwrap_err(), error, "{source}");
    assert_eq!(
        rill::backend::emit(&program).unwrap_err(),
        error,
        "{source}"
    );
}

#[test]
fn unconstrained_observation_and_return_use_tag_only_values() {
    for body in [
        "let f = Off; match f { Off => 42 | On => 0 }",
        "let f = Off(); let alias = f; match alias { Off => 42 | On => 0 }",
        "let Off = 7; let f = Off(); match f { Off => 42 | On => 0 }",
        "let f = if true { Off } else { On }; match f { Off => 42 | On => 0 }",
        "let f = match true { true => Off | false => On }; match f { Off => 42 | On => 0 }",
    ] {
        compare(&format!("{FLAG}fn main() {{ {body} }}"), Some("42"));
    }
    compare("type Token<a> = Token\nfn main() { Token }", Some("Token"));
    compare(
        "type Token<a,b> = Token\nfn make() { Token() }\nfn forward() { make() }\nfn main() { forward() }",
        Some("Token"),
    );
    compare(
        &format!("{FLAG}fn main() {{ if false {{ Off }} else {{ On }} }}"),
        Some("On"),
    );
}

#[test]
fn unrelated_payload_declarations_do_not_change_native_acceptance() {
    for unused in [
        "",
        "type Unused = Extra(i32)\n",
        "type Unused<a> = Extra(a)\ntype Product = { number: i32 }\n",
    ] {
        compare(
            &format!(
                "{FLAG}{unused}fn main() {{ let f = Off; match f {{ Off => 42 | On => 0 }} }}"
            ),
            Some("42"),
        );
    }
}

#[test]
fn weak_mutable_dynamic_tags_keep_effects_and_loop_updates() {
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) {
    count <- count + 1;
    if count == 1 { Off } else { On }
}
fn main() {
    let mut count = 0;
    let mut f = choose(inout count);
    f <- choose(inout count);
    match f { Off => 0 | On => count + 40 }
}"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) {
    count <- count + 1;
    if count == 1 { Off } else { On }
}
fn main() {
    let mut count = 0;
    let mut f = Off;
    while count < 2 { f <- choose(inout count) };
    let saved = f;
    f <- Off;
    match saved { Off => 0 | On => match f { Off => count + 40 | On => 0 } }
}"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) { count <- count + 1; Off }
fn main() { let mut count = 0; let unused = choose(inout count); count + 41 }"#,
        Some("42"),
    );
}

#[test]
fn arguments_and_scrutinees_evaluate_once_in_source_order() {
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) {
    count <- count + 1;
    if count == 1 { Off } else { On }
}
fn pair(a,b) {
    match a { Off => match b { On => 40 | Off => 0 } | On => 0 }
}
fn main() {
    let mut count = 0;
    let answer = pair(choose(inout count), choose(inout count));
    answer + count
}"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) { count <- count + 1; On }
fn main() {
    let mut count = 0;
    let score = match choose(inout count) { Off => 0 | On => 41 };
    score + count
}"#,
        Some("42"),
    );
}

#[test]
fn root_loans_forward_the_original_slot_and_keep_value_snapshots() {
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) {
    count <- count + 1;
    if count == 1 { Off } else { On }
}
fn get(read f) { f }
fn read_forward(read f) { get(f) }
fn replace(inout f, next) { f <- next }
fn write_forward(inout f, next) { replace(inout f, next) }
fn main() {
    let mut count = 0;
    let mut f = choose(inout count);
    let old = read_forward(f);
    write_forward(inout f, choose(inout count));
    match old { Off => match f { On => count + 40 | Off => 0 } | On => 0 }
}"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn score(read a, read b) {
    match a { On => match b { On => 42 | Off => 0 } | Off => 0 }
}
fn main() { let mut f = On; score(f,f) }"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn choose(inout count) { count <- count + 1; Off }
fn inspect(read f, n) { match f { Off => n + 40 | On => 0 } }
fn main() {
    let mut count = 0;
    inspect(choose(inout count), { count <- count + 1; count })
}"#,
        Some("42"),
    );
}

#[test]
fn repeated_calls_shadowing_and_distinct_source_instances_keep_identity() {
    compare(
        r#"type Flag<a> = Off | On
fn make(yes) { if yes { Off } else { On } }
fn id(x) { x }
fn main() {
    let f = make(true);
    let first = match id(f) { Off => 20 | On => 0 };
    let second = match id(f) { Off => 20 | On => 0 };
    let third = { let f = make(false); match id(f) { On => 2 | Off => 0 } };
    first + second + third
}"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn independent(x) {
    let local = if true { Off } else { On };
    match local { Off => x | On => x }
}
fn main() { if independent(true) { independent(40) + independent(2) } else { 0 } }"#,
        Some("42"),
    );
    compare(
        r#"type Flag<a> = Off | On
fn integer(x: Flag<i32>) { match x { Off => 40 | On => 0 } }
fn boolean(x: Flag<bool>) { match x { Off => 2 | On => 0 } }
fn main() { let f = Off; let alias = f; integer(alias) + boolean(f) }"#,
        Some("42"),
    );
}

#[test]
fn recursive_forwarding_preserves_tags_and_the_existing_call_depth_guard() {
    compare(
        r#"type Flag<a> = Off | On
fn first(n,x) { if n == 0 { x } else { second(n - 1,x) } }
fn second(n,x) { if n == 0 { x } else { first(n - 1,x) } }
fn main() { match first(7,On) { On => 42 | Off => 0 } }"#,
        Some("42"),
    );
    for (depth, expected) in [(30, Some("Off")), (31, None)] {
        compare(
            &format!("{FLAG}fn recur(n,read f) {{ if n == 0 {{ f }} else {{ recur(n - 1,f) }} }}\nfn main() {{ recur({depth},Off) }}"),
            expected,
        );
    }
}

#[test]
fn pure_nonterminating_nullary_recursion_has_a_finite_instance_worklist() {
    for source in [
        format!("{FLAG}fn again(x) {{ if false {{ x }} else {{ again(x) }} }}\nfn main() {{ again(Off) }}"),
        format!("{FLAG}fn first(x) {{ if false {{ x }} else {{ second(x) }} }}\nfn second(x) {{ first(x) }}\nfn main() {{ first(Off) }}"),
        format!("{FLAG}fn again() {{ if false {{ Off }} else {{ again() }} }}\nfn main() {{ again() }}"),
    ] {
        let program = parser::parse(&source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
        typed::verify(&lowered).unwrap();
        let c = typed::emit_c(&lowered).unwrap();
        assert_eq!(rill::backend::emit(&program).unwrap(), c);
        assert!(!c.contains("rl_value") && !c.contains("RL_CAP"));
        // This test deliberately does not execute any divergent program.
    }
}

#[test]
fn unused_results_and_later_arguments_do_not_erase_runtime_traps() {
    for source in [
        format!("{FLAG}fn make() {{ let n = 1 / 0; Off }}\nfn main() {{ let unused = make(); 42 }}"),
        format!("{FLAG}fn first(x,y) {{ x }}\nfn main() {{ first(Off,1 / 0) }}"),
        format!("{FLAG}fn inspect(read x,y) {{ 42 }}\nfn main() {{ inspect(Off,1 / 0) }}"),
        format!("{FLAG}fn main() {{ let f = {{ let n = 1 / 0; Off }}; match f {{ Off => 42 | On => 0 }} }}"),
        format!("{FLAG}fn main() {{ let f = Off; match f {{ Off => 1 / 0 | On => 42 }} }}"),
    ] {
        compare(&source, None);
    }
}

#[test]
fn weak_and_mutable_values_do_not_gain_polymorphism() {
    let declarations = format!("{FLAG}fn make() {{ Off }}\nfn integer(x: Flag<i32>) {{ 40 }}\nfn boolean(x: Flag<bool>) {{ 2 }}\n");
    for bindings in [
        "let f = make();",
        "let f = if true { Off } else { On };",
        "let f = match true { _ => Off };",
        "let mut f = Off;",
        "let first = make(); let second = first; let f = second;",
    ] {
        rejects_source(
            &format!("{declarations}fn main() {{ {bindings} integer(f) + boolean(f) }}"),
            "E-TYPE:",
        );
    }
    rejects_source(
        "type First<a> = A\ntype Second<a> = B\nfn main() { if true { A } else { B } }",
        "E-TYPE:",
    );
    rejects_source(
        &format!("{FLAG}fn main() {{ match Off {{ Off => 42 }} }}"),
        "E-MATCH:",
    );
    rejects_source(&format!("{FLAG}fn main() {{ Off == On }}"), "E-TYPE:");
}

#[test]
fn nullary_layout_does_not_relax_root_loan_conflicts() {
    for source in [
        format!("{FLAG}fn use_both(read a,inout b) {{ b <- a }}\nfn main() {{ let mut f = Off; use_both(f,inout f) }}"),
        format!("{FLAG}fn use_both(inout a,read b) {{ a <- b }}\nfn main() {{ let mut f = Off; use_both(inout f,f) }}"),
        format!("{FLAG}fn use_both(read a,b) {{ b }}\nfn main() {{ let mut f = Off; use_both(f,{{ f <- On; 42 }}) }}"),
        format!("{FLAG}fn set(inout f) {{ f <- On }}\nfn main() {{ let f = Off; set(inout f) }}"),
        format!("{FLAG}fn set(inout f) {{ f <- On }}\nfn forward(read f) {{ set(inout f) }}\nfn main() {{ forward(Off) }}"),
    ] {
        rejects_source(&source, "E-LOAN:");
    }
}

#[test]
fn payload_roots_and_unconstrained_nonreturning_results_remain_unsupported() {
    for source in [
        "type O<a> = None | Some(a)\nfn main() { let f = None; match f { None => 42 | Some(_) => 0 } }",
        "type Mark<a> = Mark(i32)\nfn main() { match Mark(42) { Mark(n) => n } }",
        "type Stamp<a> = { number: i32 }\nfn main() { let weak = if true { record Stamp { number = 42 } } else { record Stamp { number = 0 } }; weak.number }",
        "type Flag<a> = Off | On\ntype Link<a> = Link(a,Flag<a>)\nfn tag(x) { match Link(x,Off) { Link(_,f) => f } }\nfn main() { tag(Off) }",
        "fn forever() { forever() }\nfn main() { let unused = forever(); 42 }",
        "fn forever() { forever() }\nfn main() { if false { let unused = forever(); 0 } else { 42 } }",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{source}: {e}"));
        let error = typed::lower_main(&checked).unwrap_err();
        assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{source}: {error}");
        // Do not run the nonreturning controls or conflate typed rejection with
        // rejection by the source checker or by the separate uniform backend.
    }
}

#[test]
fn closed_nested_and_owner_phantom_arguments_keep_existing_behavior() {
    compare(
        "type Flag<a> = Off | On\ntype Wrap<a> = Wrap(a)\nfn integer(x: Flag<Wrap<i32>>) { 40 }\nfn boolean(x: Flag<Wrap<bool>>) { 2 }\nfn main() { integer(Off) + boolean(On) }",
        Some("42"),
    );
    for owner in ["SystemBoxI32", "SystemBytes"] {
        compare(
            &format!("type Owned = Own({owner})\ntype Phantom<a> = P\nfn inspect(x: Phantom<Owned>) {{ 42 }}\nfn main() {{ inspect(P) }}"),
            Some("42"),
        );
    }
    rejects_source(
        "type P<a> = P\nfn owner(x: P<SystemBoxI32>) { 42 }\nfn main() { let p = P; owner(p) }",
        "E-OWNED",
    );
}

#[test]
fn nested_nominal_and_raw_pointer_identity_still_constrain_weak_values() {
    for (first, second) in [
        ("Wrap<i32>", "Wrap<bool>"),
        ("ptr<void>", "const_ptr<void>"),
        ("ptr<First>", "ptr<Second>"),
        ("Wrap<ptr<void>>", "Wrap<const_ptr<void>>"),
    ] {
        rejects_source(
            &format!("{FLAG}type Wrap<a> = Wrap(a)\nfn make() {{ Off }}\nfn first(x: Flag<{first}>) {{ 40 }}\nfn second(x: Flag<{second}>) {{ 2 }}\nfn main() {{ let f = make(); first(f) + second(f) }}"),
            "E-TYPE:",
        );
    }
}

#[test]
fn symbolic_mapping_is_explicitly_rejected_after_ordinary_success() {
    for source in [
        "type Flag<a> = Off | On\ntype Link<a,b> = { input: a; tag: Flag<a>; unused: Flag<b> }\nfn tag(x) { let r = record Link { input = x; tag = Off; unused = Off }; r.tag }\nfn main() { let f = tag(Off); match f { Off => 42 | On => 0 } }".into(),
        format!("{FLAG}fn main() {{ Off }}"),
        format!("{FLAG}fn make() {{ Off }}\nfn main() {{ let f = make(); match f {{ Off => 42 | On => 0 }} }}"),
        format!("{FLAG}fn main() {{ let f = if true {{ Off }} else {{ On }}; match f {{ Off => 42 | On => 0 }} }}"),
    ] {
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap();
        typed::verify(&ordinary).unwrap();
        let before = rill::backend::emit(sources.program()).unwrap();
        assert_eq!(typed::emit_c(&ordinary).unwrap(), before);
        for (error, index) in [
            (lower_main_with_function_origins(sources.clone()).unwrap_err(), "function"),
            (lower_main_with_copy_node_origins(sources.clone()).unwrap_err(), "Copy node"),
        ] {
            assert_eq!(
                error,
                format!("E-TYPED-UNSUPPORTED: {index} origin index does not map opaque logical arguments"),
                "{source}"
            );
        }
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), before);
    }
}

#[test]
fn closed_mapping_controls_and_semantic_error_precedence_are_unchanged() {
    let fixture = Fixture::new();
    let path = fixture.write(
        "main.rill",
        &format!("{FLAG}fn integer(x: Flag<i32>) {{ 40 }}\nfn boolean(x: Flag<bool>) {{ 2 }}\nfn main() {{ integer(Off) + boolean(On) }}"),
    );
    let sources = load_program_with_sources(&path).unwrap();
    let before = rill::backend::emit(sources.program()).unwrap();
    let functions = lower_main_with_function_origins(sources.clone()).unwrap();
    let nodes = lower_main_with_copy_node_origins(sources).unwrap();
    assert_eq!(typed::emit_c(functions.typed()).unwrap(), before);
    assert_eq!(typed::emit_c(nodes.typed()).unwrap(), before);
    for source in [
        format!("{FLAG}fn forever() {{ forever() }}\nfn main() {{ let f = if true {{ Off }} else {{ On }}; let unused = forever(); match f {{ Off => 42 | On => 0 }} }}"),
        "type O<a> = None | Some(a)\nfn main() { None }".to_string(),
    ] {
        let path = fixture.write("bad.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        let ordinary = match check::analyze(sources.program()) {
            Ok(checked) => typed::lower_main(&checked).unwrap_err(),
            Err(error) => error,
        };
        assert_eq!(lower_main_with_function_origins(sources.clone()).unwrap_err(), ordinary);
        assert_eq!(lower_main_with_copy_node_origins(sources).unwrap_err(), ordinary);
    }
}

#[test]
fn nested_raw_phantom_metadata_remains_outside_origin_mapping() {
    for raw in ["ptr<void>", "const_ptr<void>"] {
        let source = format!("{FLAG}type Wrap<a> = Wrap(a)\nfn inspect(x: Flag<Wrap<{raw}>>) {{ 42 }}\nfn main() {{ inspect(Off) }}");
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        let checked = check::analyze(sources.program()).unwrap();
        assert!(!checked.has_c_values());
        let ordinary = typed::lower_main(&checked).unwrap();
        typed::verify(&ordinary).unwrap();
        let c = typed::emit_c(&ordinary).unwrap();
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), c);
        for error in [
            lower_main_with_function_origins(sources.clone()).unwrap_err(),
            lower_main_with_copy_node_origins(sources.clone()).unwrap_err(),
        ] {
            assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
            assert!(error.contains("native"), "{error}");
        }
    }
}

#[test]
fn imported_nullary_enums_keep_resolved_nominal_identity() {
    let fixture = Fixture::new();
    for name in ["left", "right"] {
        fixture.write(
            &format!("{name}.rill"),
            "export type Flag<a> = Off | On\nexport fn choose(yes) { if yes { Off } else { On } }",
        );
    }
    let source = "import left; import right;\nfn main() { let a = left.choose(true); let b = right.choose(false); match a { left.Off => match b { right.On => 42 | right.Off => 0 } | left.On => 0 } }";
    let path = fixture.write("main.rill", source);
    let program = rill::modules::compile::load_program(&path).unwrap();
    compare_program(&program, source, Some("42"));
    let path = fixture.write(
        "bad.rill",
        "import left; import right;\nfn main() { if true { left.Off } else { right.Off } }",
    );
    let error = rill::modules::compile::load_program(&path).unwrap_err();
    assert_eq!(error.code, "E-MODULE-CHECK");
    assert!(error.message.starts_with("E-TYPE:"));
}

#[test]
fn nested_open_logical_arguments_use_generalized_shapes_without_payload_layouts() {
    compare(
        "type Flag<a> = Off | On\ntype Link<a,b> = { input: a; tag: Flag<a>; unused: Flag<b> }\nfn tag(x) { let r = record Link { input = x; tag = Off; unused = Off }; r.tag }\nfn main() { tag(Off) }",
        Some("Off"),
    );
}
