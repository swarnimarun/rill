use rill::abi::{emit_i32_library, Artifact, Export};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn export(source: &str, c_name: &str) -> Export {
    Export {
        source: source.into(),
        c_name: c_name.into(),
    }
}

fn emit(source: &str, exports: &[Export]) -> Result<Artifact, String> {
    emit_i32_library(&rill::parser::parse(source)?, exports)
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-abi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_c_output(artifact: &Artifact, caller: &str, expected: &[u8]) {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("library.c"), &artifact.c).unwrap();
    fs::write(fixture.0.join("library.h"), &artifact.header).unwrap();
    fs::write(fixture.0.join("caller.c"), caller).unwrap();
    for optimization in ["-O0", "-O3"] {
        let executable = fixture.0.join("caller");
        let build = Command::new("cc")
            .args([
                "-std=c11",
                optimization,
                "-Werror=implicit-function-declaration",
            ])
            .arg(fixture.0.join("library.c"))
            .arg(fixture.0.join("caller.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, expected, "{optimization}");
        assert!(run.stderr.is_empty(), "{optimization}");
    }
}

#[test]
fn explicit_roots_generate_deterministic_public_artifacts() {
    let source = "fn private_helper(x: i32) -> i32 { x * 2 }\n\
                  fn first(x: i32, y: i32) -> i32 { private_helper(x) + y }\n\
                  fn second() -> i32 { 42 }";
    // Deliberately reverse source order. Discovery IDs are not export ordinals.
    let exports = [export("second", "rill_answer"), export("first", "rill_add")];
    let artifact = emit(source, &exports).unwrap();
    assert_eq!(artifact, emit(source, &exports).unwrap());
    assert!(!artifact.c.contains("int main("));
    assert!(!artifact.c.contains("rl_value"));
    assert!(artifact.c.contains("_Thread_local"));
    assert!(artifact.header.contains("#define RILL_I32_ABI_VERSION 1"));
    assert!(artifact.header.contains("int32_t rill_answer(void);"));
    assert!(artifact
        .header
        .contains("int32_t rill_add(int32_t arg0, int32_t arg1);"));
    let manifest: serde_json::Value = serde_json::from_str(&artifact.manifest).unwrap();
    assert_eq!(
        manifest,
        serde_json::json!({
            "abi": "rill-c-i32",
            "version": 1,
            "exports": [
                { "name": "rill_answer", "arity": 0, "parameters": [], "result": "i32" },
                { "name": "rill_add", "arity": 2, "parameters": ["i32", "i32"], "result": "i32" }
            ]
        })
    );
    for text in [&artifact.header, &artifact.manifest] {
        assert!(!text.contains("private_helper"));
        assert!(!text.contains("rl_fn_"));
        assert!(!text.contains("first"));
        assert!(!text.contains("second"));
    }
}

#[test]
fn public_names_keep_their_targets_after_source_reordering() {
    let declarations = [
        "fn helper(x: i32) -> i32 { x * 2 }",
        "fn update(x: i32) -> i32 { helper(x) + 4 }",
        "fn answer() -> i32 { 13 }",
    ];
    let exports = [
        export("answer", "rill_answer"),
        export("update", "rill_update"),
    ];
    let original = emit(&declarations.join("\n"), &exports).unwrap();
    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
        let source = order.map(|index| declarations[index]).join("\n");
        let artifact = emit(&source, &exports).unwrap();
        assert_eq!(artifact.header, original.header);
        assert_eq!(artifact.manifest, original.manifest);
        assert_c_output(
            &artifact,
            "#include \"library.h\"\nint main(void) { return rill_update(19) == 42 && rill_answer() == 13 ? 0 : 1; }\n",
            b"",
        );
    }
}

#[test]
fn loaded_std_reload_hooks_export_resolved_function_to_c() {
    let fixture = Fixture::new();
    let entry = fixture.0.join("hooks.rill");
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
    let loaded =
        rill::modules::compile::load_library_with_sources_and_overlays(&entry, &overlays).unwrap();
    assert!(!entry.exists());
    assert!(loaded
        .sources()
        .iter()
        .any(|snapshot| snapshot.module == "std.reload"));
    let symbol = loaded
        .symbols()
        .iter()
        .find(|symbol| symbol.symbol.name == "update")
        .unwrap();
    assert_eq!(loaded.source(symbol.source).unwrap().text, source);
    let rill::source_map::AstNode::Function(function) = loaded.node(&symbol.target).unwrap() else {
        panic!("update must resolve to its loaded function declaration");
    };
    let artifact =
        emit_i32_library(loaded.program(), &[export(&function.name, "rill_update")]).unwrap();
    assert_c_output(
        &artifact,
        r#"
#include "library.h"
int main(void) {
    int32_t state = rill_update(40, 0, -1);
    if (state != 41) return 1;
    if (rill_update(state, 0, 0) != state) return 2;
    if (rill_update(state, 1, 0) != 43) return 3;
    if (rill_update(INT32_MIN, 0, 0) != INT32_MIN) return 4;
    return 0;
}
"#,
        b"",
    );
}

#[test]
fn c11_callers_use_multiple_roots_recursion_and_traps_at_o0_and_o3() {
    let source = "fn helper(x: i32) -> i32 { x * 2 }\n\
        fn add(x: i32, y: i32) -> i32 { helper(x) + y }\n\
        fn count(n: i32) -> i32 { if n == 0 { 7 } else { count(n - 1) } }\n\
        fn even(n: i32) -> i32 { if n == 0 { 1 } else { odd(n - 1) } }\n\
        fn odd(n: i32) -> i32 { if n == 0 { 0 } else { even(n - 1) } }\n\
        fn divide(x: i32, y: i32) -> i32 { x / y }\n\
        fn answer() -> i32 { 42 }";
    let artifact = emit(
        source,
        &[
            export("answer", "rill_answer"),
            export("count", "rill_count"),
            export("add", "rill_add"),
            export("even", "rill_even"),
            export("divide", "rill_divide"),
        ],
    )
    .unwrap();
    // A second header must not be suppressed by a shared include guard.
    let other = emit(
        "fn identity(x: i32) -> i32 { x }",
        &[export("identity", "rill_identity")],
    )
    .unwrap();
    let fixture = Fixture::new();
    fs::write(fixture.0.join("library.c"), artifact.c).unwrap();
    fs::write(fixture.0.join("library.h"), artifact.header).unwrap();
    fs::write(fixture.0.join("other.c"), other.c).unwrap();
    fs::write(fixture.0.join("other.h"), other.header).unwrap();
    fs::write(
        fixture.0.join("caller.c"),
        r#"
#include "library.h"
#include "other.h"
#include "library.h"
#include <stdio.h>
#include <stdlib.h>
#if defined(__unix__) || defined(__APPLE__)
#include <pthread.h>
static void *exercise(void *unused) {
    (void)unused;
    for (int i = 0; i < 1000; ++i) {
        if (rill_count(31) != 7) return (void *)1;
    }
    return NULL;
}
#endif
int main(void) {
    const char *test = getenv("RILL_ABI_CASE");
    if (test != NULL) {
        switch (test[0]) {
        case 'o': (void)rill_add(INT32_MAX, 1); break;
        case 'z': (void)rill_divide(1, 0); break;
        case 'd': (void)rill_count(32); break;
        case 'm': (void)rill_divide(INT32_MIN, -1); break;
        default: return 99;
        }
        return 98;
    }
    for (int i = 0; i < 3; ++i) {
        if (rill_count(31) != 7 || rill_even(15) != 0 || rill_even(16) != 1) return 2;
    }
    if (rill_identity(INT32_MIN) != INT32_MIN || rill_identity(INT32_MAX) != INT32_MAX) return 3;
#if defined(__unix__) || defined(__APPLE__)
    pthread_t a, b;
    void *first, *second;
    if (pthread_create(&a, NULL, exercise, NULL) || pthread_create(&b, NULL, exercise, NULL)) return 4;
    if (pthread_join(a, &first) || pthread_join(b, &second) || first || second) return 5;
#endif
    printf("%d %d %d\n", (int)rill_add(19, 4), (int)rill_answer(), (int)rill_divide(-7, 3));
    return 0;
}
"#,
    )
    .unwrap();
    for optimization in ["-O0", "-O3"] {
        let executable = fixture.0.join("caller");
        let mut command = Command::new("cc");
        command.args([
            "-std=c11",
            optimization,
            "-Werror=implicit-function-declaration",
        ]);
        if cfg!(unix) {
            command.arg("-pthread");
        }
        let build = command
            .arg(fixture.0.join("library.c"))
            .arg(fixture.0.join("other.c"))
            .arg(fixture.0.join("caller.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&executable)
            .env_remove("RILL_ABI_CASE")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"42 42 -2\n");
        assert!(run.stderr.is_empty());
        for case in ["overflow", "zero", "depth", "minimum"] {
            let run = Command::new(&executable)
                .env("RILL_ABI_CASE", case)
                .output()
                .unwrap();
            assert_eq!(run.status.code(), Some(1), "{optimization} {case}");
            assert!(run.stdout.is_empty(), "{optimization} {case}");
            assert_eq!(run.stderr, b"rill: runtime trap\n", "{optimization} {case}");
        }
    }
}

#[test]
fn all_declarations_are_checked_before_export_validation() {
    for source in [
        "fn valid(x: i32) -> i32 { x } fn invalid() { missing }",
        "fn unused() { c\"raw\" } fn valid(x: i32) -> i32 { x }",
        "fn unused(raw: cstr) -> i32 { 0 } fn valid(x: i32) -> i32 { x }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let checked_error = rill::check::analyze_library(&program).err().unwrap();
        assert_eq!(emit_i32_library(&program, &[]).unwrap_err(), checked_error);
        assert_eq!(
            emit_i32_library(&program, &[export("valid", "rill_valid")]).unwrap_err(),
            checked_error
        );
    }
}

#[test]
fn invalid_roots_modes_and_signatures_are_rejected() {
    for source in [
        "fn root(x) { x }",
        "fn root(x: bool) -> i32 { if x { 1 } else { 0 } }",
        "type Point = { x: i32 } fn root(p: Point) -> i32 { p.x }",
        "type Choice = A | B; fn root() -> Choice { A }",
        "fn root(x: i32) { x == 0 }",
        "fn root() { () }",
        "fn root(read x: i32) -> i32 { x }",
        "fn root(inout x: i32) -> i32 { x }",
    ] {
        let error = emit(source, &[export("root", "rill_root")]).unwrap_err();
        assert!(
            error.contains("closed monomorphic value-i32"),
            "{source}: {error}"
        );
    }
    assert!(
        emit("fn root() -> i32 { 1 }", &[export("absent", "rill_root")])
            .unwrap_err()
            .contains("unknown source function")
    );
    assert!(
        emit("fn main() -> i32 { 1 }", &[export("main", "rill_root")])
            .unwrap_err()
            .contains("executable main")
    );
}

#[test]
fn names_and_export_budgets_are_checked() {
    let source = "fn root(x: i32) -> i32 { x } fn other() -> i32 { 2 }";
    for name in [
        "root",
        "main",
        "int",
        "rl_fn_0",
        "_RILL",
        "rill_",
        "rill_é",
        "rill_x;int injected",
        "rill_x\n",
        "rill_abi_version",
    ] {
        assert!(
            emit(source, &[export("root", name)])
                .unwrap_err()
                .contains("invalid C export name"),
            "{name:?}"
        );
    }
    assert!(emit(source, &[]).unwrap_err().contains("1 to 64"));
    assert!(emit(source, &vec![export("root", "rill_root"); 65])
        .unwrap_err()
        .contains("1 to 64"));
    assert!(emit(
        source,
        &[export("root", "rill_a"), export("root", "rill_b")]
    )
    .unwrap_err()
    .contains("duplicate source export"));
    assert!(emit(
        source,
        &[export("root", "rill_same"), export("other", "rill_same")]
    )
    .unwrap_err()
    .contains("duplicate C export"));
    let params = (0..33)
        .map(|i| format!("x{i}: i32"))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(emit(
        &format!("fn root({params}) -> i32 {{ x0 }}"),
        &[export("root", "rill_root")]
    )
    .unwrap_err()
    .contains("32 arguments"));
}

#[test]
fn native_owner_callable_and_symbolic_forms_are_rejected() {
    for source in [
        "@link(\"m\"); fn root(x: i32) -> i32 { x }",
        "@include(\"unavailable-header.h\"); fn root(x: i32) -> i32 { x }",
        "@include(\"unavailable-header.h\"); extern const value: i32; fn root(x: i32) -> i32 { x }",
        "fn pointer(x: ptr<Handle>) -> ptr<Handle> { x } fn root(x: i32) -> i32 { x }",
        "extern fn abs(x: i32) -> i32; fn root(x: i32) -> i32 { x }",
        "fn root(x: i32) -> i32 { let raw = u32(x); x }",
        "fn root(x: i32) -> i32 { let owned = system_box_i32_abort(x); box_read(owned) }",
        "fn root(x: i32) -> i32 { let callable = fn(y) { y + 1 }; callable(x) }",
        // A phantom local freezes an unresolved logical class even though the
        // selected endpoint itself has an integer signature.
        "type Phantom<T> = P; fn phantom() { P } fn root(x: i32) -> i32 { let hidden = phantom(); x }",
        "fn forever() { forever() } fn root(x: i32) -> i32 { let hidden = forever(); x }",
    ] {
        let program = rill::parser::parse(source)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        rill::check::analyze_library(&program)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        assert!(emit_i32_library(&program, &[export("root", "rill_root")]).is_err(), "{source}");
    }
}

#[test]
fn unused_raw_and_owner_signatures_are_rejected() {
    let mut cases = [
        "u8",
        "u32",
        "u64",
        "f32",
        "ptr<Handle>",
        "const_ptr<Handle>",
        "SystemBoxI32",
        "SystemBytes",
    ]
    .map(|annotation| {
        format!("fn ignored(raw: {annotation}) -> i32 {{ 0 }} fn root(x: i32) -> i32 {{ x }}")
    })
    .to_vec();
    cases.extend([
        "type Raw = { value: u32 } type Outer = { inner: Raw } fn ignored(value: Outer) -> i32 { 0 } fn root(x: i32) -> i32 { x }".into(),
        "type Phantom<T> = P; fn ignored(value: Phantom<u32>) -> i32 { 0 } fn root(x: i32) -> i32 { x }".into(),
        "type Phantom<T> = P; fn ignored(value: Phantom<ptr<Handle>>) -> i32 { 0 } fn root(x: i32) -> i32 { x }".into(),
    ]);
    let mut admitted = Vec::new();
    for source in &cases {
        let program = rill::parser::parse(source).unwrap();
        rill::check::analyze_library(&program).unwrap_or_else(|error| panic!("{source}: {error}"));
        match emit_i32_library(&program, &[export("root", "rill_root")]) {
            Ok(_) => admitted.push(source),
            Err(error) => assert!(error.starts_with("E-ABI:"), "{source}: {error}"),
        }
    }
    assert!(
        admitted.is_empty(),
        "admitted forbidden signatures: {admitted:#?}"
    );
}

#[test]
fn unused_raw_type_declarations_are_rejected() {
    let mut admitted = Vec::new();
    for declaration in [
        "type Raw = { value: u32 }",
        "type Raw = { value: ptr<Handle> }",
        "type Raw = Absent | Present(u32);",
        "type Phantom<T> = P; type Raw = { value: Phantom<u32> }",
    ] {
        let source = format!("{declaration} fn root(x: i32) -> i32 {{ x }}");
        let program = rill::parser::parse(&source).unwrap();
        rill::check::analyze_library(&program).unwrap_or_else(|error| panic!("{source}: {error}"));
        match emit_i32_library(&program, &[export("root", "rill_root")]) {
            Ok(_) => admitted.push(source),
            Err(error) => assert!(error.starts_with("E-ABI:"), "{source}: {error}"),
        }
    }
    assert!(
        admitted.is_empty(),
        "admitted forbidden declarations: {admitted:#?}"
    );
}

#[test]
fn internal_cstr_annotations_cannot_bypass_the_public_ast_boundary() {
    let mut program =
        rill::parser::parse("fn ignored(raw: i32) -> i32 { 0 } fn root(x: i32) -> i32 { x }")
            .unwrap();
    // CStr is an internal checker type used for foreign-call arguments. The
    // public API accepts an AST, so it must reject this unused parameter too.
    program.functions[0].params[0]
        .annotation
        .as_mut()
        .unwrap()
        .name = "$cstr".into();
    rill::check::analyze_library(&program).unwrap();
    let Err(error) = emit_i32_library(&program, &[export("root", "rill_root")]) else {
        panic!("an unused internal CStr parameter must reject");
    };
    assert!(error.starts_with("E-ABI:"), "{error}");
}

#[test]
fn generic_type_parameters_named_like_raw_types_remain_ordinary_types() {
    let source = "type Wrapper<u32> = Wrap(u32); \
        fn ignored(value: Wrapper<i32>) -> i32 { 0 } \
        fn root(x: i32) -> i32 { match Wrap(x) { Wrap(value) => value } }";
    let artifact = emit(source, &[export("root", "rill_root")]).unwrap();
    assert_c_output(
        &artifact,
        "#include \"library.h\"\nint main(void) { return rill_root(42) == 42 ? 0 : 1; }\n",
        b"",
    );
}

#[test]
fn maximum_export_and_argument_counts_are_admitted() {
    let params = (0..32)
        .map(|i| format!("x{i}: i32"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = format!("fn root({params}) -> i32 {{ x0 + x31 }}\n");
    let mut exports = vec![export("root", "rill_root")];
    for index in 1..64 {
        source.push_str(&format!("fn endpoint{index}() -> i32 {{ {index} }}\n"));
        exports.push(export(
            &format!("endpoint{index}"),
            &format!("rill_f{index}"),
        ));
    }
    let artifact = emit(&source, &exports).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&artifact.manifest).unwrap();
    assert_eq!(manifest["exports"].as_array().unwrap().len(), 64);
    assert_eq!(manifest["exports"][0]["arity"], 32);
    assert!(artifact.header.contains("int32_t arg31"));
    let fixture = Fixture::new();
    fs::write(fixture.0.join("library.c"), artifact.c).unwrap();
    fs::write(fixture.0.join("library.h"), artifact.header).unwrap();
    let arguments = (0..32)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(fixture.0.join("caller.c"), format!(
        "#include \"library.h\"\nint main(void) {{ return rill_root({arguments}) == 31 && rill_f63() == 63 ? 0 : 1; }}\n"
    )).unwrap();
    let executable = fixture.0.join("caller");
    let build = Command::new("cc")
        .args(["-std=c11", "-O3"])
        .arg(fixture.0.join("library.c"))
        .arg(fixture.0.join("caller.c"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(Command::new(&executable).status().unwrap().success());
}

#[test]
fn closed_helper_specializations_preserve_body_verification() {
    let source = "fn id(x) { x } fn root(x: i32) -> i32 { if id(true) { id(x) } else { id(0) } }";
    let artifact = emit(source, &[export("root", "rill_root")]).unwrap();
    assert!(!artifact.c.contains("rl_value"));
    assert!(artifact.c.contains("int32_t rill_root(int32_t arg0)"));
    assert_c_output(
        &artifact,
        "#include \"library.h\"\nint main(void) { return rill_root(42) == 42 && rill_root(INT32_MIN) == INT32_MIN ? 0 : 1; }\n",
        b"",
    );
}
