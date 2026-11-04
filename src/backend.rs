//! A portable C11 backend for the checked sequential subset.
use crate::ast::{BinaryOp, Expr, ExternType, ParamMode, Pattern, Program};
use crate::c_import::abi::c_type;
use std::collections::HashMap;
use std::fmt::Write;

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

/* The tag determines the active payload. A byte/vector word uses data, a
   string uses text/len, and native scalars/pointers use their matching member.
   Keeping these mutually exclusive fields in a union reduces value copies. */
typedef struct { int tag; int32_t value; size_t size; union { uint32_t data; struct { const unsigned char *text; size_t len; }; uint64_t wide; float real; const void *pointer; }; } rl_node;
typedef struct { int tag; int32_t value; size_t size; union { uint32_t data; struct { const unsigned char *text; size_t len; }; uint64_t wide; float real; const void *pointer; }; rl_node nodes[RL_CAP]; } rl_value;
static rl_value rl_make(int tag, int32_t value) { rl_value v = {0}; v.tag = tag; v.value = value; return v; }
static void rl_trap(void) { fputs("rill: runtime trap\n", stderr); exit(1); }
static rl_value rl_ffi_u(int tag, uint64_t n) { rl_value v = rl_make(tag, 0); v.wide = n; return v; }
static rl_value rl_ffi_f(float n) { rl_value v = rl_make(11, 0); v.real = n; return v; }
static rl_value rl_ffi_p(const void *p) { rl_value v = rl_make(12, 0); v.pointer = p; return v; }
static uint64_t rl_ffi_unsigned(rl_value v, int tag) { if (v.tag != tag) rl_trap(); return v.wide; }
static float rl_ffi_float(rl_value v) { if (v.tag != 11) rl_trap(); return v.real; }
static const void *rl_ffi_pointer(rl_value v) { if (v.tag != 12) rl_trap(); return v.pointer; }
static rl_value rl_ffi_convert(int tag, rl_value v) {
    if (v.tag != 0) rl_trap();
    if (tag == 11) return rl_ffi_f((float)v.value);
    if (v.value < 0 || (tag == 8 && v.value > UINT8_MAX)) rl_trap();
    return rl_ffi_u(tag, (uint64_t)v.value);
}
static unsigned rl_call_depth;
static void rl_enter(void) { if (rl_call_depth >= 32) rl_trap(); ++rl_call_depth; }
static void rl_leave(void) { --rl_call_depth; }
static rl_value rl_string(const unsigned char *text, size_t len) { rl_value v = rl_make(4, 0); v.text = text; v.len = len; return v; }
static rl_value rl_byte_len(rl_value v) { if (v.tag != 4 || v.len > INT32_MAX) rl_trap(); return rl_make(0, (int32_t)v.len); }
static rl_value rl_byte_at(rl_value v, rl_value index) { if (v.tag != 4 || index.tag != 0 || index.value < 0 || (size_t)index.value >= v.len) rl_trap(); return rl_make(0, (int32_t)v.text[index.value]); }
static rl_value rl_bytes_new(rl_value len) {
    if (len.tag != 0 || len.value < 0 || len.value > 256) rl_trap();
    rl_value v = rl_make(5, len.value); v.size = ((size_t)len.value + 3) / 4;
    if (v.size > RL_CAP) rl_trap();
    return v;
}
static rl_value rl_bytes_len(rl_value v) { if (v.tag != 5) rl_trap(); return rl_make(0, v.value); }
static rl_value rl_bytes_get(rl_value v, rl_value index) {
    if (v.tag != 5 || index.tag != 0 || index.value < 0 || index.value >= v.value) rl_trap();
    size_t i = (size_t)index.value;
    return rl_make(0, (int32_t)((v.nodes[i / 4].data >> (8 * (i % 4))) & UINT32_C(255)));
}
static rl_value rl_bytes_set(rl_value v, rl_value index, rl_value byte) {
    if (v.tag != 5 || index.tag != 0 || index.value < 0 || index.value >= v.value || byte.tag != 0 || byte.value < 0 || byte.value > 255) rl_trap();
    size_t i = (size_t)index.value; unsigned shift = (unsigned)(8 * (i % 4));
    v.nodes[i / 4].data = (v.nodes[i / 4].data & ~(UINT32_C(255) << shift)) | ((uint32_t)byte.value << shift);
    return v;
}
/* Portable lane-by-lane fallback. No target vector extension is assumed. */
static unsigned rl_vget(rl_value v, int32_t i) {
    if (v.tag != 6 || i < 0 || i >= 16 || v.size != 4) rl_trap();
    unsigned index = (unsigned)i;
    return (unsigned)((v.nodes[index / 4].data >> (8 * (index % 4))) & UINT32_C(255));
}
static void rl_vset(rl_value *v, unsigned i, unsigned byte) {
    if (v->tag != 6 || i >= 16 || byte > 255 || v->size != 4) rl_trap();
    unsigned shift = 8 * (i % 4);
    v->nodes[i / 4].data = (v->nodes[i / 4].data & ~(UINT32_C(255) << shift)) | ((uint32_t)byte << shift);
}
static rl_value rl_u8x16_load(rl_value bytes, rl_value offset) {
    if (bytes.tag != 5 || offset.tag != 0 || offset.value < 0 || bytes.value < 16 || offset.value > bytes.value - 16) rl_trap();
    if (RL_CAP < 4) rl_trap();
    rl_value result = rl_make(6, 0); result.size = 4;
    for (unsigned i = 0; i < 16; ++i) {
        unsigned pos = (unsigned)offset.value + i;
        unsigned byte = (unsigned)((bytes.nodes[pos / 4].data >> (8 * (pos % 4))) & UINT32_C(255));
        rl_vset(&result, i, byte);
    }
    return result;
}
static rl_value rl_u8x16_lane(rl_value v, rl_value index) { if (index.tag != 0) rl_trap(); return rl_make(0, (int32_t)rl_vget(v, index.value)); }
static rl_value rl_vbinary(int op, rl_value a, rl_value b) {
    if (a.tag != 6 || b.tag != 6 || a.size != 4 || b.size != 4) rl_trap();
    rl_value result = rl_make(6, 0); result.size = 4;
#if defined(RILL_SIMD_NEON)
    uint8_t left[16], right[16], output[16];
    for (unsigned i = 0; i < 16; ++i) { left[i] = (uint8_t)rl_vget(a, (int32_t)i); right[i] = (uint8_t)rl_vget(b, (int32_t)i); }
    uint8x16_t x = vld1q_u8(left), y = vld1q_u8(right), z;
    switch (op) { case 0: z = vandq_u8(x, y); break; case 1: z = vorrq_u8(x, y); break; case 2: z = veorq_u8(x, y); break; case 3: z = vceqq_u8(x, y); break; default: rl_trap(); z = vdupq_n_u8(0); }
    vst1q_u8(output, z);
    for (unsigned i = 0; i < 16; ++i) rl_vset(&result, i, output[i]);
#else
    for (unsigned i = 0; i < 16; ++i) {
        unsigned x = rl_vget(a, (int32_t)i), y = rl_vget(b, (int32_t)i), z = 0;
        switch (op) { case 0: z = x & y; break; case 1: z = x | y; break; case 2: z = x ^ y; break; case 3: z = x == y ? 255 : 0; break; default: rl_trap(); }
        rl_vset(&result, i, z);
    }
#endif
    return result;
}
static rl_value rl_u8x16_and(rl_value a, rl_value b) { return rl_vbinary(0, a, b); }
static rl_value rl_u8x16_or(rl_value a, rl_value b) { return rl_vbinary(1, a, b); }
static rl_value rl_u8x16_xor(rl_value a, rl_value b) { return rl_vbinary(2, a, b); }
static rl_value rl_u8x16_eq(rl_value a, rl_value b) { return rl_vbinary(3, a, b); }
static rl_value rl_u8x16_any(rl_value v) { for (int32_t i = 0; i < 16; ++i) if (rl_vget(v, i)) return rl_make(1, 1); return rl_make(1, 0); }
static rl_value rl_u8x16_all(rl_value v) { for (int32_t i = 0; i < 16; ++i) if (!rl_vget(v, i)) return rl_make(1, 0); return rl_make(1, 1); }
static void rl_append(rl_value *parent, rl_value child) {
    if (child.size >= RL_CAP || parent->size > RL_CAP - child.size - 1) rl_trap();
    size_t i = parent->size;
    rl_node root = {0}; root.tag = child.tag; root.value = child.value; root.size = child.size;
    if (child.tag == 4) { root.text = child.text; root.len = child.len; }
    else if (child.tag >= 8 && child.tag <= 10) root.wide = child.wide;
    else if (child.tag == 11) root.real = child.real;
    else if (child.tag == 12) root.pointer = child.pointer;
    parent->nodes[i] = root;
    for (size_t j = 0; j < child.size; ++j) parent->nodes[i + 1 + j] = child.nodes[j];
    parent->size += child.size + 1;
}
static rl_value rl_child(rl_value parent, size_t child) {
    size_t i = 0;
    for (size_t n = 0; n < child; ++n) {
        if (i >= parent.size || parent.nodes[i].size >= parent.size - i) rl_trap();
        i += parent.nodes[i].size + 1;
    }
    if (i >= parent.size || parent.nodes[i].size >= parent.size - i) rl_trap();
    rl_node root = parent.nodes[i];
    rl_value v = rl_make(root.tag, root.value); v.size = root.size;
    if (root.tag == 4) { v.text = root.text; v.len = root.len; }
    else if (root.tag >= 8 && root.tag <= 10) v.wide = root.wide;
    else if (root.tag == 11) v.real = root.real;
    else if (root.tag == 12) v.pointer = root.pointer;
    for (size_t j = 0; j < root.size; ++j) v.nodes[j] = parent.nodes[i + 1 + j];
    return v;
}
/* Replace one record field after evaluating its new value. Rebuilding keeps
   sibling payloads intact even when a variant changes its encoded size. */
