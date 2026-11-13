//! Bounded, explicit byte transport for closed pure Copy values.
//!
//! These bytes describe values, never a C aggregate layout. Every bridge decodes
//! checked fields, calls one verified typed function, then encodes its result.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::ast::{ParamMode, Program};

pub const VERSION: u32 = 1;
pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_DEPTH: usize = 16;
pub const MAX_NODES: usize = 4096;
const MAX_NAME: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schema {
    I32,
    Bool,
    F32,
    Record {
        name: String,
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        variants: Vec<Variant>,
    },
}

// Serde's internally tagged unit variants accept unknown fields even with
// deny_unknown_fields. Empty struct variants preserve strict decoding of scalar
// schemas while retaining the public unit constructors and serialized format.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SchemaInput {
    I32 {},
    Bool {},
    F32 {},
    Record {
        name: String,
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        variants: Vec<Variant>,
    },
}

impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match SchemaInput::deserialize(deserializer)? {
            SchemaInput::I32 {} => Self::I32,
            SchemaInput::Bool {} => Self::Bool,
            SchemaInput::F32 {} => Self::F32,
            SchemaInput::Record { name, fields } => Self::Record { name, fields },
            SchemaInput::Enum { name, variants } => Self::Enum { name, variants },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub schema: Schema,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Schema>,
}

/// Floating-point transport retains binary32 bits, including signed zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    I32(i32),
    Bool(bool),
    F32(u32),
    Record(Vec<(String, Value)>),
    Enum { variant: String, fields: Vec<Value> },
}

