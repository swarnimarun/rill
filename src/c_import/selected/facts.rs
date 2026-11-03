//! Bounded frontend observations and a safe C assertion renderer.
//!
//! These values are data, not evidence that a current header or toolchain has
//! been checked. Deserializers must reject unknown enum tags, and callers must
//! validate the complete value against its selected names before accepting it.
//! Only fixed type spellings, checked identifiers and numeric values enter C.
use super::identifier;
use std::collections::HashSet;
use std::fmt::Write;

/// Canonical C builtins supported by the existing scalar probes. This is a
/// deliberately closed enum, not a libclang discriminant or arbitrary C text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinKind {
    Int,
    Long,
    LongLong,
    UChar,
    UShort,
    UInt,
    ULong,
    ULongLong,
    Float,
    Bool,
}

impl BuiltinKind {
    fn c_spelling(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Long => "long",
            Self::LongLong => "long long",
            Self::UChar => "unsigned char",
            Self::UShort => "unsigned short",
            Self::UInt => "unsigned int",
            Self::ULong => "unsigned long",
            Self::ULongLong => "unsigned long long",
            Self::Float => "float",
            Self::Bool => "_Bool",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalarProbe {
    pub kind: BuiltinKind,
    pub size_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstantType {
    I32,
    U8,
    U32,
    U64,
    Bool,
}

impl ConstantType {
    fn c_spelling(self) -> &'static str {
        match self {
            Self::I32 => "int32_t",
            Self::U8 => "uint8_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
            Self::Bool => "_Bool",
        }
    }
}

/// Retain libclang's numeric signedness without storing a C literal spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstantValue {
    Signed(i64),
    Unsigned(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstantFact {
    pub name: String,
    pub rill_type: ConstantType,
    pub value: ConstantValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontendFacts {
    pub target_triple: String,
    pub pointer_bits: u32,
    /// Fixed order: int32_t, uint8_t, uint32_t, uint64_t, float, _Bool.
    pub scalar_probes: [ScalarProbe; 6],
    pub constants: Vec<ConstantFact>,
}

const SCALAR_C_TYPES: [&str; 6] = [
    "int32_t", "uint8_t", "uint32_t", "uint64_t", "float", "_Bool",
];

impl FrontendFacts {
    /// Structural validation only. This neither launches tools nor establishes
    /// target agreement, freshness, or consistency with a generated module.
    pub fn validate(&self, selected_constants: &[String]) -> Result<(), String> {
        if self.target_triple.is_empty()
            || self.target_triple.len() > 256
            || self.target_triple.chars().any(char::is_control)
        {
            return Err("E-C-IMPORT: invalid frontend target triple".into());
        }
        if !(8..=8192).contains(&self.pointer_bits) || !self.pointer_bits.is_multiple_of(8) {
            return Err("E-C-IMPORT: invalid frontend pointer width".into());
        }
        for (index, probe) in self.scalar_probes.iter().enumerate() {
            let supported = match index {
                0 => matches!(
                    probe.kind,
                    BuiltinKind::Int | BuiltinKind::Long | BuiltinKind::LongLong
                ),
                1..=3 => matches!(
                    probe.kind,
                    BuiltinKind::UChar
                        | BuiltinKind::UShort
                        | BuiltinKind::UInt
                        | BuiltinKind::ULong
                        | BuiltinKind::ULongLong
                ),
                4 => probe.kind == BuiltinKind::Float,
                5 => probe.kind == BuiltinKind::Bool,
                _ => unreachable!("fixed scalar probe array"),
            };
            if !supported || !(1..=1024).contains(&probe.size_bytes) {
                return Err(format!(
                    "E-C-IMPORT: invalid frontend scalar probe {}",
                    SCALAR_C_TYPES[index]
                ));
            }
        }
        if selected_constants.len() > 256 || self.constants.len() != selected_constants.len() {
            return Err("E-C-IMPORT: frontend constant count differs from selection".into());
        }
        let mut names = HashSet::new();
        for (fact, selected) in self.constants.iter().zip(selected_constants) {
            if fact.name != *selected
                || fact.name.len() > 256
                || !identifier(&fact.name)
                || !names.insert(&fact.name)
            {
                return Err("E-C-IMPORT: frontend constant names differ from selection".into());
            }
            let value = match fact.value {
                ConstantValue::Signed(value) => i128::from(value),
                ConstantValue::Unsigned(value) => i128::from(value),
            };
            let valid = match fact.rill_type {
                ConstantType::I32 => (i128::from(i32::MIN)..=i128::from(i32::MAX)).contains(&value),
                ConstantType::U8 => (0..=i128::from(u8::MAX)).contains(&value),
                ConstantType::U32 => (0..=i128::from(u32::MAX)).contains(&value),
                ConstantType::U64 => (0..=i128::from(u64::MAX)).contains(&value),
                ConstantType::Bool => (0..=1).contains(&value),
            };
            if !valid {
                return Err(format!(
                    "E-C-IMPORT: frontend constant {} is out of range for {:?}",
                    fact.name, fact.rill_type
                ));
            }
        }
        Ok(())
    }

    /// Derive exact assertions from validated structured facts. There is no
    /// route for an artifact to supply raw C assertions or type spellings.
    /// Callers append these to ordinary emitted C with its standard headers.
    pub fn render_assertions(&self, selected_constants: &[String]) -> Result<String, String> {
        self.render(selected_constants, true)
    }

    #[cfg(feature = "c-import")]
    pub(super) fn render_legacy_assertions(
        &self,
        selected_constants: &[String],
    ) -> Result<String, String> {
        self.render(selected_constants, false)
    }

    fn render(&self, selected_constants: &[String], strict_kinds: bool) -> Result<String, String> {
        self.validate(selected_constants)?;
        let mut assertions = format!(
            "\n_Static_assert(CHAR_BIT == 8, \"E-C-IMPORT: Rill requires 8-bit bytes\");\n_Static_assert(sizeof(void *) * CHAR_BIT == {}, \"E-C-IMPORT: frontend/target pointer width mismatch\");\n",
            self.pointer_bits
        );
        for (c_type, probe) in SCALAR_C_TYPES.iter().zip(self.scalar_probes) {
            writeln!(assertions, "_Static_assert(sizeof({c_type}) == {}, \"E-C-IMPORT: frontend/target scalar width mismatch\");", probe.size_bytes).unwrap();
            if strict_kinds {
                writeln!(assertions, "_Static_assert(__builtin_types_compatible_p({c_type}, {}), \"E-C-IMPORT: frontend/target scalar kind mismatch\");", probe.kind.c_spelling()).unwrap();
            }
        }
        for fact in &self.constants {
            let name = &fact.name;
            let c_type = fact.rill_type.c_spelling();
            let value = match fact.value {
                ConstantValue::Signed(value) => format!("({value}LL)"),
                ConstantValue::Unsigned(value) => format!("{value}ULL"),
            };
            writeln!(assertions, "_Static_assert(__builtin_constant_p({name}) && __builtin_types_compatible_p(__typeof__({name}), {c_type}) && ({name}) == {value}, \"E-C-IMPORT: incompatible target constant {name}\");").unwrap();
        }
        Ok(assertions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FrontendFacts {
        FrontendFacts {
            target_triple: "x86_64-unknown-linux-gnu".into(),
            pointer_bits: 64,
            scalar_probes: [
                ScalarProbe {
                    kind: BuiltinKind::Int,
                    size_bytes: 4,
                },
                ScalarProbe {
                    kind: BuiltinKind::UChar,
                    size_bytes: 1,
                },
                ScalarProbe {
                    kind: BuiltinKind::UInt,
                    size_bytes: 4,
                },
                ScalarProbe {
                    kind: BuiltinKind::ULong,
                    size_bytes: 8,
                },
                ScalarProbe {
                    kind: BuiltinKind::Float,
                    size_bytes: 4,
                },
                ScalarProbe {
                    kind: BuiltinKind::Bool,
                    size_bytes: 1,
                },
            ],
            constants: Vec::new(),
        }
    }

    #[test]
    fn fixed_scalar_families_and_bounds_are_checked() {
        let valid = sample();
        assert!(valid.validate(&[]).is_ok());
        for width in [0, 1, 7, 9, 8193, u32::MAX] {
            let mut invalid = valid.clone();
            invalid.pointer_bits = width;
            assert!(invalid.validate(&[]).is_err());
        }
        for size in [0, 1025, u32::MAX] {
            let mut invalid = valid.clone();
            invalid.scalar_probes[0].size_bytes = size;
            assert!(invalid.validate(&[]).is_err());
        }
        for (index, kind) in [
            (0, BuiltinKind::UInt),
            (1, BuiltinKind::Int),
            (2, BuiltinKind::Float),
            (3, BuiltinKind::Bool),
            (4, BuiltinKind::UInt),
            (5, BuiltinKind::UChar),
        ] {
            let mut invalid = valid.clone();
            invalid.scalar_probes[index].kind = kind;
            assert!(invalid.validate(&[]).is_err());
        }
        for triple in [
            String::new(),
            "x".repeat(257),
            "target\n#error bad".into(),
            "x\0y".into(),
        ] {
            let mut invalid = valid.clone();
            invalid.target_triple = triple;
            assert!(invalid.validate(&[]).is_err());
        }
    }

    #[test]
    fn constants_require_exact_names_order_and_representable_values() {
        let names: Vec<String> = vec!["FIRST".into(), "SECOND".into()];
        let mut facts = sample();
        facts.constants = vec![
            ConstantFact {
                name: names[0].clone(),
                rill_type: ConstantType::I32,
                value: ConstantValue::Signed(-1),
            },
            ConstantFact {
                name: names[1].clone(),
                rill_type: ConstantType::Bool,
                value: ConstantValue::Unsigned(1),
            },
        ];
        assert!(facts.validate(&names).is_ok());
        assert!(facts.validate(&names[..1]).is_err());
        let mut invalid = facts.clone();
        invalid.constants.swap(0, 1);
        assert!(invalid.validate(&names).is_err());
        invalid = facts.clone();
        invalid.constants[1].name = names[0].clone();
        assert!(invalid
            .validate(&[names[0].clone(), names[0].clone()])
            .is_err());
        invalid = facts.clone();
        invalid.constants[0].name = "VALUE);\n#error injected".into();
        assert!(invalid
            .validate(&[invalid.constants[0].name.clone(), names[1].clone()])
            .is_err());
        for (rill_type, value) in [
            (ConstantType::I32, ConstantValue::Signed(i64::MIN)),
            (ConstantType::I32, ConstantValue::Unsigned(u64::MAX)),
            (ConstantType::U8, ConstantValue::Unsigned(256)),
            (
                ConstantType::U32,
                ConstantValue::Unsigned(u64::from(u32::MAX) + 1),
            ),
            (ConstantType::U64, ConstantValue::Signed(-1)),
            (ConstantType::Bool, ConstantValue::Signed(2)),
            (ConstantType::Bool, ConstantValue::Unsigned(2)),
        ] {
            invalid = facts.clone();
            invalid.constants[0].rill_type = rill_type;
            invalid.constants[0].value = value;
            assert!(invalid.validate(&names).is_err(), "{rill_type:?} {value:?}");
            assert!(invalid.render_assertions(&names).is_err());
        }
    }

    #[test]
    fn canonical_kind_requirements_remain_opt_in() {
        let facts = sample();
        let legacy = facts.render(&[], false).unwrap();
        let strict = facts.render_assertions(&[]).unwrap();
        assert!(legacy.contains("frontend/target scalar width mismatch"));
        assert!(!legacy.contains("frontend/target scalar kind mismatch"));
        assert!(strict.contains("frontend/target scalar kind mismatch"));
    }

    #[test]
    fn renderer_retains_exact_types_numeric_extremes_and_order() {
        let mut facts = sample();
        facts.constants = vec![
            ConstantFact {
                name: "MINIMUM".into(),
                rill_type: ConstantType::I32,
                value: ConstantValue::Signed(i64::from(i32::MIN)),
            },
            ConstantFact {
                name: "MAXIMUM".into(),
                rill_type: ConstantType::U64,
                value: ConstantValue::Unsigned(u64::MAX),
            },
            ConstantFact {
                name: "READY".into(),
                rill_type: ConstantType::Bool,
                value: ConstantValue::Unsigned(1),
            },
        ];
        let names = facts
            .constants
            .iter()
            .map(|fact| fact.name.clone())
            .collect::<Vec<_>>();
        let text = facts.render_assertions(&names).unwrap();
        assert!(text.contains("sizeof(void *) * CHAR_BIT == 64"));
        assert!(text.contains("__builtin_types_compatible_p(uint64_t, unsigned long)"));
        assert!(text.contains("__typeof__(MINIMUM), int32_t) && (MINIMUM) == (-2147483648LL)"));
        assert!(
            text.contains("__typeof__(MAXIMUM), uint64_t) && (MAXIMUM) == 18446744073709551615ULL")
        );
        assert!(text.contains("__typeof__(READY), _Bool) && (READY) == 1ULL"));
        assert!(
            text.find("incompatible target constant MINIMUM").unwrap()
                < text.find("incompatible target constant MAXIMUM").unwrap()
        );
        assert!(!text.contains(&facts.target_triple));
        facts.scalar_probes[3].kind = BuiltinKind::ULongLong;
        let changed = facts.render_assertions(&names).unwrap();
        assert!(changed.contains("__builtin_types_compatible_p(uint64_t, unsigned long long)"));
        assert_ne!(text, changed);
    }
}