static rl_value rl_replace_field(rl_value parent, size_t index, rl_value child, size_t count) {
    if (parent.tag != 3 || index >= count) rl_trap();
    rl_value result = rl_make(parent.tag, parent.value);
    for (size_t i = 0; i < count; ++i) rl_append(&result, i == index ? child : rl_child(parent, i));
    return result;
}
static int32_t rl_get(rl_value v, int tag) { if (v.tag != tag) rl_trap(); return v.value; }
static rl_value rl_arith(int op, rl_value a, rl_value b) {
    int32_t x = rl_get(a, 0), y = rl_get(b, 0);
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
    return rl_make(0, (int32_t)n);
}
static rl_value rl_compare(int op, rl_value a, rl_value b) {
    if (a.tag != b.tag) rl_trap();
    if (op >= 2 && a.tag != 0) rl_trap();
    int v = 0;
    if (a.tag >= 8 && a.tag <= 12) {
        if (op > 1) rl_trap();
        v = a.tag == 11 ? a.real == b.real : a.tag == 12 ? a.pointer == b.pointer : a.wide == b.wide;
        return rl_make(1, op == 0 ? v : !v);
    }
    switch (op) {
    case 0: v = a.value == b.value; break;
    case 1: v = a.value != b.value; break;
    case 2: v = a.value < b.value; break;
    case 3: v = a.value <= b.value; break;
    case 4: v = a.value > b.value; break;
    case 5: v = a.value >= b.value; break;
    default: rl_trap();
    }
    return rl_make(1, v);
}
"#;

