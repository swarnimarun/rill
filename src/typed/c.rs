//! C lowering for the provisional scalar, string, vector and finite-value IR.
// A terminal child has no temporary to consume. This macro is used only inside
// expression-emission routines with an EmitOutcome return contract.
macro_rules! emit_value {
    ($outcome:expr) => {
        match $outcome {
            EmitOutcome::Value(value) => value,
            EmitOutcome::NoNormal => return EmitOutcome::NoNormal,
        }
    };
}

mod adt;
mod allocation;
mod copy_match;
mod foreign;
mod records;
mod results;
mod vectors;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use super::{CallArg, ExprKind, ExprNode, ForeignArg, LocalId, OwnerInit, Type, TypedMain};
use crate::ast::{BinaryOp, ParamMode};

const RUNTIME: &str = r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#if defined(RILL_SIMD_NEON) && (!defined(__aarch64__) || !defined(__ARM_NEON))
#error "RILL_SIMD_NEON requires an AArch64 target with NEON"
#endif
#if defined(RILL_SIMD_NEON)
#include <arm_neon.h>
#endif
typedef struct { unsigned char unused; } rl_unit;
typedef struct { const unsigned char *data; size_t len; } rl_str;
_Static_assert(CHAR_BIT == 8, "Rill strings require 8-bit bytes");
_Noreturn static void rl_trap(void) { fputs("rill: runtime trap\n", stderr); exit(1); }
static unsigned rl_call_depth;
static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }
static void rl_leave(void) { --rl_call_depth; }
static int32_t rl_byte_len(rl_str s) { if (s.len > INT32_MAX) rl_trap(); return (int32_t)s.len; }
static int32_t rl_byte_at(rl_str s, int32_t i) { if (i < 0 || (size_t)i >= s.len) rl_trap(); return (int32_t)s.data[i]; }
static int32_t rl_checked(int op, int32_t x, int32_t y) {
    int64_t n = 0;
    switch (op) {
    case 0: n = (int64_t)x + y; break;
    case 1: n = (int64_t)x - y; break;
    case 2: n = (int64_t)x * y; break;
    case 3: if (!y || (x == INT32_MIN && y == -1)) rl_trap(); n = x / y; break;
    case 4: if (!y || (x == INT32_MIN && y == -1)) rl_trap(); n = x % y; break;
    default: rl_trap();
    }
    if (n < INT32_MIN || n > INT32_MAX) rl_trap();
    return (int32_t)n;
}
"#;

/// Emit C for a structurally validated program in this IR subset. The main backend calls
/// this after `lower_main`; this verifier does not replace source checking.
pub fn emit_c(program: &TypedMain) -> Result<String, String> {
    super::verify(program)?;
    emit_verified(program)
}

