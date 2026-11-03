//! Fixed-order bounded binary records. A successful decode is never a proof.
use super::*;
use crate::c_import::{
    selected::facts::{BuiltinKind, ConstantFact, ConstantType, ConstantValue, ScalarProbe},
    Config,
};
use std::os::unix::ffi::OsStringExt;
const MAGIC: &[u8] = b"RILL-NATIVE-CONFIG\0\x01";
const PREPARATION_MAGIC: &[u8] = b"RILL-NATIVE-PREPARATION\0\x01";

struct Encoder(Vec<u8>);
impl Encoder {
    fn number(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }
    fn tag(&mut self, value: u8) {
        self.0.push(value);
    }
    fn bytes(&mut self, bytes: &[u8], module: bool) -> Result<(), String> {
        let limit = if module { MAX_MODULE } else { MAX_ARGUMENT };
        if bytes.len() > limit {
            return Err(error("encoded field exceeds its byte limit"));
        }
        if self
            .0
            .len()
            .checked_add(4)
            .is_none_or(|size| size > MAX_METADATA)
        {
            return Err(error("encoded metadata exceeds 512 KiB"));
        }
        self.number(bytes.len() as u32);
        self.0.extend(bytes);
        self.bound(module)
    }
    fn bound(&self, module: bool) -> Result<(), String> {
        if self.0.len() > if module { MAX_ARTIFACT } else { MAX_METADATA } {
            return Err(error("encoded record exceeds metadata/artifact byte limit"));
        }
        Ok(())
    }
    fn list(&mut self, values: &[OsString]) -> Result<(), String> {
        if values.len() > MAX_ITEMS {
            return Err(error("encoded list exceeds 1024 items"));
        }
        self.number(values.len() as u32);
        for value in values {
            self.bytes(value.as_bytes(), false)?;
        }
        self.bound(false)
    }
    fn strings(&mut self, values: &[String]) -> Result<(), String> {
        if values.len() > MAX_ITEMS {
            return Err(error("encoded list exceeds 1024 items"));
        }
        self.number(values.len() as u32);
        for value in values {
            self.bytes(value.as_bytes(), false)?;
        }
        self.bound(false)
    }
    fn selection(&mut self, selection: &Selection) -> Result<(), String> {
        for values in [
            &selection.native.includes,
            &selection.native.links,
            &selection.native.packages,
            &selection.functions,
            &selection.constants,
        ] {
            self.strings(values)?;
        }
        self.list(&selection.c_args)?;
        match &selection.resource_dir {
            None => self.tag(0),
            Some(path) => {
                self.tag(1);
                self.bytes(path.as_os_str().as_bytes(), false)?;
            }
        }
        Ok(())
    }
    fn resolved(&mut self, resolved: &Resolved) -> Result<(), String> {
        self.list(&resolved.cflags)?;
        self.list(&resolved.link_args)
    }
    fn facts(&mut self, facts: &FrontendFacts) -> Result<(), String> {
        self.bytes(facts.target_triple.as_bytes(), false)?;
        self.number(facts.pointer_bits);
        for probe in facts.scalar_probes {
            self.tag(kind_tag(probe.kind));
            self.number(probe.size_bytes);
        }
        self.number(facts.constants.len() as u32);
        for fact in &facts.constants {
            self.bytes(fact.name.as_bytes(), false)?;
            self.tag(match fact.rill_type {
                ConstantType::I32 => 0,
                ConstantType::U8 => 1,
                ConstantType::U32 => 2,
                ConstantType::U64 => 3,
                ConstantType::Bool => 4,
            });
            match fact.value {
                ConstantValue::Signed(value) => {
                    self.tag(0);
                    self.0.extend(value.to_le_bytes());
                }
                ConstantValue::Unsigned(value) => {
                    self.tag(1);
                    self.0.extend(value.to_le_bytes());
                }
            }
        }
        self.bound(false)
    }
}
struct Decoder<'a> {
    remaining: &'a [u8],
    consumed: usize,
}
impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() > MAX_ARTIFACT {
            return Err(error("artifact exceeds 2 MiB"));
        }
        Ok(Self {
            remaining: bytes,
            consumed: 0,
        })
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        if size > self.remaining.len() {
            return Err(error("truncated native record"));
        }
        let (value, remaining) = self.remaining.split_at(size);
        self.remaining = remaining;
        self.consumed += size;
        Ok(value)
    }
    fn tag(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn number(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn bytes(&mut self, module: bool) -> Result<&'a [u8], String> {
        let size = self.number()? as usize;
        if size > if module { MAX_MODULE } else { MAX_ARGUMENT } {
            return Err(error("native field exceeds byte limit"));
        }
        if self.consumed > MAX_METADATA
            || (!module && size > MAX_METADATA.saturating_sub(self.consumed))
        {
            return Err(error("native metadata exceeds 512 KiB"));
        }
        self.take(size)
    }
    fn string(&mut self, module: bool) -> Result<String, String> {
        std::str::from_utf8(self.bytes(module)?)
            .map(str::to_owned)
            .map_err(|_| error("native text field is not UTF-8"))
    }
    fn os(&mut self) -> Result<OsString, String> {
        Ok(OsString::from_vec(self.bytes(false)?.to_vec()))
    }
    fn list(&mut self) -> Result<Vec<OsString>, String> {
        let size = self.number()? as usize;
        if size > MAX_ITEMS || size > self.remaining.len() / 4 {
            return Err(error("invalid native list count"));
        }
        (0..size).map(|_| self.os()).collect()
    }
    fn strings(&mut self) -> Result<Vec<String>, String> {
        self.list()?
            .into_iter()
            .map(|s| {
                s.into_string()
                    .map_err(|_| error("native name is not UTF-8"))
            })
            .collect()
    }
    fn selection(&mut self) -> Result<Selection, String> {
        let native = Config {
            includes: self.strings()?,
            links: self.strings()?,
            packages: self.strings()?,
        };
        let functions = self.strings()?;
        let constants = self.strings()?;
        let c_args = self.list()?;
        let resource_dir = match self.tag()? {
            0 => None,
            1 => Some(PathBuf::from(self.os()?)),
            _ => return Err(error("invalid resource-directory tag")),
        };
        Ok(Selection {
            native,
            functions,
            constants,
            c_args,
            resource_dir,
        })
    }
    fn resolved(&mut self) -> Result<Resolved, String> {
        Ok(Resolved {
            cflags: self.list()?,
            link_args: self.list()?,
        })
    }
    fn facts(&mut self) -> Result<FrontendFacts, String> {
        let target_triple = self.string(false)?;
        let pointer_bits = self.number()?;
        let mut scalar_probes = [ScalarProbe {
            kind: BuiltinKind::Int,
            size_bytes: 0,
        }; 6];
        for probe in &mut scalar_probes {
            probe.kind = parse_kind(self.tag()?)?;
            probe.size_bytes = self.number()?;
        }
        let count = self.number()? as usize;
        if count > 256 || count > self.remaining.len() / 14 {
            return Err(error("invalid constant fact count"));
        }
        let mut constants = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.string(false)?;
            let rill_type = match self.tag()? {
                0 => ConstantType::I32,
                1 => ConstantType::U8,
                2 => ConstantType::U32,
                3 => ConstantType::U64,
                4 => ConstantType::Bool,
                _ => return Err(error("unknown constant type")),
            };
            let value = match self.tag()? {
                0 => ConstantValue::Signed(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
                1 => ConstantValue::Unsigned(u64::from_le_bytes(self.take(8)?.try_into().unwrap())),
                _ => return Err(error("unknown constant value tag")),
            };
            constants.push(ConstantFact {
                name,
                rill_type,
                value,
            });
        }
        if self.consumed > MAX_METADATA {
            return Err(error("native metadata exceeds 512 KiB"));
        }
        Ok(FrontendFacts {
            target_triple,
            pointer_bits,
            scalar_probes,
            constants,
        })
    }
    fn done(self) -> Result<(), String> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(error("trailing native record bytes"))
        }
    }
}
fn kind_tag(kind: BuiltinKind) -> u8 {
    match kind {
        BuiltinKind::Int => 0,
        BuiltinKind::Long => 1,
        BuiltinKind::LongLong => 2,
        BuiltinKind::UChar => 3,
        BuiltinKind::UShort => 4,
        BuiltinKind::UInt => 5,
        BuiltinKind::ULong => 6,
        BuiltinKind::ULongLong => 7,
        BuiltinKind::Float => 8,
        BuiltinKind::Bool => 9,
    }
}
fn parse_kind(tag: u8) -> Result<BuiltinKind, String> {
    Ok(match tag {
        0 => BuiltinKind::Int,
        1 => BuiltinKind::Long,
        2 => BuiltinKind::LongLong,
        3 => BuiltinKind::UChar,
        4 => BuiltinKind::UShort,
        5 => BuiltinKind::UInt,
        6 => BuiltinKind::ULong,
        7 => BuiltinKind::ULongLong,
        8 => BuiltinKind::Float,
        9 => BuiltinKind::Bool,
        _ => return Err(error("unknown canonical scalar kind")),
    })
}