impl Value {
    pub fn field(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Record(fields) => fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn field_mut(&mut self, name: &str) -> Option<&mut Self> {
        match self {
            Self::Record(fields) => fields
                .iter_mut()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        if let Self::I32(value) = self {
            Some(*value)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(value) = self {
            Some(*value)
        } else {
            None
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        if let Self::F32(bits) = self {
            Some(f32::from_bits(*bits))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Schema>,
    pub result: Schema,
}

/// Editable source artifacts have no authority to load code into a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub c: String,
    pub header: String,
    pub manifest: String,
    pub functions: Vec<Function>,
}

fn fail(detail: &str) -> String {
    format!("E-COPY-ABI: {detail}")
}

fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(fail(
            "schema names must contain 1 through 1024 noncontrol bytes",
        ));
    }
    Ok(())
}

impl Schema {
    /// Validate names, finite depth, total node count and maximum wire size.
    pub fn validate(&self) -> Result<(), String> {
        let mut remaining = MAX_NODES;
        let mut pending = vec![(self, 0)];
        while let Some((schema, depth)) = pending.pop() {
            remaining = remaining
                .checked_sub(1)
                .ok_or_else(|| fail("schema exceeds 4096 nodes"))?;
            if depth > MAX_DEPTH {
                return Err(fail("schema exceeds depth 16"));
            }
            match schema {
                Self::I32 | Self::Bool | Self::F32 => {}
                Self::Record { name, fields } => {
                    valid_name(name)?;
                    if fields.len() > remaining {
                        return Err(fail("schema exceeds 4096 nodes"));
                    }
                    let mut names = HashSet::new();
                    for field in fields {
                        valid_name(&field.name)?;
                        if !names.insert(&field.name) {
                            return Err(fail("duplicate record field"));
                        }
                        pending.push((&field.schema, depth + 1));
                    }
                }
                Self::Enum { name, variants } => {
                    valid_name(name)?;
                    if variants.is_empty() || variants.len() > remaining {
                        return Err(fail("enum requires 1 through 4096 variants"));
                    }
                    remaining = remaining
                        .checked_sub(variants.len())
                        .ok_or_else(|| fail("schema exceeds 4096 nodes"))?;
                    let mut names = HashSet::new();
                    for variant in variants {
                        valid_name(&variant.name)?;
                        if !names.insert(&variant.name) {
                            return Err(fail("duplicate enum variant"));
                        }
                        if variant.fields.len() > remaining {
                            return Err(fail("schema exceeds 4096 nodes"));
                        }
                        pending.extend(variant.fields.iter().map(|schema| (schema, depth + 1)));
                    }
                }
            }
        }
        if self.unchecked_max_len() > MAX_BYTES {
            return Err(fail("wire value exceeds 65536 bytes"));
        }
        Ok(())
    }

    fn unchecked_max_len(&self) -> usize {
        match self {
            Self::I32 | Self::F32 => 4,
            Self::Bool => 1,
            Self::Record { fields, .. } => {
                fields.iter().map(|f| f.schema.unchecked_max_len()).sum()
            }
            Self::Enum { variants, .. } => {
                4 + variants
                    .iter()
                    .map(|v| v.fields.iter().map(Self::unchecked_max_len).sum::<usize>())
                    .max()
                    .unwrap_or(0)
            }
        }
    }

    pub fn max_encoded_len(&self) -> Result<usize, String> {
        self.validate()?;
        Ok(self.unchecked_max_len())
    }

    pub fn validate_value(&self, value: &Value) -> Result<(), String> {
        self.encode(value).map(|_| ())
    }

    pub fn encode(&self, value: &Value) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = Vec::new();
        self.encode_into(value, &mut bytes)?;
        Ok(bytes)
    }

    fn encode_into(&self, value: &Value, bytes: &mut Vec<u8>) -> Result<(), String> {
        match (self, value) {
            (Self::I32, Value::I32(value)) => bytes.extend_from_slice(&value.to_le_bytes()),
            (Self::Bool, Value::Bool(value)) => bytes.push(u8::from(*value)),
            (Self::F32, Value::F32(value)) => bytes.extend_from_slice(&value.to_le_bytes()),
            (Self::Record { fields, .. }, Value::Record(values))
                if fields.len() == values.len() =>
            {
                for (field, (name, value)) in fields.iter().zip(values) {
                    if field.name != *name {
                        return Err(fail("record field names or order do not match the schema"));
                    }
                    field.schema.encode_into(value, bytes)?;
                }
            }
            (Self::Enum { variants, .. }, Value::Enum { variant, fields }) => {
                let tag = variants
                    .iter()
                    .position(|item| item.name == *variant)
                    .ok_or_else(|| fail("unknown enum variant"))?;
                let schemas = &variants[tag].fields;
                if schemas.len() != fields.len() {
                    return Err(fail("enum payload arity does not match the schema"));
                }
                bytes.extend_from_slice(&(tag as u32).to_le_bytes());
                for (schema, value) in schemas.iter().zip(fields) {
                    schema.encode_into(value, bytes)?;
                }
            }
            _ => return Err(fail("value does not match its schema")),
        }
        Ok(())
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<Value, String> {
        self.validate()?;
        if bytes.len() > MAX_BYTES {
            return Err(fail("wire value exceeds 65536 bytes"));
        }
        let mut input = bytes;
        let value = self.decode_from(&mut input)?;
        if !input.is_empty() {
            return Err(fail("trailing wire bytes"));
        }
        Ok(value)
    }

    fn decode_from(&self, input: &mut &[u8]) -> Result<Value, String> {
        fn word(input: &mut &[u8]) -> Result<[u8; 4], String> {
            let value = input.get(..4).ok_or_else(|| fail("truncated wire value"))?;
            let value = value.try_into().unwrap();
            *input = &input[4..];
            Ok(value)
        }
        Ok(match self {
            Self::I32 => Value::I32(i32::from_le_bytes(word(input)?)),
            Self::F32 => Value::F32(u32::from_le_bytes(word(input)?)),
            Self::Bool => {
                let value = *input.first().ok_or_else(|| fail("truncated wire value"))?;
                *input = &input[1..];
                match value {
                    0 => Value::Bool(false),
                    1 => Value::Bool(true),
                    _ => return Err(fail("invalid wire boolean")),
                }
            }
            Self::Record { fields, .. } => Value::Record(
                fields
                    .iter()
                    .map(|field| Ok((field.name.clone(), field.schema.decode_from(input)?)))
                    .collect::<Result<_, String>>()?,
            ),
            Self::Enum { variants, .. } => {
                let tag = u32::from_le_bytes(word(input)?) as usize;
                let variant = variants
                    .get(tag)
                    .ok_or_else(|| fail("invalid wire enum tag"))?;
                Value::Enum {
                    variant: variant.name.clone(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|schema| schema.decode_from(input))
                        .collect::<Result<_, _>>()?,
                }
            }
        })
    }
}

impl Function {
    pub fn validate(&self) -> Result<(), String> {
        super::validate_c_name(&self.name)?;
        if self.parameters.len() > 32 {
            return Err(fail("function exceeds 32 parameters"));
        }
        let mut bytes = 0;
        for schema in &self.parameters {
            bytes += schema.max_encoded_len()?;
        }
        if bytes > MAX_BYTES {
            return Err(fail("argument bytes exceed 65536"));
        }
        self.result.validate()
    }

    pub fn encode_arguments(&self, arguments: &[Value]) -> Result<Vec<u8>, String> {
        self.validate()?;
        if arguments.len() != self.parameters.len() {
            return Err(fail("argument count does not match the function"));
        }
        let mut bytes = Vec::new();
        for (schema, value) in self.parameters.iter().zip(arguments) {
            schema.encode_into(value, &mut bytes)?;
        }
        Ok(bytes)
    }

    pub fn decode_arguments(&self, bytes: &[u8]) -> Result<Vec<Value>, String> {
        self.validate()?;
        if bytes.len() > MAX_BYTES {
            return Err(fail("argument bytes exceed 65536"));
        }
        let mut input = bytes;
        let values = self
            .parameters
            .iter()
            .map(|schema| schema.decode_from(&mut input))
            .collect::<Result<_, _>>()?;
        if !input.is_empty() {
            return Err(fail("trailing argument bytes"));
        }
        Ok(values)
    }

    pub fn decode_result(&self, bytes: &[u8]) -> Result<Value, String> {
        self.result.decode(bytes)
    }
}

/// Check every declaration and emit explicit closed Copy roots as byte bridges.
pub fn emit_copy_library(program: &Program, exports: &[super::Export]) -> Result<Artifact, String> {
    let checked = prepare(program, exports)?;
    let (c, functions) = crate::typed::emit_copy_library(&checked, exports)?;
    let manifest = manifest(&functions)?;
    let mut header = String::from("/* Experimental Rill Copy wire ABI, version 1. */\n#include <stdint.h>\n#ifndef RILL_COPY_WIRE_ABI_VERSION\n#define RILL_COPY_WIRE_ABI_VERSION 1\n#elif RILL_COPY_WIRE_ABI_VERSION != 1\n#error \"incompatible Rill Copy wire ABI version\"\n#endif\n/* Status: 0 success, 2 malformed input, 3 invalid buffer, 4 invalid result.\n   Output capacity must fit the result schema's maximum encoded length. */\n#ifdef __cplusplus\nextern \"C\" {\n#endif\n");
    for function in &functions {
        use std::fmt::Write;
        writeln!(header, "uint32_t {}(const unsigned char *input, uint32_t input_len, unsigned char *out, uint32_t out_capacity, uint32_t *out_len);", function.name).unwrap();
    }
    header.push_str("#ifdef __cplusplus\n}\n#endif\n");
    Ok(Artifact {
        c,
        header,
        manifest,
        functions,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) struct CopyReloadArtifact {
    pub c: String,
    pub manifest: String,
    pub exports: Vec<Function>,
    pub identity: Vec<u8>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn emit_copy_reload(
    program: &Program,
    exports: &[super::Export],
    build_context: &[u8],
) -> Result<CopyReloadArtifact, String> {
    let mut exports = exports.to_vec();
    exports.sort_by(|a, b| a.c_name.cmp(&b.c_name));
    let checked = prepare(program, &exports)?;
    let (c, functions, identity) =
        crate::typed::emit_copy_reload(&checked, &exports, build_context)?;
    let manifest = manifest(&functions)?;
    Ok(CopyReloadArtifact {
        c,
        manifest,
        exports: functions,
        identity,
    })
}

pub(crate) fn manifest(functions: &[Function]) -> Result<String, String> {
    serde_json::to_string_pretty(
        &serde_json::json!({ "abi": "rill-copy-wire", "version": VERSION, "exports": functions }),
    )
    .map(|text| text + "\n")
    .map_err(|error| fail(&format!("cannot serialize manifest: {error}")))
}

fn prepare<'p>(
    program: &'p Program,
    exports: &[super::Export],
) -> Result<crate::check::CheckedProgram<'p>, String> {
    let checked = crate::check::analyze_library(program)?;
    if program.functions.iter().any(|f| f.name == "main") {
        return Err(fail("a library cannot declare executable main"));
    }
    if program.native != crate::c_import::Config::default()
        || !program.externs.is_empty()
        || !program.constants.is_empty()
        || checked.has_foreign_source_types()?
    {
        return Err(fail(
            "native dependencies, declarations and foreign values are unsupported",
        ));
    }
    if checked.has_owned() || program.allocation_types.is_some() {
        return Err(fail(
            "owner values and allocation contracts are unsupported",
        ));
    }
    if crate::functions::needs_lowering(program) {
        return Err(fail("callable values are unsupported"));
    }
    if exports.is_empty() || exports.len() > 64 {
        return Err(fail("expected 1 to 64 exports"));
    }
    let mut sources = HashSet::new();
    let mut names = HashSet::new();
    for export in exports {
        super::validate_c_name(&export.c_name)?;
        if !sources.insert(&export.source) || !names.insert(&export.c_name) {
            return Err(fail("duplicate source or external export"));
        }
        let function = program
            .functions
            .iter()
            .find(|f| f.name == export.source)
            .ok_or_else(|| fail("unknown source function"))?;
        let signature = &checked.functions[&export.source];
        if !signature.quantified.is_empty()
            || function.params.len() > 32
            || function.params.iter().any(|p| p.mode != ParamMode::Value)
        {
            return Err(fail(
                "exports require closed monomorphic value signatures with at most 32 parameters",
            ));
        }
    }
    Ok(checked)
}