fn emit_verified(program: &TypedMain) -> Result<String, String> {
    let mut out = program.native.config.preamble()?;
    out.push_str(RUNTIME);
    crate::c_import::abi::emit_declarations(
        &program.native.config,
        &program.native.functions,
        &program.native.constants,
        &mut out,
    );
    for (id, pointer) in program.native.pointers.iter().enumerate() {
        let pointer_type = crate::c_import::abi::c_type(&crate::ast::ExternType::Pointer {
            pointee: pointer.pointee.clone(),
            is_const: pointer.is_const,
        });
        // A read loan qualifies the pointer SLOT, not its foreign pointee.
        writeln!(out, "typedef {pointer_type} rl_cptr_{id};").unwrap();
        writeln!(
            out,
            "_Static_assert(sizeof(rl_cptr_{id}) <= 64, \"unsupported typed pointer layout\");"
        )
        .unwrap();
    }
    let mut emitter = Emitter {
        out,
        next: 0,
        frame_cost: 0,
        program,
        owners: HashMap::new(),
        borrowed_copy: HashSet::new(),
        adt_costs: super::adts::layout_costs(&program.adts)?,
    };
    emitter.out.push_str("_Static_assert(sizeof(int) <= 64 && sizeof(_Bool) <= 64 && sizeof(int32_t) <= 64 && sizeof(rl_unit) <= 64 && sizeof(rl_str) <= 64 && sizeof(uint8_t) <= 64 && sizeof(uint32_t) <= 64 && sizeof(uint64_t) <= 64 && sizeof(float) <= 64, \"unsupported typed value layout\");\n");
    if program.adts.iter().any(|adt| adt.owned)
        || program.functions.iter().any(|function| {
            matches!(function.result, super::ResultRepr::Value(ty) if ty.is_owner())
                || function.params.iter().any(|(_, ty, _)| ty.is_owner())
                || match &function.body {
                    super::Computation::Value(value) => contains_owner(value),
                    super::Computation::NoNormal(value) => {
                        value.any_value_root(&mut contains_owner)
                    }
                }
        })
    {
        emitter.out.push_str(allocation::RUNTIME);
    }
    let mut capacities = BTreeSet::new();
    for function in &program.functions {
        if let super::ResultRepr::Value(ty) = function.result {
            collect_type(ty, &mut capacities);
        }
        for (_, ty, _) in &function.params {
            collect_type(*ty, &mut capacities);
        }
        match &function.body {
            super::Computation::Value(value) => collect_expr(value, &mut capacities),
            super::Computation::NoNormal(value) => {
                value.any_value_root(&mut |value| {
                    collect_expr(value, &mut capacities);
                    false
                });
            }
        }
    }
    for adt in &program.adts {
        for variant in &adt.variants {
            for ty in &variant.fields {
                collect_type(*ty, &mut capacities);
            }
        }
    }
    for n in capacities {
        let storage = n.max(1);
        writeln!(
            emitter.out,
            "typedef struct {{ unsigned char data[{storage}]; }} rl_bytes_{n};"
        )
        .unwrap();
        writeln!(
            emitter.out,
            "_Static_assert(sizeof(rl_bytes_{n}) <= {}, \"unsupported byte value layout\");",
            n.max(64)
        )
        .unwrap();
        writeln!(emitter.out, "static int32_t rl_bytes_{n}_get(rl_bytes_{n} b, int32_t i) {{ if (i < 0 || i >= {n}) rl_trap(); return b.data[i]; }}").unwrap();
        writeln!(emitter.out, "static rl_bytes_{n} rl_bytes_{n}_set(rl_bytes_{n} b, int32_t i, int32_t v) {{ if (i < 0 || i >= {n} || v < 0 || v > 255) rl_trap(); b.data[i] = (unsigned char)v; return b; }}").unwrap();
    }
    emitter.out.push_str(vectors::RUNTIME);
    adt::layouts(&mut emitter);
    adt::printers(&mut emitter);
    for (id, function) in program.functions.iter().enumerate() {
        write!(
            emitter.out,
            "\nstatic {} rl_fn_{id}(",
            result_type(&function.result)
        )
        .unwrap();
        if function.params.is_empty() {
            emitter.out.push_str("void");
        }
        for (i, (local, ty, mode)) in function.params.iter().enumerate() {
            if i > 0 {
                emitter.out.push_str(", ");
            }
            let param_type = parameter_type(*ty, *mode);
            write!(emitter.out, "{param_type} rl_l_{}", local.0).unwrap();
        }
        emitter.out.push_str(");\n");
    }
    for (id, function) in program.functions.iter().enumerate() {
        emitter.owners.clear();
        emitter.borrowed_copy.clear();
        for (local, ty, mode) in &function.params {
            if !ty.is_owner() && *mode != ParamMode::Value {
                emitter.borrowed_copy.insert(*local);
            }
            if ty.is_owner() {
                emitter.owners.insert(*local, *ty);
            }
        }
        write!(
            emitter.out,
            "\nstatic {} rl_fn_{id}(",
            result_type(&function.result)
        )
        .unwrap();
        if function.params.is_empty() {
            emitter.out.push_str("void");
        }
        for (i, (local, ty, mode)) in function.params.iter().enumerate() {
            if i > 0 {
                emitter.out.push_str(", ");
            }
            let param_type = parameter_type(*ty, *mode);
            write!(emitter.out, "{param_type} rl_l_{}", local.0).unwrap();
        }
        emitter.out.push_str(") {\nrl_enter();\n");
        emitter.frame_cost = 8 * 64;
        for (_, ty, _) in &function.params {
            emitter.add_cost(*ty);
            emitter.add_cost(*ty);
        }
        let result = match &function.body {
            super::Computation::Value(value) => emitter.expr(value),
            super::Computation::NoNormal(value) => emitter.no_normal(value),
        };
        if emitter.frame_cost > 1024 * 1024 {
            return Err("E-C-STACK: estimated typed frame exceeds 1 MiB limit".into());
        }
        match result {
            EmitOutcome::Value(result) => {
                for local in &function.param_cleanup {
                    emitter.drop_cleanup(local);
                }
                writeln!(emitter.out, "rl_leave(); return {result};\n}}").unwrap();
            }
            EmitOutcome::NoNormal => {
                // An impossible normal return must trap, never manufacture the
                // function's declared value or discharge a frame early.
                emitter.out.push_str("rl_trap();\n}\n");
            }
        }
    }
    let entry = &program.functions[program.entry];
    if matches!(entry.result, super::ResultRepr::NoNormal(_)) {
        writeln!(
            emitter.out,
            "\nint main(void) {{ rl_fn_{}(); rl_trap(); }}",
            program.entry
        )
        .unwrap();
        return Ok(emitter.out);
    }
    writeln!(
        emitter.out,
        "\nint main(void) {{ {} result = rl_fn_{}();",
        c_type(entry.result.value()?),
        program.entry
    )
    .unwrap();
    match entry.result.value()? {
        Type::CU8 | Type::CU32 | Type::CU64 | Type::CF32 | Type::RawPointer(_) => {
            adt::print_value(&mut emitter, entry.result.value()?, "result");
            emitter.out.push_str("fputc('\\n', stdout);\n");
        }
        Type::Int => emitter
            .out
            .push_str("printf(\"%\" PRId32 \"\\n\", result);\n"),
        Type::Bool => emitter
            .out
            .push_str("puts(result ? \"true\" : \"false\");\n"),
        Type::Unit => emitter.out.push_str("(void)result; puts(\"()\");\n"),
        Type::StaticStr => emitter
            .out
            .push_str("fwrite(result.data, 1, result.len, stdout); fputc('\\n', stdout);\n"),
        Type::U8x16 => emitter.out.push_str("rl_print_vector(result); fputc('\\n', stdout);\n"),
        Type::Adt{id,..} => writeln!(emitter.out,"rl_print_adt_{id}(&result); fputc('\\n', stdout);").unwrap(),
        Type::OwnedI32 | Type::OwnedBytes => return Err("E-TYPED-OWNER: owner result cannot escape".into()),
        Type::Bytes(n) => writeln!(emitter.out, "fputs(\"bytes[\", stdout); for (size_t i = 0; i < {n}; ++i) {{ if (i) fputs(\", \", stdout); printf(\"%u\", (unsigned)result.data[i]); }} puts(\"]\");").unwrap(),
    }
    emitter.out.push_str("return 0;\n}\n");
    Ok(emitter.out)
}