pub(super) fn encode(config: &UnverifiedConfig) -> Result<Vec<u8>, String> {
    config.validate()?;
    let mut out = Encoder(MAGIC.to_vec());
    out.number(config.generator_contract);
    out.tag(0); // Unix bytes, schema v1 only.
    match &config.work_directory {
        WorkDirectory::RelativeToArtifactParent(path) => {
            out.tag(0);
            out.bytes(path.as_os_str().as_bytes(), false)?;
        }
        WorkDirectory::Absolute(path) => {
            out.tag(1);
            out.bytes(path.as_os_str().as_bytes(), false)?;
        }
    }
    out.tag(match config.compiler_assertion.driver {
        Driver::C => 0,
        Driver::ZigCc => 1,
    });
    out.bytes(
        config
            .compiler_assertion
            .invocation_path
            .as_os_str()
            .as_bytes(),
        false,
    )?;
    out.bytes(config.binding_module.as_bytes(), false)?;
    out.selection(&config.selection)?;
    out.list(&config.declared_link_args)?;
    out.resolved(&config.expected_resolution)?;
    out.facts(&config.expected_frontend)?;
    out.bytes(config.module_utf8.as_bytes(), true)?;
    Ok(out.0)
}
pub(super) fn decode(bytes: &[u8]) -> Result<UnverifiedConfig, String> {
    let mut input = Decoder::new(bytes)?;
    if input.take(MAGIC.len())? != MAGIC {
        return Err(error("unknown native configuration magic/schema"));
    }
    let generator_contract = input.number()?;
    if input.tag()? != 0 {
        return Err(error("unsupported path encoding"));
    }
    let work_directory = match input.tag()? {
        0 => WorkDirectory::RelativeToArtifactParent(input.os()?.into()),
        1 => WorkDirectory::Absolute(input.os()?.into()),
        _ => return Err(error("unknown working-directory kind")),
    };
    let driver = match input.tag()? {
        0 => Driver::C,
        1 => Driver::ZigCc,
        _ => return Err(error("unknown compiler driver")),
    };
    let compiler_assertion = CompilerAssertion {
        driver,
        invocation_path: input.os()?.into(),
    };
    let binding_module = input.string(false)?;
    let selection = input.selection()?;
    let declared_link_args = input.list()?;
    let expected_resolution = input.resolved()?;
    let expected_frontend = input.facts()?;
    let module_utf8 = input.string(true)?;
    input.done()?;
    let result = UnverifiedConfig {
        generator_contract,
        work_directory,
        compiler_assertion,
        binding_module,
        selection,
        declared_link_args,
        expected_resolution,
        expected_frontend,
        module_utf8,
    };
    result.validate()?;
    Ok(result)
}
/// Worker-specific frame: complete, typed data without an executable or output path.
pub fn encode_preparation(
    preparation: &Preparation,
    selection: &Selection,
) -> Result<Vec<u8>, String> {
    validate_preparation(preparation, selection)?;
    let mut out = Encoder(PREPARATION_MAGIC.to_vec());
    out.resolved(&preparation.resolved)?;
    out.list(&preparation.cflags)?;
    out.facts(&preparation.facts)?;
    out.bytes(preparation.module.as_bytes(), true)?;
    Ok(out.0)
}
pub fn decode_preparation(bytes: &[u8], selection: &Selection) -> Result<Preparation, String> {
    let mut input = Decoder::new(bytes)?;
    if input.take(PREPARATION_MAGIC.len())? != PREPARATION_MAGIC {
        return Err(error("unknown native preparation frame"));
    }
    let resolved = input.resolved()?;
    let cflags = input.list()?;
    let facts = input.facts()?;
    let module = input.string(true)?;
    input.done()?;
    let result = Preparation {
        module,
        resolved,
        cflags,
        facts,
    };
    validate_preparation(&result, selection)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> UnverifiedConfig {
        UnverifiedConfig{
            generator_contract:1,work_directory:WorkDirectory::RelativeToArtifactParent("..".into()),
            compiler_assertion:CompilerAssertion{driver:Driver::C,invocation_path:"/usr/bin/cc".into()},binding_module:"native.bindings".into(),
            selection:Selection{native:Config{includes:vec!["api.h".into()],..Default::default()},functions:vec!["answer".into()],constants:vec!["ANSWER".into()],c_args:vec!["-I".into(),OsString::from_vec(b"a '\\ \xff".to_vec()),"-O2".into()],resource_dir:Some(PathBuf::from(OsString::from_vec(b"resource \xfe".to_vec())))},
            declared_link_args:vec!["/tmp/input.o".into()],expected_resolution:Resolved{cflags:vec!["-DVALUE=1".into()],link_args:vec!["-la".into(),"-lb".into(),"-la".into()]},
            expected_frontend:FrontendFacts{target_triple:"x86_64-unknown-linux-gnu".into(),pointer_bits:64,scalar_probes:[ScalarProbe{kind:BuiltinKind::Int,size_bytes:4},ScalarProbe{kind:BuiltinKind::UChar,size_bytes:1},ScalarProbe{kind:BuiltinKind::UInt,size_bytes:4},ScalarProbe{kind:BuiltinKind::ULong,size_bytes:8},ScalarProbe{kind:BuiltinKind::Float,size_bytes:4},ScalarProbe{kind:BuiltinKind::Bool,size_bytes:1}],constants:vec![ConstantFact{name:"ANSWER".into(),rill_type:ConstantType::I32,value:ConstantValue::Signed(42)}]},
            module_utf8:"// target comment\n@include(\"api.h\");\nexport extern fn answer() -> i32;\nexport extern const ANSWER: i32;\n".into(),
        }
    }
    #[test]
    fn metadata_bound_includes_the_module_length_prefix() {
        for extra in 0..=4 {
            let start = MAX_METADATA - 4 + extra;
            let mut encoder = Encoder(vec![0; start]);
            let encoded = encoder.bytes(b"x", true);
            let mut decoder = Decoder {
                remaining: &[1, 0, 0, 0, b'x'],
                consumed: start,
            };
            let decoded = decoder.bytes(true);
            assert_eq!(encoded.is_ok(), decoded.is_ok(), "metadata prefix {start}");
            assert_eq!(encoded.is_ok(), extra == 0);
        }
    }
    #[test]
    fn resource_operands_and_fact_declaration_type_must_agree() {
        for resource in ["@response", "-E", ""] {
            let mut c = fixture();
            c.selection.resource_dir = Some(resource.into());
            assert!(c.encode().is_err(), "{resource}");
        }
        let mut c = fixture();
        c.expected_frontend.constants[0].rill_type = ConstantType::U32;
        assert!(c.encode().unwrap_err().contains("fact type"));
        let mut c = fixture();
        c.expected_resolution.link_args = vec!["-la".into(); MAX_ITEMS];
        assert!(c.encode().unwrap_err().contains("combined link"));
    }

    #[test]
    fn deterministic_byte_roundtrip_and_escaped_inspection() {
        let c = fixture();
        let bytes = c.encode().unwrap();
        assert_eq!(bytes, c.encode().unwrap());
        assert_eq!(c, UnverifiedConfig::decode(&bytes).unwrap());
        let inspect = c.inspect().unwrap();
        assert!(inspect.contains("\\xff"));
        assert!(!inspect.contains('\u{fffd}'));
    }
    #[test]
    fn every_truncation_and_unknown_or_trailing_data_rejects() {
        let bytes = fixture().encode().unwrap();
        for end in 0..bytes.len() {
            assert!(UnverifiedConfig::decode(&bytes[..end]).is_err(), "{end}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(UnverifiedConfig::decode(&extra).is_err());
        for index in [0, MAGIC.len() - 1, MAGIC.len() + 4, MAGIC.len() + 5] {
            let mut bad = bytes.clone();
            bad[index] = 255;
            assert!(UnverifiedConfig::decode(&bad).is_err(), "{index}");
        }
        let mut bad = bytes;
        bad[MAGIC.len() + 6..MAGIC.len() + 10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(UnverifiedConfig::decode(&bad).is_err());
        assert!(UnverifiedConfig::decode(&vec![0; MAX_ARTIFACT + 1]).is_err());
    }
    #[test]
    fn bounded_metadata_and_module_and_selection() {
        let mut c = fixture();
        c.module_utf8 = "x".repeat(MAX_MODULE + 1);
        assert!(c.encode().is_err());
        let mut c = fixture();
        c.selection.c_args = vec!["-pthread".into(); MAX_ITEMS + 1];
        assert!(c.encode().is_err());
        let mut c = fixture();
        c.selection.c_args = (0..9)
            .flat_map(|_| ["-I".into(), OsString::from("x".repeat(MAX_ARGUMENT))])
            .collect();
        assert!(c.encode().is_err());
        let mut c = fixture();
        c.compiler_assertion.invocation_path =
            PathBuf::from(OsString::from_vec(b"/tmp/c\0c".to_vec()));
        assert!(c.encode().is_err());
    }
    #[test]
    fn fact_and_binding_shape_corruption_rejects() {
        for name in ["std.bindings", "@entry", "a..b", "../a", "a/b", "Upper"] {
            let mut c = fixture();
            c.binding_module = name.into();
            assert!(c.encode().is_err(), "{name}");
        }
        let mut c = fixture();
        c.expected_frontend.constants[0].name = "OTHER".into();
        assert!(c.encode().is_err());
        let mut c = fixture();
        c.expected_frontend.constants[0].rill_type = ConstantType::Bool;
        c.expected_frontend.constants[0].value = ConstantValue::Signed(2);
        assert!(c.encode().is_err());
        for text in [
            "fn evil() { evil() }",
            "type Evil<a> = Bad(Evil<a>)",
            "export extern fn wrong() -> i32;",
        ] {
            let mut c = fixture();
            c.module_utf8 = text.into();
            assert!(c.encode().is_err());
        }
    }
    #[test]
    fn preparation_is_a_separate_exact_frame() {
        let c = fixture();
        let p = c.preparation();
        let bytes = encode_preparation(&p, &c.selection).unwrap();
        let decoded = decode_preparation(&bytes, &c.selection).unwrap();
        assert_eq!(decoded.module, p.module);
        assert_eq!(decoded.facts, p.facts);
        assert_eq!(decoded.resolved, p.resolved);
        assert_eq!(decoded.cflags, p.cflags);
        for end in 0..bytes.len() {
            assert!(decode_preparation(&bytes[..end], &c.selection).is_err());
        }
        let mut p = c.preparation();
        p.cflags.reverse();
        assert!(encode_preparation(&p, &c.selection).is_err());
        assert!(UnverifiedConfig::decode(&bytes).is_err());
    }
    #[test]
    fn fresh_expectations_compare_order_values_and_bytes() {
        let c = fixture();
        c.compare_fresh(&c.preparation()).unwrap();
        let mut p = c.preparation();
        p.resolved.link_args.reverse();
        p.resolved.link_args.push("-la".into());
        assert!(c.compare_fresh(&p).unwrap_err().contains("DRIFT"));
        let mut p = c.preparation();
        p.facts.constants[0].value = ConstantValue::Signed(41);
        assert!(c.compare_fresh(&p).unwrap_err().contains("DRIFT"));
        let mut p = c.preparation();
        p.module.push_str("// edit\n");
        assert!(c.compare_fresh(&p).unwrap_err().contains("DRIFT"));
        let mut compiler = c.compiler_assertion.clone();
        compiler.invocation_path = "/tmp/other".into();
        assert!(c.compare_compiler(&compiler).is_err());
    }
}
