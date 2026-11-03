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