fn result_type(result: &super::ResultRepr) -> String {
    match result {
        super::ResultRepr::Value(ty) => c_type(*ty),
        super::ResultRepr::NoNormal(_) => "void".into(),
    }
}
fn c_type(ty: Type) -> String {
    match ty {
        Type::Int => "int32_t".into(),
        Type::Bool => "int".into(),
        Type::Unit => "rl_unit".into(),
        Type::StaticStr => "rl_str".into(),
        Type::CU8 => "uint8_t".into(),
        Type::CU32 => "uint32_t".into(),
        Type::CU64 => "uint64_t".into(),
        Type::CF32 => "float".into(),
        Type::RawPointer(id) => format!("rl_cptr_{}", id.0),
        Type::U8x16 => "rl_u8x16".into(),
        Type::Bytes(n) => format!("rl_bytes_{n}"),
        Type::OwnedI32 => "int32_t *".into(),
        Type::OwnedBytes => "rl_owned_bytes *".into(),
        Type::Adt { id, .. } => format!("rl_adt_{id}"),
    }
}
fn parameter_type(ty: Type, mode: ParamMode) -> String {
    match mode {
        ParamMode::Read if !ty.is_owner() => format!("const {} *", c_type(ty)),
        ParamMode::Inout if !ty.is_owner() => format!("{} *", c_type(ty)),
        ParamMode::Read => format!("const {}", c_type(ty)),
        _ => c_type(ty),
    }
}
fn type_cost(ty: Type, costs: &[usize]) -> usize {
    match ty {
        Type::Bytes(n) => n.max(64),
        Type::Adt { id, .. } => costs[id],
        _ => 64,
    }
}

