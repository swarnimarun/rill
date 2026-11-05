#![cfg(any(target_os = "macos", target_os = "linux"))]

use rill::abi::Export;
use rill::native_reload::{NativeCandidate, NativeCompiler, NativeSession, Optimization};

fn exports() -> Vec<Export> {
    vec![Export {
        source: "update".into(),
        c_name: "rill_update".into(),
    }]
}

fn candidate(source: &str, optimization: Optimization) -> NativeCandidate {
    NativeCandidate::build(
        &rill::parser::parse(source).unwrap(),
        &exports(),
        &NativeCompiler::new("cc", optimization),
    )
    .unwrap()
}

#[test]
fn native_body_and_helper_replacement_preserves_host_integer_state() {
    for optimization in [Optimization::O0, Optimization::O3] {
        let first = candidate(
            "fn helper(x: i32) -> i32 { x + 1 }\nfn update(state: i32) -> i32 { helper(state) }",
            optimization,
        );
        let first_identity = first.build_identity().to_owned();
        assert_eq!(first_identity.len(), 64);
        let mut session = NativeSession::new(first).unwrap();
        let mut state = 10;
        state = session.call_i32("rill_update", &[state]).unwrap();
        assert_eq!(state, 11);
        assert_eq!(session.generation(), 0);
        let second = candidate(
            "fn update(state: i32) -> i32 { helper(state) }\nfn helper(x: i32) -> i32 { x + 7 }",
            optimization,
        );
        assert_ne!(second.build_identity(), first_identity);
        assert_eq!(session.replace(second).unwrap(), 1);
        state = session.call_i32("rill_update", &[state]).unwrap();
        assert_eq!(state, 18);
        assert_eq!(session.generation(), 1);
    }
}

#[test]
fn several_roots_use_verified_ids_after_export_and_declaration_reordering() {
    let first = rill::parser::parse(
        "fn helper(x: i32) -> i32 { x * 2 }\nfn update(x: i32) -> i32 { helper(x) + 1 }\nfn constant() -> i32 { 42 }",
    )
    .unwrap();
    let mut roots = exports();
    roots.push(Export {
        source: "constant".into(),
        c_name: "rill_constant".into(),
    });
    let first = NativeCandidate::build(&first, &roots, &NativeCompiler::default()).unwrap();
    let manifest = first.manifest().to_owned();
    let mut session = NativeSession::new(first).unwrap();
    assert_eq!(session.call_i32("rill_constant", &[]).unwrap(), 42);
    assert_eq!(session.call_i32("rill_update", &[3]).unwrap(), 7);
    roots.reverse();
    for root in &mut roots {
        root.source = if root.c_name == "rill_constant" {
            "answer".into()
        } else {
            "run".into()
        };
    }
    let second = rill::parser::parse(
        "fn answer() -> i32 { 43 }\nfn run(x: i32) -> i32 { helper(x) + 2 }\nfn helper(x: i32) -> i32 { x * 3 }",
    )
    .unwrap();
    let second = NativeCandidate::build(&second, &roots, &NativeCompiler::default()).unwrap();
    assert_eq!(second.manifest(), manifest);
    session.replace(second).unwrap();
    assert_eq!(session.call_i32("rill_constant", &[]).unwrap(), 43);
    assert_eq!(session.call_i32("rill_update", &[3]).unwrap(), 11);
}

#[test]
fn source_library_resolves_standard_reload_hooks_for_each_candidate() {
    use std::collections::BTreeMap;

    let entry = std::env::temp_dir().join(format!(
        "rill-native-hook-overlay-{}.rill",
        std::process::id()
    ));
    let build = |increment: i32| {
        let source = format!(
            "import std.reload;\nfn update(state: i32, generation: i32, previous: i32) -> i32 {{\nlet event = reload.context(generation, previous);\nif reload.first_load(event) {{ state + 1 }} else {{\nif reload.changed(event) {{ state + {increment} }} else {{ state }}\n}}\n}}"
        );
        let overlays = BTreeMap::from([(entry.clone(), source)]);
        let loaded =
            rill::modules::compile::load_library_with_sources_and_overlays(&entry, &overlays)
                .unwrap();
        let symbol = loaded
            .symbols()
            .iter()
            .find(|symbol| symbol.symbol.name == "update")
            .unwrap();
        let rill::source_map::AstNode::Function(function) = loaded.node(&symbol.target).unwrap()
        else {
            panic!("update must resolve to a function");
        };
        NativeCandidate::build(
            loaded.program(),
            &[Export {
                source: function.name.clone(),
                c_name: "rill_update".into(),
            }],
            &NativeCompiler::default(),
        )
        .unwrap()
    };
    let mut session = NativeSession::new(build(2)).unwrap();
    let state = session.call_i32("rill_update", &[10, 0, -1]).unwrap();
    assert_eq!(state, 11);
    session.replace(build(7)).unwrap();
    let state = session
        .call_i32("rill_update", &[state, session.generation(), 0])
        .unwrap();
    assert_eq!(state, 18);
    assert_eq!(session.call_i32("rill_update", &[state, 1, 1]).unwrap(), 18);
}

