use rill::{backend, parser};

#[test]
fn checked_reference_rejects_owners_even_in_inactive_code_or_types() {
    for source in [
        "fn main() { let owner = system_box_i32_abort(42); box_read(owner) }",
        "fn main() { let owner = system_bytes_zeroed_abort(1); owned_bytes_len(owner) }",
        "fn unused() { let owner = system_box_i32_abort(42); box_read(owner) } fn main() { 42 }",
        "fn main() { if false { let owner = system_box_i32_abort(42); box_read(owner) } else { 42 } }",
        "type Resource = Empty | Box(SystemBoxI32) fn main() { 42 }",
        "type Resource = Empty | Bytes(SystemBytes) fn main() { 42 }",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        assert!(checked.has_owned(), "{source}");
        assert_eq!(
            backend::emit_reference(&program).unwrap_err(),
            "E-REFERENCE: owner programs require the checked typed path",
            "{source}"
        );
        // The new oracle must not redirect valid owner programs away from the
        // existing typed production path.
        assert!(!backend::emit(&program).unwrap().contains("rl_value"));
    }
}

#[test]
fn checked_reference_validates_native_metadata_before_emission() {
    let program = parser::parse("extern fn native(value: i32) -> i32; fn main() { 42 }").unwrap();
    let mut bad_symbol = program.clone();
    bad_symbol.externs[0].name = "native; injected".into();
    let mut bad_pointer = program.clone();
    bad_pointer.externs[0].result = rill::ast::ExternType::Pointer {
        pointee: "Window; injected".into(),
        is_const: false,
    };
    let mut bad_params = program.clone();
    bad_params.externs[0]
        .params
        .push(program.externs[0].params[0].clone());
    let mut bad_include = program;
    bad_include
        .native
        .includes
        .push("api.h\n#error injected".into());
    for program in [bad_symbol, bad_pointer, bad_params, bad_include] {
        let expected = rill::check::analyze(&program).err().unwrap();
        assert_eq!(backend::emit_reference(&program).unwrap_err(), expected);
        assert_eq!(backend::emit(&program).unwrap_err(), expected);
    }
}

#[test]
fn unused_manual_prototypes_keep_exact_abi_types_names_and_order() {
    let cases = [
        ("i32", "int32_t"),
        ("bool", "_Bool"),
        ("u8", "uint8_t"),
        ("u32", "uint32_t"),
        ("u64", "uint64_t"),
        ("f32", "float"),
        ("ptr<Window>", "Window *"),
        ("const_ptr<Window>", "const Window *"),
        ("ptr<void>", "void *"),
        ("const_ptr<void>", "const void *"),
        ("ptr<i32>", "int32_t *"),
        ("const_ptr<u8>", "const uint8_t *"),
        ("ptr<u32>", "uint32_t *"),
        ("ptr<u64>", "uint64_t *"),
        ("ptr<f32>", "float *"),
        ("const_ptr<bool>", "const _Bool *"),
    ];
    let mut source = String::new();
    let mut expected = String::new();
    for (index, (rill, c)) in cases.iter().enumerate() {
        source.push_str(&format!(
            "extern fn native_{index}(value: {rill}) -> {rill};\n"
        ));
        expected.push_str(&format!("extern {c} native_{index}({c});\n"));
    }
    source.push_str(
        "extern fn text(value: cstr) -> unit; extern fn empty() -> unit; fn main() { 42 }",
    );
    expected.push_str("extern void text(const char *);\nextern void empty(void);\n");
    let program = parser::parse(&source).unwrap();
    let snapshot = program.clone();
    let c = backend::emit_reference(&program).unwrap();
    assert!(c.contains(&expected), "{c}");
    assert!(!c.contains("E-FFI: incompatible header"));
    let typed = backend::emit(&program).unwrap();
    assert!(!typed.contains("rl_value"));
    assert!(typed.contains(&expected));
    assert_eq!(program, snapshot);
}