fn collect_type(ty: Type, output: &mut BTreeSet<usize>) {
    if let Type::Bytes(n) = ty {
        output.insert(n);
    }
}
fn collect_expr(expr: &ExprNode, output: &mut BTreeSet<usize>) {
    collect_type(expr.ty, output);
    match &expr.kind {
        ExprKind::InterruptedCall(call) => {
            call.any_value_root(&mut |value| {
                collect_expr(value, output);
                false
            });
        }
        ExprKind::NoStorageLet { value, body, .. } => {
            value.any_value_root(&mut |value| {
                collect_expr(value, output);
                false
            });
            collect_expr(body, output);
        }

        ExprKind::ForeignCall(_, args) => {
            for arg in args {
                if let ForeignArg::Value(value) = arg {
                    collect_expr(value, output);
                }
            }
        }
        ExprKind::Vector(_, args) => {
            for arg in args {
                collect_expr(arg, output);
            }
        }
        ExprKind::TryAlloc { input, .. } => collect_expr(input, output),
        ExprKind::CopyMatch { value, arms } => {
            collect_expr(value, output);
            for arm in arms {
                collect_expr(&arm.body, output);
            }
        }
        ExprKind::Record { fields, .. } => {
            for (_, field) in fields {
                collect_expr(field, output);
            }
        }
        ExprKind::FieldAssign(_, value) => collect_expr(value, output),
        ExprKind::FieldRead(_) => {}
        ExprKind::Constructor { fields, .. } => {
            for field in fields {
                collect_expr(field, output);
            }
        }
        ExprKind::Match { value, arms } => {
            collect_expr(value, output);
            for arm in arms {
                collect_expr(&arm.body, output);
            }
        }
        ExprKind::OwnerLet { init, body, .. } => {
            if let OwnerInit::Alloc(value)
            | OwnerInit::BytesAlloc(value)
            | OwnerInit::Call(value)
            | OwnerInit::Expr(value) = init
            {
                collect_expr(value, output);
            }
            collect_expr(body, output);
        }
        ExprKind::OwnerWrite(_, value) | ExprKind::OwnerBytesGet(_, value) => {
            collect_expr(value, output)
        }
        ExprKind::OwnerBytesSet(_, a, b) => {
            collect_expr(a, output);
            collect_expr(b, output);
        }
        ExprKind::OwnerRead(_) | ExprKind::OwnerBytesLen(_) | ExprKind::OwnerMove(_) => {}
        ExprKind::Call(_, args) => {
            for arg in args {
                if let CallArg::Value(value) | CallArg::ReadTemporary(value) = arg {
                    collect_expr(value, output);
                }
            }
        }
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => {
            collect_expr(value, output);
            collect_expr(body, output);
        }
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            collect_expr(a, output);
            collect_expr(b, output);
            collect_expr(c, output);
        }
        ExprKind::ForeignConvert(_, value)
        | ExprKind::IsNull(value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => collect_expr(value, output),
        ExprKind::NullPointer
        | ExprKind::ForeignConst(_)
        | ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Unit
        | ExprKind::Read(_)
        | ExprKind::BytesNew(_) => {}
    }
}

fn contains_owner(expr: &ExprNode) -> bool {
    if expr.ty.is_owner() {
        return true;
    }
    match &expr.kind {
        ExprKind::InterruptedCall(call) => call.any_value_root(&mut contains_owner),
        ExprKind::NoStorageLet { value, body, .. } => {
            value.any_value_root(&mut contains_owner) || contains_owner(body)
        }

        ExprKind::ForeignCall(_, args) => args
            .iter()
            .any(|arg| matches!(arg, ForeignArg::Value(value) if contains_owner(value))),
        ExprKind::Vector(_, args) => args.iter().any(contains_owner),
        ExprKind::CopyMatch { value, arms } => {
            contains_owner(value) || arms.iter().any(|a| contains_owner(&a.body))
        }
        ExprKind::Constructor { fields, .. } => fields.iter().any(contains_owner),
        ExprKind::Match { value, arms } => {
            contains_owner(value) || arms.iter().any(|a| contains_owner(&a.body))
        }
        ExprKind::OwnerLet { .. }
        | ExprKind::OwnerMove(_)
        | ExprKind::OwnerRead(_)
        | ExprKind::OwnerWrite(_, _)
        | ExprKind::OwnerBytesLen(_)
        | ExprKind::OwnerBytesGet(..)
        | ExprKind::OwnerBytesSet(..) => true,
        ExprKind::Record { fields, .. } => fields.iter().any(|(_, field)| contains_owner(field)),
        ExprKind::FieldAssign(_, value) => contains_owner(value),
        ExprKind::Call(_, args) => args.iter().any(|arg| match arg {
            CallArg::Value(value) | CallArg::ReadTemporary(value) => contains_owner(value),
            CallArg::ReadCopy(_)
            | CallArg::InoutCopy(_)
            | CallArg::ReadField(_)
            | CallArg::InoutField(_) => false,
            CallArg::Move(_) | CallArg::ReadOwner(_) | CallArg::InoutOwner(_) => true,
        }),
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => contains_owner(value) || contains_owner(body),
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            contains_owner(a) || contains_owner(b) || contains_owner(c)
        }
        ExprKind::ForeignConvert(_, value)
        | ExprKind::IsNull(value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => contains_owner(value),
        _ => false,
    }
}

