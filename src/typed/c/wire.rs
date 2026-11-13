//! Byte bridges use the same checked registry and field names as C lowering.
use super::*;
use crate::abi::copy::{Function, MAX_BYTES};

const HELPERS: &str = r#"
#include <string.h>
static int rl_copy_read_word(const unsigned char *input, uint32_t length, uint32_t *cursor, uint32_t *word) {
    if (*cursor > length || length - *cursor < 4) return 0;
    uint32_t p = *cursor;
    *word = (uint32_t)input[p] | ((uint32_t)input[p + 1] << 8) | ((uint32_t)input[p + 2] << 16) | ((uint32_t)input[p + 3] << 24);
    *cursor += 4;
    return 1;
}
static void rl_copy_write_word(unsigned char *out, uint32_t *cursor, uint32_t word) {
    uint32_t p = *cursor;
    out[p] = (unsigned char)word;
    out[p + 1] = (unsigned char)(word >> 8);
    out[p + 2] = (unsigned char)(word >> 16);
    out[p + 3] = (unsigned char)(word >> 24);
    *cursor += 4;
}
"#;

pub(super) fn append(
    library: &super::super::library::Library,
    out: &mut String,
) -> Result<(), String> {
    let functions = super::super::library::copy_functions(library)?;
    out.push_str(HELPERS);
    // Identity-only f32 roots may have no arithmetic expression. They require
    // the binary32 target assertion and strict flags just as arithmetic does.
    if functions.iter().any(has_f32) {
        out.push_str(crate::floats::STRICT);
    }
    let mut wire = Wire {
        program: &library.program,
        out,
        next: 0,
    };
    for (slot, (export, contract)) in library.exports.iter().zip(&functions).enumerate() {
        let function = &library.program.functions[export.function];
        let super::super::ResultRepr::Value(result_type) = function.result else {
            return Err("E-COPY-ABI: missing result storage".into());
        };
        writeln!(wire.out, "static uint32_t rl_copy_call_{slot}(const unsigned char *input, uint32_t input_len, unsigned char *out, uint32_t out_capacity, uint32_t *out_len) {{").unwrap();
        writeln!(wire.out, "if (out_len == NULL || input_len > {MAX_BYTES} || out_capacity > {MAX_BYTES} || (input_len != 0 && input == NULL)) return 3;").unwrap();
        wire.out.push_str("*out_len = 0;\n");
        let maximum = contract.result.max_encoded_len()?;
        if maximum != 0 {
            writeln!(
                wire.out,
                "if (out_capacity < {maximum}u || out == NULL) return 3;"
            )
            .unwrap();
        }
        wire.out.push_str("uint32_t cursor = 0;\n");
        for (index, (_, ty, _)) in function.params.iter().enumerate() {
            writeln!(wire.out, "{} arg{index};", c_type(*ty)).unwrap();
            wire.decode(*ty, &format!("arg{index}"));
        }
        wire.out.push_str("if (cursor != input_len) return 2;\n");
        write!(
            wire.out,
            "{} result = rl_fn_{}(",
            c_type(result_type),
            export.function
        )
        .unwrap();
        for index in 0..function.params.len() {
            if index != 0 {
                wire.out.push_str(", ");
            }
            write!(wire.out, "arg{index}").unwrap();
        }
        wire.out.push_str(");\ncursor = 0;\n");
        wire.encode(result_type, "result");
        wire.out.push_str("*out_len = cursor;\nreturn 0;\n}\n");
        writeln!(wire.out, "uint32_t {}(const unsigned char *input, uint32_t input_len, unsigned char *out, uint32_t out_capacity, uint32_t *out_len) {{ return rl_copy_call_{slot}(input, input_len, out, out_capacity, out_len); }}", export.c_name).unwrap();
    }
    wire.out.push_str("uint32_t rill_abi_invoke_copy_v1(uint32_t slot, const unsigned char *input, uint32_t input_len, unsigned char *out, uint32_t out_capacity, uint32_t *out_len) {\nswitch (slot) {\n");
    for slot in 0..library.exports.len() {
        writeln!(wire.out, "case {slot}: return rl_copy_call_{slot}(input, input_len, out, out_capacity, out_len);").unwrap();
    }
    wire.out.push_str("default: return 1;\n}\n}\n");
    Ok(())
}

