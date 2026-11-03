//! Adversarial checks at the independently verified native IR boundary.
use super::*;
use crate::{ast::ExternType, check, parser};

fn lower(source: &str) -> TypedMain {
    let p = parser::parse(source).unwrap();
    lower_main(&check::analyze(&p).unwrap()).unwrap()
}

fn sample() -> TypedMain {
    lower("@include(\"api.h\"); extern fn native(x: i32, flag: bool, text: cstr) -> i32; extern const FLAG: bool; fn main() { native(42, FLAG, c\"é\") }")
}

fn reject(program: &TypedMain) {
    let error = verify(program).unwrap_err();
    assert!(
        !error.starts_with("E-TYPED-UNSUPPORTED:"),
        "malformed IR cannot select fallback: {error}"
    );
    assert_eq!(emit_c(program).unwrap_err(), error);
    assert_eq!(evaluate(program).unwrap_err(), error);
}

#[test]
fn native_manifest_retains_all_declarations_and_configuration() {
    let source = "@include(\"z.h\"); @include(\"a.h\"); @link(\"second\"); @link(\"first\"); @pkg_config(\"one\"); extern fn unused(p: const_ptr<Window>, n: u64) -> f32; extern const UNUSED: u8; fn main() { 42 }";
    let p = parser::parse(source).unwrap();
    let ir = lower_main(&check::analyze(&p).unwrap()).unwrap();
    assert_eq!(ir.native.config, p.native);
    assert_eq!(ir.native.functions, p.externs);
    assert_eq!(ir.native.constants, p.constants);
    assert_eq!(evaluate(&ir).unwrap(), crate::interpreter::Value::Int(42));
    let c = emit_c(&ir).unwrap();
    assert!(c.starts_with("#include <z.h>\n#include <a.h>"));
    assert!(c.contains("float (*)(const Window *, uint64_t)"));
    assert!(c.contains("incompatible header constant UNUSED"));
    assert!(c.contains("sizeof(_Bool) <= 64"));
    assert!(!c.contains("rl_value"));
}

#[test]
fn rejects_native_ids_arity_operand_kinds_and_result_types() {
    let base = sample();
    let mut cases = Vec::new();
    let mut p = base.clone();
    p.functions[p.entry].body.kind = ExprKind::ForeignCall(ForeignFunctionId(u32::MAX), vec![]);
    cases.push(p);
    let mut p = base.clone();
    if let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind {
        args.pop();
    }
    cases.push(p);
    let mut p = base.clone();
    if let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind {
        args[0] = ForeignArg::StaticCStr("wrong".into());
    }
    cases.push(p);
    let mut p = base.clone();
    if let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind {
        args[2] = ForeignArg::Value(ExprNode {
            ty: Type::StaticStr,
            kind: ExprKind::Str("wrong".into()),
        });
    }
    cases.push(p);
    let mut p = base.clone();
    if let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind {
        args[2] = ForeignArg::StaticCStr("bad\0string".into());
    }
    cases.push(p);
    let mut p = base.clone();
    if let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind {
        args[1] = ForeignArg::Value(ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(1),
        });
    }
    cases.push(p);
    let mut p = base.clone();
    p.functions[p.entry].result = Type::Bool;
    p.functions[p.entry].body.ty = Type::Bool;
    cases.push(p);
    let mut p = base.clone();
    p.functions[p.entry].body.kind = ExprKind::ForeignConst(ForeignConstantId(u32::MAX));
    cases.push(p);
    let mut p = base;
    p.functions[p.entry].body.kind = ExprKind::ForeignConst(ForeignConstantId(0));
    cases.push(p); // The bool constant cannot be forged as the i32 function.
    for p in cases {
        reject(&p);
    }
}