/// Compile-time emission outcome, never a runtime Rill value.
#[derive(Debug)]
enum EmitOutcome {
    Value(String),
    NoNormal,
}

struct Emitter<'a> {
    out: String,
    next: usize,
    frame_cost: usize,
    program: &'a TypedMain,
    owners: HashMap<LocalId, Type>,
    borrowed_copy: HashSet<LocalId>,
    adt_costs: Vec<usize>,
}
impl Emitter<'_> {
    fn copy_place(&self, id: LocalId) -> String {
        if self.borrowed_copy.contains(&id) {
            format!("(*rl_l_{})", id.0)
        } else {
            format!("rl_l_{}", id.0)
        }
    }
    fn add_cost(&mut self, ty: Type) {
        self.frame_cost = self
            .frame_cost
            .saturating_add(type_cost(ty, &self.adt_costs));
    }
    fn temp(&mut self) -> String {
        let name = format!("rl_t_{}", self.next);
        self.next += 1;
        name
    }
    fn call_arguments(&mut self, id: usize, args: &[CallArg]) -> Option<Vec<String>> {
        let mut values = Vec::new();
        for (arg, (_, arg_ty, mode)) in args.iter().zip(&self.program.functions[id].params) {
            match arg {
                CallArg::Value(arg) => {
                    values.push(match self.expr(arg) {
                        EmitOutcome::Value(value) => value,
                        EmitOutcome::NoNormal => return None,
                    });
                    self.add_cost(arg.ty);
                }
                CallArg::ReadCopy(local) | CallArg::InoutCopy(local) => {
                    let temporary = self.temp();
                    self.add_cost(*arg_ty);
                    let param_ty = parameter_type(*arg_ty, *mode);
                    let place = self.copy_place(*local);
                    writeln!(self.out, "{param_ty} {temporary} = &({place});").unwrap();
                    values.push(temporary);
                }
                CallArg::ReadField(place) | CallArg::InoutField(place) => {
                    let temporary = self.temp();
                    self.add_cost(*arg_ty);
                    let param_ty = parameter_type(*arg_ty, *mode);
                    let place = self.field_place(place);
                    writeln!(self.out, "{param_ty} {temporary} = &({place});").unwrap();
                    values.push(temporary);
                }
                CallArg::ReadTemporary(value) => {
                    let value = match self.expr(value) {
                        EmitOutcome::Value(value) => value,
                        EmitOutcome::NoNormal => return None,
                    };
                    let temporary = self.temp();
                    self.add_cost(*arg_ty);
                    let param_ty = parameter_type(*arg_ty, *mode);
                    writeln!(self.out, "{param_ty} {temporary} = &{value};").unwrap();
                    values.push(temporary);
                }
                CallArg::ReadOwner(local) | CallArg::InoutOwner(local) => {
                    let temporary = self.temp();
                    self.add_cost(*arg_ty);
                    let param_ty = parameter_type(*arg_ty, *mode);
                    writeln!(self.out, "{param_ty} {temporary} = rl_l_{};", local.0).unwrap();
                    values.push(temporary);
                }
                CallArg::Move(local) => {
                    let temporary = self.temp();
                    self.add_cost(*arg_ty);
                    let owner_ty = c_type(*arg_ty);
                    writeln!(
                        self.out,
                        "{owner_ty} {temporary} = rl_l_{}; rl_l_{} = {};",
                        local.0,
                        local.0,
                        adt::zero(*arg_ty)
                    )
                    .unwrap();
                    values.push(temporary);
                }
            }
        }
        Some(values)
    }
    fn expr(&mut self, expr: &ExprNode) -> EmitOutcome {
        // A no-storage sequence keeps its concrete source type but has no
        // normal emitted result. Do not allocate/name/cost a fictitious slot.
        if let ExprKind::InterruptedCall(call) = &expr.kind {
            return self.interrupted(call);
        }
        if let ExprKind::NoStorageLet { value, .. } = &expr.kind {
            return self.no_normal(value);
        }
        let result = self.temp();
        let ty = c_type(expr.ty);
        self.add_cost(expr.ty);
        match &expr.kind {
            ExprKind::InterruptedCall(_) => unreachable!("handled without a result slot"),
            ExprKind::NoStorageLet { .. } => unreachable!("handled without a result slot"),

            ExprKind::ForeignCall(id, args) => return self.foreign_call(&result, *id, args),
            ExprKind::ForeignConst(id) => self.foreign_constant(&result, *id),
            ExprKind::ForeignConvert(conversion, input) => {
                return self.foreign_convert(&result, &ty, *conversion, input);
            }
            ExprKind::NullPointer => writeln!(self.out, "{ty} {result} = NULL;").unwrap(),
            ExprKind::IsNull(input) => {
                let input = emit_value!(self.expr(input));
                writeln!(self.out, "int {result} = ({input} == NULL);").unwrap();
            }
            ExprKind::Vector(op, args) => return self.vector(&result, &ty, *op, args),
            ExprKind::Record { fields, .. } => return self.record(&result, &ty, fields),
            ExprKind::FieldRead(place) => {
                let value = self.field_place(place);
                writeln!(self.out, "{ty} {result} = {value};").unwrap();
            }
            ExprKind::FieldAssign(place, value) => {
                let value = emit_value!(self.expr(value));
                let place = self.field_place(place);
                writeln!(
                    self.out,
                    "{place} = {value}; {ty} {result} = (rl_unit){{0}};"
                )
                .unwrap();
            }
            ExprKind::TryAlloc {
                kind,
                input,
                layout,
            } => return self.try_allocation(&result, *kind, input, layout),
            ExprKind::CopyMatch { value, arms } => {
                return self.match_copy(&result, &ty, value, arms)
            }
            ExprKind::Constructor {
                adt,
                variant,
                fields,
            } => return self.constructor(&result, &ty, *adt, *variant, fields),
            ExprKind::Match { value, arms } => return self.match_value(&result, &ty, value, arms),
            ExprKind::Int(n) => writeln!(self.out, "{ty} {result} = INT32_C({n});").unwrap(),
            ExprKind::Str(value) => {
                let mut literal = String::from("\"");
                for byte in value.as_bytes() {
                    write!(literal, "\\{byte:03o}").unwrap();
                }
                literal.push('"');
                writeln!(
                    self.out,
                    "{ty} {result} = (rl_str){{(const unsigned char *){literal}, {}}};",
                    value.len()
                )
                .unwrap();
            }
            ExprKind::BytesNew(n) => {
                writeln!(self.out, "{ty} {result} = (rl_bytes_{n}){{0}};").unwrap()
            }
            ExprKind::BytesLen(value) => {
                let Type::Bytes(n) = value.ty else {
                    unreachable!()
                };
                let value = emit_value!(self.expr(value));
                writeln!(self.out, "(void){value}; {ty} {result} = INT32_C({n});").unwrap();
            }
            ExprKind::BytesGet(value, index) => {
                let Type::Bytes(n) = value.ty else {
                    unreachable!()
                };
                let value = emit_value!(self.expr(value));
                let index = emit_value!(self.expr(index));
                writeln!(
                    self.out,
                    "{ty} {result} = rl_bytes_{n}_get({value}, {index});"
                )
                .unwrap();
            }
            ExprKind::BytesSet(value, index, byte) => {
                let Type::Bytes(n) = value.ty else {
                    unreachable!()
                };
                let value = emit_value!(self.expr(value));
                let index = emit_value!(self.expr(index));
                let byte = emit_value!(self.expr(byte));
                writeln!(
                    self.out,
                    "{ty} {result} = rl_bytes_{n}_set({value}, {index}, {byte});"
                )
                .unwrap();
            }
            ExprKind::ByteLen(value) => {
                let value = emit_value!(self.expr(value));
                writeln!(self.out, "{ty} {result} = rl_byte_len({value});").unwrap();
            }
            ExprKind::ByteAt(value, index) => {
                let value = emit_value!(self.expr(value));
                let index = emit_value!(self.expr(index));
                writeln!(self.out, "{ty} {result} = rl_byte_at({value}, {index});").unwrap();
            }
            ExprKind::Bool(value) => {
                writeln!(self.out, "{ty} {result} = {};", i32::from(*value)).unwrap()
            }
            ExprKind::Unit => writeln!(self.out, "{ty} {result} = (rl_unit){{0}};").unwrap(),
            ExprKind::Read(id) => {
                let place = self.copy_place(*id);
                writeln!(self.out, "{ty} {result} = {place};").unwrap();
            }
            ExprKind::Call(id, args) => {
                let Some(values) = self.call_arguments(*id, args) else {
                    return EmitOutcome::NoNormal;
                };
                writeln!(
                    self.out,
                    "{ty} {result} = rl_fn_{id}({});",
                    values.join(", ")
                )
                .unwrap();
            }
            ExprKind::OwnerLet {
                local,
                init,
                body,
                cleanup,
                ..
            } => {
                let owner_ty = match init {
                    OwnerInit::Alloc(_) => Type::OwnedI32,
                    OwnerInit::BytesAlloc(_) => Type::OwnedBytes,
                    OwnerInit::Move(source) => self.owners[source],
                    OwnerInit::Call(value) | OwnerInit::Expr(value) => value.ty,
                };
                self.owners.insert(*local, owner_ty);
                self.add_cost(owner_ty);
                match init {
                    OwnerInit::Alloc(value) => {
                        let value = emit_value!(self.expr(value));
                        writeln!(
                            self.out,
                            "int32_t *rl_l_{} = rl_box_i32_abort({value});",
                            local.0
                        )
                        .unwrap();
                    }
                    OwnerInit::BytesAlloc(value) => {
                        let value = emit_value!(self.expr(value));
                        writeln!(
                            self.out,
                            "rl_owned_bytes *rl_l_{} = rl_owned_bytes_new({value});",
                            local.0
                        )
                        .unwrap();
                    }
                    OwnerInit::Call(value) | OwnerInit::Expr(value) => {
                        let value = emit_value!(self.expr(value));
                        let owner_ty = c_type(owner_ty);
                        writeln!(self.out, "{owner_ty} rl_l_{} = {value};", local.0).unwrap();
                    }
                    OwnerInit::Move(source) => {
                        let owner_ty = c_type(owner_ty);
                        writeln!(
                            self.out,
                            "{owner_ty} rl_l_{} = rl_l_{}; rl_l_{} = {};",
                            local.0,
                            source.0,
                            source.0,
                            adt::zero(self.owners[source])
                        )
                        .unwrap();
                    }
                }
                let value = emit_value!(self.expr(body));
                if let Some(id) = cleanup {
                    self.drop_cleanup(id);
                }
                writeln!(self.out, "{ty} {result} = {value};").unwrap();
            }
            ExprKind::OwnerMove(local) => {
                writeln!(
                    self.out,
                    "{ty} {result} = rl_l_{}; rl_l_{} = {};",
                    local.0,
                    local.0,
                    adt::zero(expr.ty)
                )
                .unwrap();
            }
            ExprKind::OwnerRead(local) => {
                writeln!(self.out, "{ty} {result} = *rl_l_{};", local.0).unwrap()
            }
            ExprKind::OwnerWrite(local, value) => {
                let value = emit_value!(self.expr(value));
                writeln!(
                    self.out,
                    "*rl_l_{} = {value}; {ty} {result} = (rl_unit){{0}};",
                    local.0
                )
                .unwrap();
            }
            ExprKind::OwnerBytesLen(local) => {
                writeln!(self.out, "{ty} {result} = (int32_t)rl_l_{}->len;", local.0).unwrap();
            }
            ExprKind::OwnerBytesGet(local, index) => {
                let index = emit_value!(self.expr(index));
                writeln!(
                    self.out,
                    "{ty} {result} = rl_owned_bytes_get(rl_l_{}, {index});",
                    local.0
                )
                .unwrap();
            }
            ExprKind::OwnerBytesSet(local, index, value) => {
                let index = emit_value!(self.expr(index));
                let value = emit_value!(self.expr(value));
                writeln!(self.out, "rl_owned_bytes_set(rl_l_{}, {index}, {value}); {ty} {result} = (rl_unit){{0}};", local.0).unwrap();
            }
            ExprKind::Let {
                local, value, body, ..
            } => {
                let value_temp = emit_value!(self.expr(value));
                self.add_cost(value.ty);
                writeln!(
                    self.out,
                    "{} rl_l_{} = {value_temp};",
                    c_type(value.ty),
                    local.0
                )
                .unwrap();
                let body_temp = emit_value!(self.expr(body));
                writeln!(self.out, "{ty} {result} = {body_temp};").unwrap();
            }
            ExprKind::Assign(id, value) => {
                let value_temp = emit_value!(self.expr(value));
                let place = self.copy_place(*id);
                writeln!(
                    self.out,
                    "{place} = {value_temp}; {ty} {result} = (rl_unit){{0}};"
                )
                .unwrap();
            }
            ExprKind::If(condition, yes, no) => {
                let condition = emit_value!(self.expr(condition));
                writeln!(self.out, "{ty} {result}; if ({condition}) {{").unwrap();
                let yes = self.expr(yes);
                if let EmitOutcome::Value(value) = &yes {
                    writeln!(self.out, "{result} = {value};").unwrap();
                }
                self.out.push_str("} else {\n");
                let no = self.expr(no);
                if let EmitOutcome::Value(value) = &no {
                    writeln!(self.out, "{result} = {value};").unwrap();
                }
                self.out.push_str("}\n");
                if matches!((yes, no), (EmitOutcome::NoNormal, EmitOutcome::NoNormal)) {
                    return EmitOutcome::NoNormal;
                }
            }
            ExprKind::While(condition, body) => {
                self.out.push_str("for (;;) {\n");
                let condition = match self.expr(condition) {
                    EmitOutcome::Value(condition) => condition,
                    EmitOutcome::NoNormal => {
                        self.out.push_str("}\n");
                        return EmitOutcome::NoNormal;
                    }
                };
                writeln!(self.out, "if (!{condition}) break;").unwrap();
                if let EmitOutcome::Value(body) = self.expr(body) {
                    writeln!(self.out, "(void){body};").unwrap();
                }
                // The condition's false edge is a genuine normal predecessor,
                // even when every entered iteration has no normal successor.
                writeln!(self.out, "}} {ty} {result} = (rl_unit){{0}};").unwrap();
            }
            ExprKind::Binary(op, left, right) => {
                let left_temp = emit_value!(self.expr(left));
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let condition = if *op == BinaryOp::And {
                        left_temp.clone()
                    } else {
                        format!("!{left_temp}")
                    };
                    writeln!(self.out, "{ty} {result}; if ({condition}) {{").unwrap();
                    if let EmitOutcome::Value(right_temp) = self.expr(right) {
                        writeln!(self.out, "{result} = {right_temp};").unwrap();
                    }
                    writeln!(
                        self.out,
                        "}} else {{ {result} = {}; }}",
                        i32::from(*op == BinaryOp::Or)
                    )
                    .unwrap();
                } else {
                    let right_temp = emit_value!(self.expr(right));
                    let expression = match op {
                        BinaryOp::Add => format!("rl_checked(0, {left_temp}, {right_temp})"),
                        BinaryOp::Sub => format!("rl_checked(1, {left_temp}, {right_temp})"),
                        BinaryOp::Mul => format!("rl_checked(2, {left_temp}, {right_temp})"),
                        BinaryOp::Div => format!("rl_checked(3, {left_temp}, {right_temp})"),
                        BinaryOp::Rem => format!("rl_checked(4, {left_temp}, {right_temp})"),
                        BinaryOp::Eq if left.ty == Type::Unit => "1".into(),
                        BinaryOp::Ne if left.ty == Type::Unit => "0".into(),
                        BinaryOp::Eq => format!("{left_temp} == {right_temp}"),
                        BinaryOp::Ne => format!("{left_temp} != {right_temp}"),
                        BinaryOp::Lt => format!("{left_temp} < {right_temp}"),
                        BinaryOp::Le => format!("{left_temp} <= {right_temp}"),
                        BinaryOp::Gt => format!("{left_temp} > {right_temp}"),
                        BinaryOp::Ge => format!("{left_temp} >= {right_temp}"),
                        BinaryOp::And | BinaryOp::Or => unreachable!(),
                    };
                    if left.ty == Type::Unit {
                        writeln!(self.out, "(void){left_temp}; (void){right_temp};").unwrap();
                    }
                    writeln!(self.out, "{ty} {result} = {expression};").unwrap();
                }
            }
        }
        EmitOutcome::Value(result)
    }
}