#[test]
fn failed_build_and_incompatible_contract_leave_old_generation_callable() {
    let mut session = NativeSession::new(candidate(
        "fn update(x: i32) -> i32 { x + 1 }",
        Optimization::O0,
    ))
    .unwrap();
    let invalid =
        rill::parser::parse("fn update(x: i32) -> i32 { x + 2 }\nfn unused() -> i32 { false }")
            .unwrap();
    assert!(NativeCandidate::build(&invalid, &exports(), &NativeCompiler::default()).is_err());
    let wrong_arity = candidate(
        "fn update(x: i32, y: i32) -> i32 { x + y }",
        Optimization::O3,
    );
    assert!(session.replace(wrong_arity).is_err());
    let renamed = rill::parser::parse("fn update(x: i32) -> i32 { x + 9 }").unwrap();
    let renamed = NativeCandidate::build(
        &renamed,
        &[Export {
            source: "update".into(),
            c_name: "rill_renamed".into(),
        }],
        &NativeCompiler::default(),
    )
    .unwrap();
    assert!(session.replace(renamed).is_err());
    assert_eq!(session.generation(), 0);
    assert_eq!(session.call_i32("rill_update", &[4]).unwrap(), 5);
    assert!(session.call_i32("rill_update", &[]).is_err());
    assert!(session.call_i32("rill_unknown", &[4]).is_err());
    assert_eq!(session.call_i32("rill_update", &[4]).unwrap(), 5);
}

#[test]
fn compiler_failure_cannot_replace_the_running_generation() {
    let mut session = NativeSession::new(candidate(
        "fn update(x: i32) -> i32 { x + 1 }",
        Optimization::O0,
    ))
    .unwrap();
    let source = rill::parser::parse("fn update(x: i32) -> i32 { x + 8 }").unwrap();
    // /usr/bin/false is a trusted executable that never emits an artifact.
    let error = NativeCandidate::build(
        &source,
        &exports(),
        &NativeCompiler::new("/usr/bin/false", Optimization::O0),
    )
    .err()
    .unwrap();
    assert!(error.contains("native compiler failed"), "{error}");
    assert_eq!(session.generation(), 0);
    assert_eq!(session.call_i32("rill_update", &[4]).unwrap(), 5);
    let error = NativeCandidate::build(
        &source,
        &exports(),
        &NativeCompiler::new("/usr/bin/true", Optimization::O0),
    )
    .err()
    .unwrap();
    assert!(error.contains("regular candidate library"), "{error}");
}

#[test]
fn recursive_and_many_argument_endpoints_keep_the_exact_c_contract() {
    for optimization in [Optimization::O0, Optimization::O3] {
        let source = "fn update(x: i32) -> i32 { if x == 0 { 0 } else { 1 + update(x - 1) } }";
        let mut session = NativeSession::new(candidate(source, optimization)).unwrap();
        assert_eq!(session.call_i32("rill_update", &[10]).unwrap(), 10);
        let parameters = (0..32)
            .map(|index| format!("x{index}: i32"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("fn update({parameters}) -> i32 {{ x0 + x31 }}");
        let mut session = NativeSession::new(candidate(&source, optimization)).unwrap();
        let mut arguments = [0; 32];
        arguments[0] = i32::MIN;
        arguments[31] = i32::MAX;
        assert_eq!(session.call_i32("rill_update", &arguments).unwrap(), -1);
    }
}

#[test]
fn native_runtime_traps_terminate_only_the_child_test_process() {
    const CHILD: &str = "RILL_NATIVE_RELOAD_TRAP_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        let source = if mode == "depth" {
            "fn update(x: i32) -> i32 { update(x) }"
        } else {
            "fn update(x: i32) -> i32 { x + 1 }"
        };
        let mut session = NativeSession::new(candidate(source, Optimization::O3)).unwrap();
        let _ = session.call_i32("rill_update", &[i32::MAX]);
        panic!("native trap returned");
    }
    for mode in ["arithmetic", "depth"] {
        let directory = std::env::temp_dir().join(format!(
            "rill-native-trap-test-{}-{mode}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("native_runtime_traps_terminate_only_the_child_test_process")
            .arg("--nocapture")
            .env(CHILD, mode)
            .env("TMPDIR", &directory)
            .output()
            .unwrap();
        // Traps exit without running destructors. The parent owns the child's
        // temporary root and removes its source and library after it exits.
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "rill: runtime trap\n"
        );
    }
}
