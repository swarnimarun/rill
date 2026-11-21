use rill::{ast::Program, parser, reload::Session};

fn library(source: &str) -> Program {
    parser::parse(source).unwrap()
}

#[test]
fn host_state_continues_across_checked_hook_replacement() {
    let mut session = Session::new(
        library("fn update(state) { state + 1 } fn render(state) { state * 2 }"),
        &["update", "render"],
    )
    .unwrap();
    assert_eq!(session.generation(), 0);
    let state = session.call_i32("update", &[20]).unwrap();
    assert_eq!(session.call_i32("render", &[state]).unwrap(), 42);
    assert_eq!(
        session
            .replace(library(
                "fn update(state) { state + 2 } fn render(state) { state * 3 }"
            ))
            .unwrap(),
        1
    );
    let state = session.call_i32("update", &[state]).unwrap();
    assert_eq!(state, 23);
    assert_eq!(session.call_i32("render", &[state]).unwrap(), 69);
    assert_eq!(session.generation(), 1);
}

#[test]
fn rejected_candidates_retain_the_active_generation() {
    let mut session = Session::new(library("fn update(x) { x + 1 }"), &["update"]).unwrap();
    for source in [
        "fn other(x) { x + 2 }",
        "fn update(x, y) { x + y }",
        "fn update(read x) { x + 2 }",
        "fn update(inout x) { x <- x + 2; x }",
        "fn update(x: bool) -> i32 { if x { 42 } else { 0 } }",
        "fn update(x: i32) -> bool { x == 42 }",
        "fn update(x) { x }",
        "fn update(x) { x + true }",
        "fn update(x) { x + 2 } fn unused() { missing }",
        "fn update(x) { x + 2 } fn main() { 0 }",
    ] {
        assert!(session.replace(library(source)).is_err(), "{source}");
        assert_eq!(session.generation(), 0, "{source}");
        assert_eq!(session.call_i32("update", &[41]).unwrap(), 42, "{source}");
    }
    assert_eq!(
        session.replace(library("fn update(x) { x + 2 }")).unwrap(),
        1
    );
}

#[test]
fn hook_contract_rejects_missing_duplicate_generic_and_borrowed_hooks() {
    for (source, names) in [
        ("fn update(x) { x + 1 }", vec![]),
        ("fn update(x) { x + 1 }", vec!["update", "update"]),
        ("fn update(x) { x + 1 }", vec!["absent"]),
        ("fn update(x) { x }", vec!["update"]),
        ("fn update(read x) { x + 1 }", vec!["update"]),
        ("fn update(inout x) { x <- x + 1; x }", vec!["update"]),
        (
            "fn update(x: bool) { if x { 1 } else { 0 } }",
            vec!["update"],
        ),
        ("fn update(x: i32) { x == 0 }", vec!["update"]),
        ("fn main() { 42 }", vec!["main"]),
        ("", vec!["byte_len"]),
    ] {
        assert!(Session::new(library(source), &names).is_err(), "{source}");
    }
}

#[test]
fn native_declarations_dependencies_raw_values_and_owners_reject() {
    for preamble in [
        "@include(\"stddef.h\");",
        "@link(\"example\");",
        "@pkg_config(\"example\");",
        "extern fn example() -> i32;",
        "extern const example: i32;",
    ] {
        let source = format!("{preamble} fn update(x) {{ x + 1 }}");
        let result = Session::new(library(&source), &["update"]);
        assert!(result.err().unwrap().contains("E-RELOAD"), "{source}");
    }
    for source in [
        "fn update(x) { let owner = system_box_i32_abort(x); box_read(owner) }",
        "fn update(x) { x + 1 } fn unused() { let owner = system_box_i32_abort(0); box_read(owner) }",
        "type Resource = Resource(SystemBoxI32) fn update(x) { x + 1 }",
        "fn update(x) { let raw = u32(x); x + 1 }",
    ] {
        assert!(Session::new(library(source), &["update"]).is_err(), "{source}");
    }
}