fn wrap_c(ty: &ExternType, value: &str) -> String {
    // Header constants may be runtime-valued comma expressions. Keep the
    // entire expansion a single operand, just as typed constant reads do.
    let value = format!("({value})");
    match ty {
        ExternType::I32 => format!("rl_make(0, {value})"),
        ExternType::Bool => format!("rl_make(1, ({value}) != 0)"),
        ExternType::U8 => format!("rl_ffi_u(8, {value})"),
        ExternType::U32 => format!("rl_ffi_u(9, {value})"),
        ExternType::U64 => format!("rl_ffi_u(10, {value})"),
        ExternType::F32 => format!("rl_ffi_f({value})"),
        ExternType::Pointer { .. } => format!("rl_ffi_p({value})"),
        _ => unreachable!("checked foreign result"),
    }
}
fn unwrap_c(ty: &ExternType, value: &str) -> String {
    match ty {
        ExternType::I32 => format!("rl_get({value}, 0)"),
        ExternType::Bool => format!("(rl_get({value}, 1) != 0)"),
        ExternType::U8 => format!("(uint8_t)rl_ffi_unsigned({value}, 8)"),
        ExternType::U32 => format!("(uint32_t)rl_ffi_unsigned({value}, 9)"),
        ExternType::U64 => format!("rl_ffi_unsigned({value}, 10)"),
        ExternType::F32 => format!("rl_ffi_float({value})"),
        ExternType::Pointer { .. } => format!("({})rl_ffi_pointer({value})", c_type(ty)),
        _ => unreachable!("checked foreign argument"),
    }
}

pub fn emit(program: &Program) -> Result<String, String> {
    if crate::functions::needs_lowering(program) {
        return emit(&crate::functions::lower(program)?);
    }
    let checked = crate::check::analyze(program)?;
    match crate::typed::lower_main(&checked) {
        Ok(typed) => return crate::typed::emit_c(&typed),
        Err(error) if error.starts_with("E-TYPED-UNSUPPORTED:") && !checked.has_owned() => {}
        Err(error) => return Err(error),
    }
    emit_checked_reference(&checked)
}

/// Emit uniform C11 directly from the checked Copy subset's source AST.
/// This is a differential oracle during migration to the shared typed path;
/// owner programs must never use this copying representation. Like `emit`, it
/// returns target-header assertions but does not run the target C compiler.
pub fn emit_reference(program: &Program) -> Result<String, String> {
    let checked = crate::check::analyze(program)?;
    emit_checked_reference(&checked)
}