#[test]
fn rejects_forged_native_metadata_even_when_unused() {
    let base = sample();
    let mut cases = Vec::new();
    for name in ["bad; injected", "rl_fn_0", "_reserved", "int", "main"] {
        let mut p = base.clone();
        p.native.functions[0].name = name.into();
        cases.push(p);
    }
    let mut p = base.clone();
    p.native.functions[0].params[0].ty = ExternType::Unit;
    cases.push(p);
    let mut p = base.clone();
    p.native.functions[0].result = ExternType::CStr;
    cases.push(p);
    let mut p = base.clone();
    p.native.functions[0].params[0].name = "bad name".into();
    cases.push(p);
    let mut p = base.clone();
    p.native.functions[0].params[1].name = "x".into();
    cases.push(p);
    let mut p = base.clone();
    p.native.functions.push(p.native.functions[0].clone());
    cases.push(p);
    let mut p = base.clone();
    p.native.constants[0].name = "native".into();
    cases.push(p);
    let mut p = base.clone();
    p.native.constants[0].ty = ExternType::Unit;
    cases.push(p);
    let mut p = base.clone();
    p.native.config.includes.clear();
    cases.push(p);
    let mut p = base.clone();
    p.native
        .config
        .includes
        .push("bad.h\n#error injected".into());
    cases.push(p);
    for config in [0, 1] {
        let mut p = base.clone();
        if config == 0 {
            p.native.config.links.push("-injected".into());
        } else {
            p.native.config.packages.push("bad;package".into());
        }
        cases.push(p);
    }
    let mut p = base;
    p.native.functions[0].params[0].ty = ExternType::Pointer {
        pointee: "bad *".into(),
        is_const: true,
    };
    cases.push(p);
    for mut p in cases {
        p.functions[p.entry].body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(42),
        };
        reject(&p);
    }
}

#[test]
fn unused_raw_abi_metadata_is_allowed_but_forged_runtime_nodes_are_not() {
    let mut p = lower("extern fn unused(x: u32) -> u32; fn main() { 42 }");
    p.functions[p.entry].body.kind = ExprKind::ForeignCall(
        ForeignFunctionId(0),
        vec![ForeignArg::Value(ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(42),
        })],
    );
    reject(&p);
    let mut p = lower("@include(\"api.h\"); extern const RAW: u64; fn main() { 42 }");
    p.functions[p.entry].body.kind = ExprKind::ForeignConst(ForeignConstantId(0));
    reject(&p);
}

#[test]
fn source_and_independent_ir_reject_owner_native_mixing() {
    for prefix in [
        "@include(\"api.h\");",
        "extern fn native() -> i32;",
        "@include(\"api.h\"); extern const FLAG: bool;",
    ] {
        let p = parser::parse(&format!(
            "{prefix} fn main() {{ let owner = system_box_i32_abort(1); box_read(owner) }}"
        ))
        .unwrap();
        assert!(check::analyze(&p).is_err());
    }
    let mut owner = lower("fn main() { let owner = system_box_i32_abort(1); box_read(owner) }");
    owner.native = sample().native;
    reject(&owner);
    let mut phantom = lower("type Unused = Hold(SystemBoxI32) fn main() { 42 }");
    // The lowerer omits unused source layouts; forge a real owner type registry
    // entry to ensure the independent restriction checks unused metadata too.
    phantom.adts.push(AdtType {
        origin: AdtOrigin::Source,
        record_fields: None,
        name: "Unused".into(),
        arguments: vec![],
        variants: vec![AdtVariant {
            name: "Hold".into(),
            fields: vec![Type::OwnedI32],
        }],
        owned: true,
    });
    phantom.native = sample().native;
    reject(&phantom);
}

#[test]
fn native_children_keep_enclosing_exclusive_and_shared_loans() {
    let mut p = lower("extern fn native(x: i32) -> i32; fn sink(inout x, y) { x <- y; } fn main() { let mut x = 1; sink(inout x, native(2)) }");
    let ExprKind::Let { local, body, .. } = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::Value(native) = &mut args[1] else {
        panic!()
    };
    let ExprKind::ForeignCall(_, args) = &mut native.kind else {
        panic!()
    };
    args[0] = ForeignArg::Value(ExprNode {
        ty: Type::Int,
        kind: ExprKind::Read(*local),
    });
    reject(&p);

    let mut p = lower("extern fn native(x: i32) -> i32; fn sink(read x, y) { x + y } fn main() { let mut x = 1; sink(x, native(2)) }");
    let ExprKind::Let { local, body, .. } = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::Value(native) = &mut args[1] else {
        panic!()
    };
    let ExprKind::ForeignCall(_, args) = &mut native.kind else {
        panic!()
    };
    args[0] = ForeignArg::Value(ExprNode {
        ty: Type::Int,
        kind: ExprKind::Let {
            local: LocalId(u32::MAX),
            mutable: false,
            value: Box::new(ExprNode {
                ty: Type::Unit,
                kind: ExprKind::Assign(
                    *local,
                    Box::new(ExprNode {
                        ty: Type::Int,
                        kind: ExprKind::Int(0),
                    }),
                ),
            }),
            body: Box::new(ExprNode {
                ty: Type::Int,
                kind: ExprKind::Int(2),
            }),
        },
    });
    reject(&p);
}