#[test]
fn dormant_raw_types_reject_new_and_replacement_without_changing_state() {
    let mut candidates = [
        "u8",
        "u32",
        "u64",
        "f32",
        "ptr<Handle>",
        "const_ptr<Handle>",
    ]
    .map(|ty| {
        library(&format!(
            "fn ignored(raw: {ty}) -> i32 {{ 0 }} fn update(x: i32) -> i32 {{ x + 100 }}"
        ))
    })
    .to_vec();
    for source in [
        "type Raw = { value: u32 } fn update(x: i32) -> i32 { x + 100 }",
        "type Raw = { value: ptr<Handle> } type Outer = { inner: Raw } fn ignored(value: Outer) -> i32 { 0 } fn update(x: i32) -> i32 { x + 100 }",
        "type Phantom<T> = P; fn ignored(value: Phantom<u32>) -> i32 { 0 } fn update(x: i32) -> i32 { x + 100 }",
        "type Phantom<T> = P; type Raw = { value: Phantom<ptr<Handle>> } fn update(x: i32) -> i32 { x + 100 }",
        "fn ignored() { let f = fn(raw: u32) { 0 }; 0 } fn update(x: i32) -> i32 { x + 100 }",
        "type Holder<T> = { callback: T } fn ignored() { let holder = record Holder { callback = fn(raw: const_ptr<Handle>) { 0 } }; 0 } fn update(x: i32) -> i32 { x + 100 }",
    ] {
        candidates.push(library(source));
    }
    let mut cstr =
        library("fn ignored(raw: i32) -> i32 { 0 } fn update(x: i32) -> i32 { x + 100 }");
    cstr.functions[0].params[0]
        .annotation
        .as_mut()
        .unwrap()
        .name = "$cstr".into();
    candidates.push(cstr);
    let mut callable_cstr = library(
        "fn ignored() { let f = fn(raw: i32) { 0 }; 0 } fn update(x: i32) -> i32 { x + 100 }",
    );
    let rill::ast::Expr::Let(_, value, _) = &mut callable_cstr.functions[0].body else {
        panic!("expected the unused callable binding");
    };
    let rill::ast::Expr::Lambda(params, _) = value.as_mut() else {
        panic!("expected the callable initializer");
    };
    params[0].annotation.as_mut().unwrap().name = "$cstr".into();
    candidates.push(callable_cstr);

    let mut session = Session::new(library("fn update(x) { x + 1 }"), &["update"]).unwrap();
    assert_eq!(
        session.replace(library("fn update(x) { x + 2 }")).unwrap(),
        1
    );
    for candidate in candidates {
        // These are valid source libraries. The pure embedding contract must
        // reject their dormant raw types before publishing either generation.
        rill::check::analyze_library(&candidate).unwrap();
        let Err(error) = Session::new(candidate.clone(), &["update"]) else {
            panic!("raw library must reject");
        };
        assert!(error.starts_with("E-RELOAD:"), "{error}");
        let error = session.replace(candidate).unwrap_err();
        assert!(error.starts_with("E-RELOAD:"), "{error}");
        assert_eq!(session.generation(), 1);
        assert_eq!(session.call_i32("update", &[40]).unwrap(), 42);
    }
}

#[test]
fn generic_type_parameters_can_shadow_raw_names_in_reload_libraries() {
    let program = library("type Wrapper<u32> = Wrap(u32); fn update(x: i32) -> i32 { match Wrap(x) { Wrap(value) => value + 1 } }");
    let session = Session::new(program, &["update"]).unwrap();
    assert_eq!(session.call_i32("update", &[41]).unwrap(), 42);
}

#[test]
fn calls_check_registration_and_arity_and_accept_zero_arguments() {
    let session = Session::new(
        library("fn answer() { 42 } fn sum(x, y) { x + y } fn helper(x) { x + 1 }"),
        &["answer", "sum"],
    )
    .unwrap();
    assert_eq!(session.call_i32("answer", &[]).unwrap(), 42);
    assert_eq!(session.call_i32("sum", &[20, 22]).unwrap(), 42);
    assert!(session.call_i32("answer", &[0]).is_err());
    assert!(session.call_i32("sum", &[42]).is_err());
    assert!(session.call_i32("helper", &[41]).is_err());
    assert!(session.call_i32("absent", &[]).is_err());
}

#[test]
fn validation_never_executes_hooks_and_traps_preserve_the_generation() {
    let mut session = Session::new(library("fn update(x) { x / 0 }"), &["update"]).unwrap();
    assert!(session.call_i32("update", &[42]).is_err());
    assert_eq!(session.generation(), 0);
    assert_eq!(
        session.replace(library("fn update(x) { x + 1 }")).unwrap(),
        1
    );
    assert!(session.call_i32("update", &[i32::MAX]).is_err());
    assert_eq!(session.call_i32("update", &[41]).unwrap(), 42);
    assert_eq!(session.generation(), 1);
}

#[test]
fn helper_changes_and_pure_generic_helpers_use_the_new_generation() {
    let mut session = Session::new(
        library("fn identity(x) { x } fn helper(x) { identity(x) + 1 } fn update(x) { helper(x) }"),
        &["update"],
    )
    .unwrap();
    assert_eq!(session.call_i32("update", &[40]).unwrap(), 41);
    session
        .replace(library(
            "fn helper(x) { let add = fn(y) { y + 2 }; add(x) } fn update(x) { helper(x) }",
        ))
        .unwrap();
    assert_eq!(session.call_i32("update", &[40]).unwrap(), 42);
}

#[test]
fn bundled_source_hooks_receive_explicit_host_generations() {
    use std::{collections::BTreeMap, fs};

    let directory =
        std::env::temp_dir().join(format!("rill-reload-source-hooks-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("hooks.rill");
    let source = r#"
import std.reload;
fn update(state: i32, generation: i32, previous: i32) -> i32 {
    let event = reload.context(generation, previous);
    if reload.first_load(event) { state + 1 } else {
        if reload.changed(event) { state + 2 } else { state }
    }
}
"#;
    let overlays = BTreeMap::from([(entry.clone(), source.to_owned())]);
    let loaded = rill::modules::compile::load_library_with_sources_and_overlays(&entry, &overlays);
    // Sources are retained by the loaded value; this test creates no disk file.
    fs::remove_dir(&directory).unwrap();
    let program = loaded.unwrap().into_program();
    let hook = program
        .functions
        .iter()
        .find(|function| function.name.ends_with("$update"))
        .unwrap()
        .name
        .clone();
    let mut session = Session::new(program.clone(), &[&hook]).unwrap();
    let state = session.call_i32(&hook, &[40, 0, -1]).unwrap();
    assert_eq!(state, 41);
    assert_eq!(session.call_i32(&hook, &[state, 0, 0]).unwrap(), state);
    session.replace(program).unwrap();
    assert_eq!(
        session
            .call_i32(&hook, &[state, session.generation(), 0])
            .unwrap(),
        43
    );
}