fn emit_checked_reference(checked: &crate::check::CheckedProgram<'_>) -> Result<String, String> {
    if checked.has_owned() {
        return Err("E-REFERENCE: owner programs require the checked typed path".into());
    }
    let program = checked.program;
    if crate::functions::needs_lowering(program) {
        return Err("E-REFERENCE: callable C emission requires checked closure conversion".into());
    }
    let has_payload = program
        .types
        .iter()
        .flat_map(|ty| &ty.variants)
        .any(|variant| !variant.fields.is_empty());
    let capacity = if has_payload {
        checked.max_inline_nodes()?.max(1)
    } else {
        1
    };
    let capacity = if program.functions.iter().any(|f| has_bytes(&f.body)) {
        capacity.max(65)
    } else {
        capacity
    };
    let mut constructors = HashMap::new();
    for variant in program.types.iter().flat_map(|ty| &ty.variants) {
        let index = constructors.len();
        if constructors
            .insert(variant.name.clone(), (index, variant.fields.len()))
            .is_some()
        {
            return Err(format!("duplicate constructor: {}", variant.name));
        }
    }
    let externs: HashMap<_, _> = program
        .externs
        .iter()
        .map(|function| (function.name.clone(), function))
        .collect();
    let mut functions = HashMap::new();
    for (i, f) in program.functions.iter().enumerate() {
        if functions
            .insert(
                f.name.clone(),
                (i, f.params.iter().map(|p| p.mode).collect::<Vec<_>>()),
            )
            .is_some()
        {
            return Err(format!("duplicate function: {}", f.name));
        }
    }
    let (main, main_modes) = functions.get("main").ok_or("missing main function")?;
    if !main_modes.is_empty() {
        return Err("main must have no parameters".into());
    }
    let mut out = program.native.preamble()?;
    out.push_str(&format!("#define RL_CAP {capacity}\n{RUNTIME}"));
    out.push_str("_Static_assert(CHAR_BIT == 8, \"Rill strings require 8-bit bytes\");\n");
    out.push_str("_Static_assert(sizeof(rl_node) <= 64 && sizeof(rl_value) <= 64 * (RL_CAP + 1), \"unsupported target value layout\");\n");
    crate::c_import::abi::emit_declarations(
        &program.native,
        &program.externs,
        &program.constants,
        &mut out,
    );
    for (i, f) in program.functions.iter().enumerate() {
        write!(out, "\nstatic rl_value rl_fn_{i}(").unwrap();
        if f.params.is_empty() {
            out.push_str("void");
        }
        for (j, param) in f.params.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            let (qualifier, pointer) = match param.mode {
                ParamMode::Value => ("", ""),
                ParamMode::Read => ("const ", "*"),
                ParamMode::Inout => ("", "*"),
            };
            write!(out, "{qualifier}rl_value {pointer}rl_p_{j}").unwrap();
        }
        out.push_str(");\n");
    }
    for (i, f) in program.functions.iter().enumerate() {
        write!(out, "\nstatic rl_value rl_fn_{i}(").unwrap();
        if f.params.is_empty() {
            out.push_str("void");
        }
        for (j, param) in f.params.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            let (qualifier, pointer) = match param.mode {
                ParamMode::Value => ("", ""),
                ParamMode::Read => ("const ", "*"),
                ParamMode::Inout => ("", "*"),
            };
            write!(out, "{qualifier}rl_value {pointer}rl_p_{j}").unwrap();
        }
        out.push_str(") {\nrl_enter();\n");
        let mut env = HashMap::new();
        for (j, param) in f.params.iter().enumerate() {
            let name = if param.mode != ParamMode::Value {
                format!("(*rl_p_{j})")
            } else {
                format!("rl_p_{j}")
            };
            env.insert(param.name.clone(), name);
        }
        let mut emitter = Emitter {
            out: &mut out,
            functions: &functions,
            constructors: &constructors,
            externs: &externs,
            checked,
            next: 0,
        };
        let result = emitter.expr(&f.body, &mut env)?;
        // The estimate intentionally overcounts lexical temporaries and arguments.
        // It bounds one source of very large frames, not total native stack use.
        let value_count = emitter
            .next
            .checked_add(f.params.len().saturating_mul(2))
            .and_then(|n| n.checked_add(8))
            .ok_or("E-C-STACK: frame estimate overflow")?;
        let estimated = capacity
            .checked_add(1)
            .and_then(|n| n.checked_mul(64))
            .and_then(|n| n.checked_mul(value_count))
            .ok_or("E-C-STACK: frame estimate overflow")?;
        if estimated > 1024 * 1024 {
            return Err(format!(
                "E-C-STACK: estimated frame for '{}' is {estimated} bytes (1 MiB limit)",
                f.name
            ));
        }
        writeln!(out, "rl_leave(); return {result};\n}}").unwrap();
    }
    out.push_str("\nstatic void rl_print(rl_value v) { if (v.tag == 0) printf(\"%\" PRId32, v.value); else if (v.tag >= 8 && v.tag <= 10) printf(\"%\" PRIu64, v.wide); else if (v.tag == 11) printf(\"%.9g\", (double)v.real); else if (v.tag == 12) printf(\"%p\", (void *)v.pointer); else if (v.tag == 1) fputs(v.value ? \"true\" : \"false\", stdout); else if (v.tag == 2) fputs(\"()\", stdout); else if (v.tag == 4) fwrite(v.text, 1, v.len, stdout); else if (v.tag == 6) { fputs(\"u8x16[\", stdout); for (int32_t i = 0; i < 16; ++i) { if (i) fputs(\", \", stdout); printf(\"%u\", rl_vget(v, i)); } fputc(']', stdout); } else if (v.tag == 5) { fputs(\"bytes[\", stdout); for (int32_t i = 0; i < v.value; ++i) { if (i) fputs(\", \", stdout); printf(\"%\" PRId32, rl_bytes_get(v, rl_make(0, i)).value); } fputc(']', stdout); } else if (v.tag == 3) { switch (v.value) {\n");
    for ty in &program.types {
        for variant in &ty.variants {
            let (index, arity) = constructors[&variant.name];
            if let Some(fields) = &ty.record_fields {
                writeln!(
                    out,
                    "case {index}: fputs({}, stdout);",
                    c_literal(crate::display::constructor_name(&ty.name))
                )
                .unwrap();
                out.push_str("fputs(\" {\", stdout);\n");
                for (field, name) in fields.iter().enumerate() {
                    if field == 0 {
                        out.push_str("fputc(' ', stdout);\n");
                    } else {
                        out.push_str("fputs(\"; \", stdout);\n");
                    }
                    writeln!(out, "fputs({}, stdout); fputs(\" = \", stdout); rl_print(rl_child(v, {field}));", c_literal(name)).unwrap();
                }
                if !fields.is_empty() {
                    out.push_str("fputc(' ', stdout);\n");
                }
                out.push_str("fputc('}', stdout);\n");
            } else {
                writeln!(
                    out,
                    "case {index}: fputs({}, stdout);",
                    c_literal(crate::display::constructor_name(&variant.name))
                )
                .unwrap();
                if arity > 0 {
                    out.push_str("fputc('(', stdout);\n");
                    for field in 0..arity {
                        if field > 0 {
                            out.push_str("fputs(\", \", stdout);\n");
                        }
                        writeln!(out, "rl_print(rl_child(v, {field}));").unwrap();
                    }
                    out.push_str("fputc(')', stdout);\n");
                }
            }
            out.push_str("break;\n");
        }
    }
    writeln!(out, "default: rl_trap(); }} }} else rl_trap(); }}\nint main(void) {{ rl_value v = rl_fn_{main}(); rl_print(v); fputc('\\n', stdout); return 0; }}").unwrap();
    Ok(out)
}