#[test]
fn hidden_owner_operations_inside_dead_native_arguments_cannot_evade_visitors() {
    let owner = lower("fn main() { let owner = system_box_i32_abort(7); box_read(owner) }");
    let mut p = sample();
    let ExprKind::ForeignCall(_, args) = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    args[0] = ForeignArg::Value(owner.functions[owner.entry].body.clone());
    let native = p.functions[p.entry].body.clone();
    p.functions[p.entry].body = ExprNode {
        ty: Type::Int,
        kind: ExprKind::If(
            Box::new(ExprNode {
                ty: Type::Bool,
                kind: ExprKind::Bool(false),
            }),
            Box::new(native),
            Box::new(ExprNode {
                ty: Type::Int,
                kind: ExprKind::Int(42),
            }),
        ),
    };
    assert!(verify(&p).unwrap_err().starts_with("E-TYPED-OWNER:"));
    reject(&p);
}

#[test]
fn dependency_flags_alone_do_not_widen_the_owner_exclusion() {
    let ir = lower("@link(\"m\"); @pkg_config(\"example\"); fn main() { let owner = system_box_i32_abort(42); box_read(owner) }");
    verify(&ir).unwrap();
    assert_eq!(evaluate(&ir).unwrap(), crate::interpreter::Value::Int(42));
    assert!(!emit_c(&ir).unwrap().contains("rl_value"));
}

fn pointer_sample() -> TypedMain {
    lower("extern fn first(p: ptr<Window>) -> ptr<Window>; extern fn second(p: const_ptr<Window>) -> const_ptr<Window>; extern fn third(p: ptr<Other>) -> ptr<Other>; fn main() { first(c_null()) == c_null() }")
}

#[test]
fn pointer_identity_is_exact_deterministic_and_slot_qualified() {
    let source = "extern fn unused(a: ptr<i32>, b: ptr<int32_t>, c: const_ptr<i32>) -> unit; fn identity(value) { value } fn inspect(read p: ptr<i32>) { is_null(p) } fn main() { let p = identity(c_null()); inspect(p) }";
    let a = lower(source);
    let b = lower(source);
    assert_eq!(a.native.pointers, b.native.pointers);
    assert_eq!(a.native.pointers.len(), 3);
    assert_eq!(a.native.pointers[0].pointee, "i32");
    assert_eq!(a.native.pointers[1].pointee, "int32_t");
    assert!(!a.native.pointers[0].is_const);
    assert!(a.native.pointers[2].is_const);
    let c = emit_c(&a).unwrap();
    assert!(c.contains("typedef int32_t * rl_cptr_0;"));
    assert!(c.contains("typedef int32_t * rl_cptr_1;"));
    assert!(c.contains("typedef const int32_t * rl_cptr_2;"));
    assert!(c.contains("const rl_cptr_0 * rl_l_"));
    assert!(!c.contains("const int32_t **"));
    assert!(!c.contains("restrict"));
    assert!(c.contains("sizeof(rl_cptr_0) <= 64"));
}

#[test]
fn rejects_forged_pointer_registry_and_distinct_pointer_equality() {
    let base = pointer_sample();
    let mut cases = Vec::new();
    let mut p = base.clone();
    p.native.pointers.clear();
    cases.push(p);
    let mut p = base.clone();
    p.native.pointers.push(p.native.pointers[0].clone());
    cases.push(p);
    for pointee in ["bad *", "rl_cptr_0", "return", "A; injected"] {
        let mut p = base.clone();
        p.native.pointers[0].pointee = pointee.into();
        p.native.functions[0].result = ExternType::Pointer {
            pointee: pointee.into(),
            is_const: false,
        };
        p.native.functions[0].params[0].ty = p.native.functions[0].result.clone();
        cases.push(p);
    }
    for wrong in [1, 2, u32::MAX] {
        let mut p = base.clone();
        let ExprKind::Binary(_, _, right) = &mut p.functions[p.entry].body.kind else {
            panic!()
        };
        right.ty = Type::RawPointer(PointerTypeId(wrong));
        cases.push(p);
    }
    for wrong in [1, 2, u32::MAX] {
        let mut p = base.clone();
        let ExprKind::Binary(_, left, _) = &mut p.functions[p.entry].body.kind else {
            panic!()
        };
        let ExprKind::ForeignCall(_, args) = &mut left.kind else {
            panic!()
        };
        let ForeignArg::Value(input) = &mut args[0] else {
            panic!()
        };
        input.ty = Type::RawPointer(PointerTypeId(wrong));
        cases.push(p);
    }
    for p in cases {
        reject(&p);
    }
}