#[test]
fn unused_header_assertions_and_dependency_metadata_are_not_pruned_or_reordered() {
    let program = parser::parse(
        r#"
@include("z.h");
@include("a.h");
@include("z.h");
@link("z");
@link("a");
@pkg_config("second");
@pkg_config("first");
extern fn zed(value: bool) -> unit;
extern fn alpha() -> ptr<Window>;
extern const BYTE: u8;
extern const WORD: u32;
extern const WIDE: u64;
extern const SIGNED: i32;
extern const FLAG: bool;
extern const FLOAT: f32;
extern const POINTER: const_ptr<Window>;
fn main() { 42 }
"#,
    )
    .unwrap();
    let snapshot = program.clone();
    let c = backend::emit_reference(&program).unwrap();
    assert!(c.starts_with("#include <z.h>\n#include <a.h>\n#include <z.h>\n#define RL_CAP"));
    let declarations: Vec<_> = c.lines().filter(|line| line.contains("E-FFI:")).collect();
    assert_eq!(declarations, [
        "_Static_assert(__builtin_types_compatible_p(__typeof__(&zed), void (*)(_Bool)), \"E-FFI: incompatible header prototype for zed\");",
        "_Static_assert(__builtin_types_compatible_p(__typeof__(&alpha), Window * (*)(void)), \"E-FFI: incompatible header prototype for alpha\");",
        "_Static_assert(__builtin_classify_type(BYTE) == 1 && (BYTE) >= 0 && (BYTE) <= UINT8_MAX, \"E-FFI: incompatible header constant BYTE\");",
        "_Static_assert(__builtin_classify_type(WORD) == 1 && (WORD) >= 0 && (WORD) <= UINT32_MAX, \"E-FFI: incompatible header constant WORD\");",
        "_Static_assert(__builtin_classify_type(WIDE) == 1 && (WIDE) >= 0 && (WIDE) <= UINT64_MAX, \"E-FFI: incompatible header constant WIDE\");",
        "_Static_assert(__builtin_classify_type(SIGNED) == 1 && ((SIGNED) >= 0 ? (SIGNED) <= INT32_MAX : (SIGNED) >= INT32_MIN), \"E-FFI: incompatible header constant SIGNED\");",
        "_Static_assert(__builtin_types_compatible_p(__typeof__(FLAG), _Bool), \"E-FFI: incompatible header constant FLAG\");",
        "_Static_assert(__builtin_types_compatible_p(__typeof__(FLOAT), float), \"E-FFI: incompatible header constant FLOAT\");",
        "_Static_assert(__builtin_types_compatible_p(__typeof__(POINTER), const Window *), \"E-FFI: incompatible header constant POINTER\");",
    ]);
    let typed = backend::emit(&program).unwrap();
    assert!(!typed.contains("rl_value"));
    assert!(typed.starts_with("#include <z.h>\n#include <a.h>\n#include <z.h>"));
    assert_eq!(
        typed
            .lines()
            .filter(|line| line.contains("E-FFI:"))
            .collect::<Vec<_>>(),
        declarations
    );
    assert_eq!(program, snapshot);
}

#[test]
fn raw_runtime_uses_checked_typed_path_and_keeps_independent_reference() {
    for source in [
        "fn main() { u32(0) }",
        "@include(\"stdint.h\"); extern const UINT64_MAX: u64; fn main() { UINT64_MAX }",
    ] {
        let program = parser::parse(source).unwrap();
        let c = backend::emit_reference(&program).unwrap();
        assert!(c.contains("rl_value"));
        let checked = rill::check::analyze(&program).unwrap();
        let typed = rill::typed::lower_main(&checked).unwrap();
        rill::typed::verify(&typed).unwrap();
        let typed_c = rill::typed::emit_c(&typed).unwrap();
        assert!(!typed_c.contains("rl_value"), "{source}");
        assert!(!typed_c.contains("RL_CAP"), "{source}");
        assert_eq!(typed_c, backend::emit(&program).unwrap(), "{source}");
        assert_ne!(c, typed_c, "{source}");
    }
}