fn has_f32(function: &Function) -> bool {
    use crate::abi::copy::Schema;
    let mut pending: Vec<_> = function
        .parameters
        .iter()
        .chain(std::iter::once(&function.result))
        .collect();
    while let Some(schema) = pending.pop() {
        match schema {
            Schema::F32 => return true,
            Schema::Record { fields, .. } => pending.extend(fields.iter().map(|f| &f.schema)),
            Schema::Enum { variants, .. } => {
                pending.extend(variants.iter().flat_map(|v| &v.fields))
            }
            _ => {}
        }
    }
    false
}

struct Wire<'p, 'o> {
    program: &'p TypedMain,
    out: &'o mut String,
    next: usize,
}

impl Wire<'_, '_> {
    fn word(&mut self) -> String {
        let name = format!("word{}", self.next);
        self.next += 1;
        name
    }

    fn decode(&mut self, ty: Type, target: &str) {
        match ty {
            Type::Int | Type::CF32 => {
                let word = self.word();
                writeln!(self.out, "uint32_t {word}; if (!rl_copy_read_word(input, input_len, &cursor, &{word})) return 2; memcpy(&({target}), &{word}, 4);").unwrap();
            }
            Type::Bool => {
                writeln!(self.out, "if (cursor >= input_len || input[cursor] > 1) return 2; {target} = input[cursor++];").unwrap();
            }
            Type::Adt { id, owned: false } => {
                let adt = &self.program.adts[id];
                if adt.record_fields.is_some() {
                    if adt.variants[0].fields.is_empty() {
                        writeln!(self.out, "{target}.unused = 0;").unwrap();
                    }
                    for (field, ty) in adt.variants[0].fields.iter().enumerate() {
                        self.decode(*ty, &format!("{target}.f{field}"));
                    }
                } else {
                    let word = self.word();
                    writeln!(self.out, "uint32_t {word}; if (!rl_copy_read_word(input, input_len, &cursor, &{word})) return 2; switch ({word}) {{").unwrap();
                    for (tag, variant) in adt.variants.iter().enumerate() {
                        writeln!(self.out, "case {tag}: {{ {target}.tag = {tag};").unwrap();
                        for (field, ty) in variant.fields.iter().enumerate() {
                            self.decode(*ty, &format!("{target}.payload.v{tag}.f{field}"));
                        }
                        self.out.push_str("break;\n}\n");
                    }
                    self.out.push_str("default: return 2;\n}\n");
                }
            }
            _ => unreachable!("independently verified Copy wire type"),
        }
    }

    fn encode(&mut self, ty: Type, source: &str) {
        match ty {
            Type::Int | Type::CF32 => {
                let word = self.word();
                writeln!(self.out, "uint32_t {word}; memcpy(&{word}, &({source}), 4); rl_copy_write_word(out, &cursor, {word});").unwrap();
            }
            Type::Bool => {
                writeln!(self.out, "out[cursor++] = {source} ? 1 : 0;").unwrap();
            }
            Type::Adt { id, owned: false } => {
                let adt = &self.program.adts[id];
                if adt.record_fields.is_some() {
                    for (field, ty) in adt.variants[0].fields.iter().enumerate() {
                        self.encode(*ty, &format!("{source}.f{field}"));
                    }
                } else {
                    writeln!(self.out, "switch ({source}.tag) {{").unwrap();
                    for (tag, variant) in adt.variants.iter().enumerate() {
                        writeln!(
                            self.out,
                            "case {tag}: {{ rl_copy_write_word(out, &cursor, {tag});"
                        )
                        .unwrap();
                        for (field, ty) in variant.fields.iter().enumerate() {
                            self.encode(*ty, &format!("{source}.payload.v{tag}.f{field}"));
                        }
                        self.out.push_str("break;\n}\n");
                    }
                    self.out.push_str("default: return 4;\n}\n");
                }
            }
            _ => unreachable!("independently verified Copy wire type"),
        }
    }
}