#[test]
fn rejects_invalid_raw_types_in_unused_adt_arguments_fields_and_capture() {
    let invalid = Type::RawPointer(PointerTypeId(u32::MAX));
    let mut p = lower("type Rec = { field: i32 } fn main() { record Rec { field = 42 } }");
    p.adts[0].variants[0].fields[0] = invalid;
    reject(&p);
    let mut p = lower("@include(\"api.h\"); extern const POINTER: ptr<Window>; type M<a> = M(ptr<Window>) fn main() { let unused = M(POINTER); 42 }");
    let capture = p
        .adts
        .iter_mut()
        .find(|adt| adt.origin == AdtOrigin::LocalCapture)
        .expect("native constant capture");
    capture.arguments[0] = invalid;
    capture.variants[0].fields[0] = invalid;
    assert!(verify(&p).unwrap_err().contains("invalid pointer identity"));
    reject(&p);
    let mut p = lower("type Marker<a> = Empty fn main() { 42 }");
    p.adts.push(AdtType {
        origin: AdtOrigin::Source,
        record_fields: None,
        name: "Marker".into(),
        arguments: vec![invalid],
        variants: vec![AdtVariant {
            name: "Empty".into(),
            fields: vec![],
        }],
        owned: false,
    });
    reject(&p);
    let mut p = pointer_sample();
    p.functions.push(TypedFunction {
        params: vec![(LocalId(u32::MAX), invalid, ParamMode::Read)],
        result: Type::Bool,
        body: ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Bool(true),
        },
        param_cleanup: vec![],
    });
    reject(&p);
    let mut p = pointer_sample();
    p.functions.push(TypedFunction {
        params: vec![],
        result: invalid,
        body: ExprNode {
            ty: invalid,
            kind: ExprKind::NullPointer,
        },
        param_cleanup: vec![],
    });
    reject(&p);
}

#[test]
fn rejects_forged_conversion_null_and_raw_operators() {
    let mut cases = Vec::new();
    for (name, ty) in [
        ("u8", Type::CU8),
        ("u32", Type::CU32),
        ("u64", Type::CU64),
        ("f32", Type::CF32),
    ] {
        let base = lower(&format!("fn main() {{ {name}(7) }}"));
        assert_eq!(base.functions[base.entry].result, ty);
        let mut p = base.clone();
        let ExprKind::ForeignConvert(_, input) = &mut p.functions[p.entry].body.kind else {
            panic!()
        };
        **input = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Bool(true),
        };
        cases.push(p);
        let mut p = base;
        p.functions[p.entry].result = Type::Int;
        p.functions[p.entry].body.ty = Type::Int;
        cases.push(p);
    }
    let mut p = lower("fn main() { 42 }");
    p.functions[p.entry].body.kind = ExprKind::NullPointer;
    cases.push(p);
    let mut p = lower("fn main() { true }");
    p.functions[p.entry].body.kind = ExprKind::IsNull(Box::new(ExprNode {
        ty: Type::Int,
        kind: ExprKind::Int(0),
    }));
    cases.push(p);
    let mut p = lower("extern fn p() -> ptr<i32>; fn main() { is_null(p()) }");
    p.functions[p.entry].result = Type::Int;
    p.functions[p.entry].body.ty = Type::Int;
    cases.push(p);
    for op in [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Rem,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
    ] {
        for source in [
            "fn main() { u32(1) == u32(2) }",
            "fn main() { f32(1) == f32(2) }",
            "extern fn p() -> ptr<i32>; fn main() { p() == c_null() }",
        ] {
            let mut p = lower(source);
            let ExprKind::Binary(actual, _, _) = &mut p.functions[p.entry].body.kind else {
                panic!()
            };
            *actual = op;
            cases.push(p);
        }
    }
    for p in cases {
        reject(&p);
    }
}