struct Emitter<'a, 'p> {
    out: &'a mut String,
    functions: &'a HashMap<String, (usize, Vec<ParamMode>)>,
    constructors: &'a HashMap<String, (usize, usize)>,
    externs: &'a HashMap<String, &'a crate::ast::ExternFunction>,
    checked: &'a crate::check::CheckedProgram<'p>,
    next: usize,
}
impl Emitter<'_, '_> {
    fn temp(&mut self) -> String {
        let n = self.next;
        self.next += 1;
        format!("rl_t_{n}")
    }
    fn read_path(
        &mut self,
        root: &str,
        path: &[crate::check::FieldInfo],
    ) -> Result<String, String> {
        if path.is_empty() {
            return Err("E-C-RECORD: empty checked field path".into());
        }
        let mut value = root.to_string();
        for field in path {
            let temporary = self.temp();
            writeln!(
                self.out,
                "rl_value {temporary} = rl_child({value}, {});",
                field.index
            )
            .unwrap();
            value = temporary;
        }
        Ok(value)
    }
    fn write_path(
        &mut self,
        root: &str,
        path: &[crate::check::FieldInfo],
        replacement: &str,
    ) -> Result<(), String> {
        if path.is_empty() {
            return Err("E-C-RECORD: empty checked field path".into());
        }
        // Re-read the current root after argument evaluation/callee execution,
        // then rebuild ancestors from the changed leaf outward.
        let mut parents = vec![root.to_string()];
        for field in &path[..path.len() - 1] {
            let temporary = self.temp();
            writeln!(
                self.out,
                "rl_value {temporary} = rl_child({}, {});",
                parents.last().unwrap(),
                field.index
            )
            .unwrap();
            parents.push(temporary);
        }
        let mut value = replacement.to_string();
        for (parent, field) in parents.iter().zip(path).rev() {
            let temporary = self.temp();
            writeln!(
                self.out,
                "rl_value {temporary} = rl_replace_field({parent}, {}, {value}, {});",
                field.index, field.arity
            )
            .unwrap();
            value = temporary;
        }
        writeln!(self.out, "{root} = {value};").unwrap();
        Ok(())
    }
    fn pattern(
        &self,
        pattern: &Pattern,
        value: &str,
        env: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        Ok(match pattern {
            Pattern::Wildcard => "1".into(),
            Pattern::Binding(name) => {
                env.insert(name.clone(), value.into());
                "1".into()
            }
            Pattern::Bool(b) => {
                format!("({value}).tag == 1 && ({value}).value == {}", i32::from(*b))
            }
            Pattern::Int(n) => format!("({value}).tag == 0 && ({value}).value == INT32_C({n})"),
            Pattern::Unit => format!("({value}).tag == 2"),
            Pattern::Constructor(name, fields) => {
                let &(index, arity) = self
                    .constructors
                    .get(name)
                    .ok_or_else(|| format!("unknown constructor: {name}"))?;
                if fields.len() != arity {
                    return Err(format!("wrong number of fields for {name}"));
                }
                let mut parts = vec![format!("({value}).tag == 3 && ({value}).value == {index}")];
                for (i, field) in fields.iter().enumerate() {
                    parts.push(self.pattern(field, &format!("rl_child({value}, {i})"), env)?);
                }
                parts
                    .into_iter()
                    .map(|p| format!("({p})"))
                    .collect::<Vec<_>>()
                    .join(" && ")
            }
        })
    }
    fn expr(&mut self, e: &Expr, env: &mut HashMap<String, String>) -> Result<String, String> {
        let result = self.temp();
        match e {
            Expr::Lambda(..) | Expr::Apply(..) | Expr::FunctionOp(..) => {
                return Err("E-FUNCTION: callable conversion was not run before C emission".into());
            }
            Expr::Int(n) => {
                writeln!(self.out, "rl_value {result} = rl_make(0, INT32_C({n}));").unwrap();
            }
            Expr::Bool(b) => {
                writeln!(
                    self.out,
                    "rl_value {result} = rl_make(1, {});",
                    i32::from(*b)
                )
                .unwrap();
            }
            Expr::Unit => {
                writeln!(self.out, "rl_value {result} = rl_make(2, 0);").unwrap();
            }
            Expr::Str(value) => {
                let literal = c_literal(value);
                writeln!(
                    self.out,
                    "rl_value {result} = rl_string((const unsigned char *){literal}, {});",
                    value.len()
                )
                .unwrap();
            }
            Expr::CStr(_) => {
                return Err("E-FFI: cstr literals are only supported as direct C arguments".into())
            }
            Expr::Move(_) => return Err("E-OWNER: move requires checked ownership lowering".into()),
            Expr::Inout(_) | Expr::InoutField(_, _) => {
                return Err("E-LOAN: inout argument is only valid at a call".into())
            }
            Expr::Record(_, fields) => {
                let info = self
                    .checked
                    .record_info(e)
                    .ok_or("E-C-RECORD: missing checked record metadata")?
                    .clone();
                let &(index, arity) = self
                    .constructors
                    .get(&info.constructor)
                    .ok_or("E-C-RECORD: missing constructor")?;
                if arity != fields.len() || info.field_indices.len() != arity {
                    return Err("E-C-RECORD: invalid checked field mapping".into());
                }
                let mut ordered = vec![None; arity];
                for ((_, value), &field) in fields.iter().zip(&info.field_indices) {
                    if field >= arity || ordered[field].is_some() {
                        return Err("E-C-RECORD: invalid checked field index".into());
                    }
                    ordered[field] = Some(self.expr(value, env)?);
                }
                writeln!(self.out, "rl_value {result} = rl_make(3, {index});").unwrap();
                for value in ordered {
                    writeln!(
                        self.out,
                        "rl_append(&{result}, {});",
                        value.ok_or("E-C-RECORD: missing checked field")?
                    )
                    .unwrap();
                }
            }
            Expr::Field(root, _) => {
                let path = self
                    .checked
                    .field_info(e)
                    .ok_or("E-C-RECORD: missing checked field metadata")?
                    .to_vec();
                let root = env.get(root).ok_or("E-C-RECORD: missing root")?.clone();
                let value = self.read_path(&root, &path)?;
                writeln!(self.out, "rl_value {result} = {value};").unwrap();
            }
            Expr::FieldAssign(root, _, value) => {
                let path = self
                    .checked
                    .field_info(e)
                    .ok_or("E-C-RECORD: missing checked field metadata")?
                    .to_vec();
                let value = self.expr(value, env)?;
                let root = env.get(root).ok_or("E-C-RECORD: missing root")?.clone();
                self.write_path(&root, &path, &value)?;
                writeln!(self.out, "rl_value {result} = rl_make(2, 0);").unwrap();
            }
            Expr::Var(name) => {
                if let Some(v) = env.get(name) {
                    writeln!(self.out, "rl_value {result} = {v};").unwrap();
                } else if let Some(constant) = self
                    .checked
                    .program
                    .constants
                    .iter()
                    .find(|c| &c.name == name)
                {
                    writeln!(
                        self.out,
                        "rl_value {result} = {};",
                        wrap_c(&constant.ty, name)
                    )
                    .unwrap();
                } else if let Some((index, 0)) = self.constructors.get(name) {
                    writeln!(self.out, "rl_value {result} = rl_make(3, {index});").unwrap();
                } else {
                    return Err(format!("unknown variable: {name}"));
                }
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                let v = self.expr(value, env)?;
                let old = env.insert(name.clone(), v);
                let b = self.expr(body, env)?;
                if let Some(old) = old {
                    env.insert(name.clone(), old);
                } else {
                    env.remove(name);
                }
                writeln!(self.out, "rl_value {result} = {b};").unwrap();
            }
            Expr::If(cond, yes, no) => {
                let c = self.expr(cond, env)?;
                writeln!(self.out, "rl_value {result};\nif (rl_get({c}, 1)) {{").unwrap();
                let y = self.expr(yes, env)?;
                writeln!(self.out, "{result} = {y};\n}} else {{").unwrap();
                let n = self.expr(no, env)?;
                writeln!(self.out, "{result} = {n};\n}}").unwrap();
            }
            Expr::Assign(name, value) => {
                let assigned = self.expr(value, env)?;
                let target = env
                    .get(name)
                    .ok_or_else(|| format!("unknown variable: {name}"))?;
                writeln!(
                    self.out,
                    "{target} = {assigned}; rl_value {result} = rl_make(2, 0);"
                )
                .unwrap();
            }
            Expr::While(condition, body) => {
                writeln!(self.out, "for (;;) {{").unwrap();
                let cond = self.expr(condition, env)?;
                writeln!(self.out, "if (!rl_get({cond}, 1)) break;").unwrap();
                let body_value = self.expr(body, env)?;
                writeln!(
                    self.out,
                    "(void)rl_get({body_value}, 2);\n}}\nrl_value {result} = rl_make(2, 0);"
                )
                .unwrap();
            }
            Expr::Call(name, args) => {
                if [
                    "u8x16_load",
                    "u8x16_lane",
                    "u8x16_and",
                    "u8x16_or",
                    "u8x16_xor",
                    "u8x16_eq",
                    "u8x16_any",
                    "u8x16_all",
                ]
                .contains(&name.as_str())
                {
                    let expected = if name == "u8x16_any" || name == "u8x16_all" {
                        1
                    } else {
                        2
                    };
                    if args.len() != expected {
                        return Err(format!("wrong number of arguments to {name}"));
                    }
                    let mut values = Vec::new();
                    for arg in args {
                        values.push(self.expr(arg, env)?);
                    }
                    writeln!(
                        self.out,
                        "rl_value {result} = rl_{name}({});",
                        values.join(", ")
                    )
                    .unwrap();
                    return Ok(result);
                }
                if ["bytes_new", "bytes_len", "bytes_get", "bytes_set"].contains(&name.as_str()) {
                    let expected = match name.as_str() {
                        "bytes_new" | "bytes_len" => 1,
                        "bytes_get" => 2,
                        _ => 3,
                    };
                    if args.len() != expected {
                        return Err(format!("wrong number of arguments to {name}"));
                    }
                    let mut values = Vec::new();
                    for arg in args {
                        values.push(self.expr(arg, env)?);
                    }
                    writeln!(
                        self.out,
                        "rl_value {result} = rl_{name}({});",
                        values.join(", ")
                    )
                    .unwrap();
                    return Ok(result);
                }
                if name == "byte_len" || name == "byte_at" {
                    let expected = if name == "byte_len" { 1 } else { 2 };
                    if args.len() != expected {
                        return Err(format!("wrong number of arguments to {name}"));
                    }
                    let mut values = Vec::new();
                    for arg in args {
                        values.push(self.expr(arg, env)?);
                    }
                    let function = if name == "byte_len" {
                        "rl_byte_len"
                    } else {
                        "rl_byte_at"
                    };
                    writeln!(
                        self.out,
                        "rl_value {result} = {function}({});",
                        values.join(", ")
                    )
                    .unwrap();
                    return Ok(result);
                }
                if matches!(name.as_str(), "u8" | "u32" | "u64" | "f32") {
                    let value = self.expr(&args[0], env)?;
                    let tag = match name.as_str() {
                        "u8" => 8,
                        "u32" => 9,
                        "u64" => 10,
                        _ => 11,
                    };
                    writeln!(
                        self.out,
                        "rl_value {result} = rl_ffi_convert({tag}, {value});"
                    )
                    .unwrap();
                    return Ok(result);
                }
                if name == "c_null" {
                    writeln!(self.out, "rl_value {result} = rl_ffi_p(NULL);").unwrap();
                    return Ok(result);
                }
                if name == "is_null" {
                    let value = self.expr(&args[0], env)?;
                    writeln!(
                        self.out,
                        "rl_value {result} = rl_make(1, rl_ffi_pointer({value}) == NULL);"
                    )
                    .unwrap();
                    return Ok(result);
                }
                if let Some(function) = self.externs.get(name) {
                    if args.len() != function.params.len() {
                        return Err(format!("wrong number of arguments to {name}"));
                    }
                    let mut values = Vec::new();
                    for (arg, param) in args.iter().zip(&function.params) {
                        match (&param.ty, arg) {
                            (ExternType::CStr, Expr::CStr(value)) => {
                                if value.as_bytes().contains(&0) {
                                    return Err("E-FFI: cstr literal contains NUL".into());
                                }
                                values.push(c_literal(value));
                            }
                            (ty, _) if !matches!(ty, ExternType::CStr | ExternType::Unit) => {
                                let value = self.expr(arg, env)?;
                                values.push(unwrap_c(ty, &value));
                            }
                            (ExternType::CStr, _) => {
                                return Err("E-FFI: cstr arguments must be literals".into())
                            }
                            (ExternType::Unit, _) => {
                                return Err("E-FFI: unit parameter is unsupported".into())
                            }
                            _ => unreachable!("checked foreign type"),
                        }
                    }
                    let arguments = values.join(", ");
                    if function.result == ExternType::Unit {
                        writeln!(
                            self.out,
                            "{name}({arguments}); rl_value {result} = rl_make(2, 0);"
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            self.out,
                            "rl_value {result} = {};",
                            wrap_c(&function.result, &format!("{name}({arguments})"))
                        )
                        .unwrap();
                    }
                    return Ok(result);
                }
                if let Some(&(index, arity)) = self.constructors.get(name) {
                    if args.len() != arity {
                        return Err(format!("wrong number of arguments to {name}"));
                    }
                    let mut values = Vec::new();
                    for arg in args {
                        values.push(self.expr(arg, env)?);
                    }
                    writeln!(self.out, "rl_value {result} = rl_make(3, {index});").unwrap();
                    for value in values {
                        writeln!(self.out, "rl_append(&{result}, {value});").unwrap();
                    }
                    return Ok(result);
                }
                let (idx, modes) = self
                    .functions
                    .get(name)
                    .ok_or_else(|| format!("unknown function: {name}"))?;
                if args.len() != modes.len() {
                    return Err(format!("wrong number of arguments to {name}"));
                }
                let mut values = Vec::new();
                let mut writebacks = Vec::new();
                for (arg, mode) in args.iter().zip(modes) {
                    match (mode, arg) {
                        (ParamMode::Read, Expr::Var(name)) if env.contains_key(name) => {
                            values.push(format!("&({})", env[name]));
                        }
                        (ParamMode::Read, _) => {
                            let temporary = self.expr(arg, env)?;
                            values.push(format!("&({temporary})"));
                        }
                        (ParamMode::Inout, Expr::InoutField(root, _)) => {
                            let path = self
                                .checked
                                .field_info(arg)
                                .ok_or("E-C-RECORD: missing checked field metadata")?
                                .to_vec();
                            let root = env.get(root).ok_or("E-C-RECORD: missing root")?.clone();
                            let temporary = self.read_path(&root, &path)?;
                            values.push(format!("&({temporary})"));
                            writebacks.push((root, path, temporary));
                        }
                        (ParamMode::Inout, Expr::Inout(name)) => {
                            let value = env
                                .get(name)
                                .ok_or_else(|| format!("unknown variable: {name}"))?;
                            values.push(format!("&({value})"));
                        }
                        (ParamMode::Inout, _) => {
                            return Err("E-LOAN: expected inout argument".into())
                        }
                        (ParamMode::Value, _) => values.push(self.expr(arg, env)?),
                    }
                }
                writeln!(
                    self.out,
                    "rl_value {result} = rl_fn_{idx}({});",
                    values.join(", ")
                )
                .unwrap();
                for (root, path, temporary) in writebacks {
                    self.write_path(&root, &path, &temporary)?;
                }
            }
            Expr::Match(scrutinee, arms) => {
                let value = self.expr(scrutinee, env)?;
                let matched = self.temp();
                writeln!(self.out, "rl_value {result}; int {matched} = 0;").unwrap();
                for arm in arms {
                    let mut scope = env.clone();
                    let condition = self.pattern(&arm.pattern, &value, &mut scope)?;
                    writeln!(self.out, "if (!{matched} && ({condition})) {{").unwrap();
                    // Pattern bindings need addressable storage for read loans.
                    let mut bindings = Vec::new();
                    pattern_bindings(&arm.pattern, &mut bindings);
                    for name in bindings {
                        let value = scope
                            .get(name)
                            .ok_or("E-C-LOAN: missing pattern binding")?
                            .clone();
                        let local = self.temp();
                        writeln!(self.out, "rl_value {local} = {value};").unwrap();
                        scope.insert(name.to_string(), local);
                    }
                    let arm_value = self.expr(&arm.body, &mut scope)?;
                    writeln!(self.out, "{result} = {arm_value}; {matched} = 1; }}").unwrap();
                }
                writeln!(self.out, "if (!{matched}) rl_trap();").unwrap();
            }
            Expr::Binary(op, left, right) => {
                let l = self.expr(left, env)?;
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let cond = if matches!(op, BinaryOp::And) {
                        format!("rl_get({l}, 1)")
                    } else {
                        format!("!rl_get({l}, 1)")
                    };
                    writeln!(self.out, "rl_value {result};\nif ({cond}) {{").unwrap();
                    let r = self.expr(right, env)?;
                    writeln!(self.out, "{result} = rl_make(1, rl_get({r}, 1));\n}} else {{ {result} = rl_make(1, {}); }}", i32::from(matches!(op, BinaryOp::Or))).unwrap();
                } else {
                    let r = self.expr(right, env)?;
                    let (function, code) = match op {
                        BinaryOp::Add => ("rl_arith", 0),
                        BinaryOp::Sub => ("rl_arith", 1),
                        BinaryOp::Mul => ("rl_arith", 2),
                        BinaryOp::Div => ("rl_arith", 3),
                        BinaryOp::Rem => ("rl_arith", 4),
                        BinaryOp::Eq => ("rl_compare", 0),
                        BinaryOp::Ne => ("rl_compare", 1),
                        BinaryOp::Lt => ("rl_compare", 2),
                        BinaryOp::Le => ("rl_compare", 3),
                        BinaryOp::Gt => ("rl_compare", 4),
                        BinaryOp::Ge => ("rl_compare", 5),
                        BinaryOp::And | BinaryOp::Or => unreachable!(),
                    };
                    writeln!(
                        self.out,
                        "rl_value {result} = {function}({code}, {l}, {r});"
                    )
                    .unwrap();
                }
            }
        }
        Ok(result)
    }
}