#[test]
fn new_raw_visitors_do_not_hide_owner_children_or_native_mixing() {
    let owner = lower("fn main() { let owned = system_box_i32_abort(9); box_read(owned) }");
    let mut p = lower("fn main() { u32(1) }");
    let ExprKind::ForeignConvert(_, value) = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    **value = owner.functions[owner.entry].body.clone();
    assert!(verify(&p).unwrap_err().starts_with("E-TYPED-OWNER:"));
    reject(&p);
    let mut p = owner;
    p.adts.push(AdtType {
        origin: AdtOrigin::Source,
        record_fields: None,
        name: "RawPhantom".into(),
        arguments: vec![Type::CU8],
        variants: vec![AdtVariant {
            name: "RawEmpty".into(),
            fields: vec![],
        }],
        owned: false,
    });
    assert!(verify(&p).unwrap_err().starts_with("E-TYPED-OWNER:"));
    reject(&p);
}

#[test]
fn raw_children_respect_enclosing_rill_loans() {
    let mut p = lower(
        "fn sink(inout x, y) { x <- 0; y } fn main() { let mut x = 1; sink(inout x, u32(2)) }",
    );
    let ExprKind::Let { local, body, .. } = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::Value(value) = &mut args[1] else {
        panic!()
    };
    let ExprKind::ForeignConvert(_, input) = &mut value.kind else {
        panic!()
    };
    **input = ExprNode {
        ty: Type::Int,
        kind: ExprKind::Read(*local),
    };
    reject(&p);
}

#[test]
fn pointer_field_loans_reject_wrong_projection_and_forwarding_mode() {
    let source = "type Pair = { left: ptr<i32>; right: const_ptr<i32> } fn inspect(read p: ptr<i32>) { is_null(p) } fn main() { let mut pair = record Pair { left = c_null(); right = c_null() }; inspect(pair.left) }";
    let base = lower(source);
    for wrong in [1, usize::MAX] {
        let mut p = base.clone();
        let ExprKind::Let { body, .. } = &mut p.functions[p.entry].body.kind else {
            panic!()
        };
        let ExprKind::Call(_, args) = &mut body.kind else {
            panic!()
        };
        let CallArg::ReadField(place) = &mut args[0] else {
            panic!()
        };
        place.path[0].field = wrong;
        reject(&p);
    }
    let mut p = base.clone();
    let ExprKind::Let { body, .. } = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::ReadField(place) = &mut args[0] else {
        panic!()
    };
    place.path[0].record = usize::MAX;
    reject(&p);
    let mut p = base;
    let ExprKind::Let { body, .. } = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    let ExprKind::Call(_, args) = &mut body.kind else {
        panic!()
    };
    let CallArg::ReadField(place) = &args[0] else {
        panic!()
    };
    args[0] = CallArg::InoutField(place.clone());
    reject(&p);
}

#[test]
fn raw_operations_reject_before_operand_execution_but_still_verify_children() {
    for source in [
        "fn main() { u8(1 / 0) }",
        "fn main() { u32(2147483647 + 1) }",
        "fn main() { u64(1 / 0) }",
        "fn main() { f32(1 / 0) }",
        "fn get(p: ptr<i32>) { p } fn main() { is_null(get({ 1 / 0; c_null() })) }",
    ] {
        let p = parser::parse(source).unwrap();
        let ir = lower_main(&check::analyze(&p).unwrap()).unwrap();
        let expected = Err("E-FFI: raw C values require native C emission".to_string());
        assert_eq!(evaluate(&ir), expected, "{source}");
        assert_eq!(crate::interpreter::run_reference(&p), expected, "{source}");
    }
    let mut p = lower("fn main() { u32(1) }");
    let ExprKind::ForeignConvert(_, input) = &mut p.functions[p.entry].body.kind else {
        panic!()
    };
    input.kind = ExprKind::Read(LocalId(u32::MAX));
    reject(&p);
}

#[test]
fn pointer_registry_limit_checks_new_identities_but_reuses_exact_pairs() {
    let mut pointers = Vec::new();
    for id in 0..4096 {
        assert_eq!(
            foreign::intern_pointer(&mut pointers, &format!("Native{id}"), false).unwrap(),
            PointerTypeId(id)
        );
    }
    assert_eq!(
        foreign::intern_pointer(&mut pointers, "Native4095", false).unwrap(),
        PointerTypeId(4095)
    );
    assert!(foreign::intern_pointer(&mut pointers, "Native4095", true)
        .unwrap_err()
        .contains("excessive pointer registry"));
    assert!(foreign::intern_pointer(&mut pointers, "OneMore", false).is_err());
    assert_eq!(pointers.len(), 4096);
    let mut p = lower("fn main() { 42 }");
    p.native.pointers = pointers;
    verify(&p).unwrap();
    p.native.pointers.push(foreign::PointerType {
        pointee: "OneMore".into(),
        is_const: false,
    });
    reject(&p);
}