fn c_literal(value: &str) -> String {
    let mut literal = String::from("\"");
    for byte in value.as_bytes() {
        write!(literal, "\\{byte:03o}").unwrap();
    }
    literal.push('"');
    literal
}

fn has_bytes(expr: &Expr) -> bool {
    match expr {
        Expr::Lambda(..) | Expr::Apply(..) | Expr::FunctionOp(..) => {
            crate::check::callable_children(expr)
                .into_iter()
                .any(has_bytes)
        }
        Expr::Call(name, args) => {
            matches!(
                name.as_str(),
                "bytes_new"
                    | "bytes_len"
                    | "bytes_get"
                    | "bytes_set"
                    | "u8x16_load"
                    | "u8x16_lane"
                    | "u8x16_and"
                    | "u8x16_or"
                    | "u8x16_xor"
                    | "u8x16_eq"
                    | "u8x16_any"
                    | "u8x16_all"
            ) || args.iter().any(has_bytes)
        }
        Expr::Binary(_, a, b) | Expr::Let(_, a, b) | Expr::LetMut(_, a, b) | Expr::While(a, b) => {
            has_bytes(a) || has_bytes(b)
        }
        Expr::If(a, b, c) => has_bytes(a) || has_bytes(b) || has_bytes(c),
        Expr::Match(value, arms) => has_bytes(value) || arms.iter().any(|arm| has_bytes(&arm.body)),
        Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => has_bytes(value),
        Expr::Record(_, fields) => fields.iter().any(|(_, value)| has_bytes(value)),
        Expr::Field(_, _) => false,
        Expr::Int(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Str(_)
        | Expr::CStr(_)
        | Expr::Var(_)
        | Expr::Inout(_)
        | Expr::InoutField(_, _)
        | Expr::Move(_) => false,
    }
}

fn pattern_bindings<'a>(pattern: &'a Pattern, output: &mut Vec<&'a str>) {
    match pattern {
        Pattern::Binding(name) => output.push(name),
        Pattern::Constructor(_, fields) => {
            for field in fields {
                pattern_bindings(field, output);
            }
        }
        _ => {}
    }
}
