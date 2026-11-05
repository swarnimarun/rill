//! Explicit binary32 value operations. Ordinary arithmetic still defaults to i32.
use crate::interpreter::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatOp {
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Lt,
    Le,
    Gt,
    Ge,
    IsFinite,
}

impl FloatOp {
    pub(crate) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "f32_add" => Self::Add,
            "f32_sub" => Self::Sub,
            "f32_mul" => Self::Mul,
            "f32_div" => Self::Div,
            "f32_neg" => Self::Neg,
            "f32_lt" => Self::Lt,
            "f32_le" => Self::Le,
            "f32_gt" => Self::Gt,
            "f32_ge" => Self::Ge,
            "f32_is_finite" => Self::IsFinite,
            _ => return None,
        })
    }

    pub(crate) fn arity(self) -> usize {
        if matches!(self, Self::Neg | Self::IsFinite) {
            1
        } else {
            2
        }
    }

    pub(crate) fn predicate(self) -> bool {
        matches!(
            self,
            Self::Lt | Self::Le | Self::Gt | Self::Ge | Self::IsFinite
        )
    }

    pub(crate) fn evaluate(self, values: &[Value]) -> Result<Value, String> {
        if values.len() != self.arity() {
            return Err("E-FLOAT: invalid operation arity".into());
        }
        let (a, b) = match values {
            [Value::F32(a), Value::F32(b)] => (*a, *b),
            [Value::F32(a)] => (*a, 0.0),
            _ => return Err("E-FLOAT: operation requires f32 values".into()),
        };
        Ok(match self {
            Self::Add => Value::F32(a + b),
            Self::Sub => Value::F32(a - b),
            Self::Mul => Value::F32(a * b),
            Self::Div => Value::F32(a / b),
            Self::Neg => Value::F32(-a),
            Self::Lt => Value::Bool(a < b),
            Self::Le => Value::Bool(a <= b),
            Self::Gt => Value::Bool(a > b),
            Self::Ge => Value::Bool(a >= b),
            Self::IsFinite => Value::Bool(a.is_finite()),
        })
    }

    pub(crate) fn c_call(self, arguments: &[String]) -> String {
        let code = match self {
            Self::Add | Self::Lt => 0,
            Self::Sub | Self::Le => 1,
            Self::Mul | Self::Gt => 2,
            Self::Div | Self::Ge => 3,
            Self::Neg | Self::IsFinite => 4,
        };
        let helper = if self.predicate() {
            "rl_f32_predicate"
        } else {
            "rl_f32_arithmetic"
        };
        format!(
            "{helper}({code}, {}, {})",
            arguments[0],
            arguments.get(1).map(String::as_str).unwrap_or("0.0f")
        )
    }
}

/// Match the native runtime's nine-significant-digit C `%g` representation.
/// NaN payload/sign is deliberately not part of the observable text contract.
pub(crate) fn display(value: f32) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let scientific = format!("{:.8e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if (-4..9).contains(&exponent) {
        let digits = mantissa.replace('.', "");
        let position = exponent + 1;
        let mut fixed = if position <= 0 {
            format!("0.{}{}", "0".repeat((-position) as usize), digits)
        } else {
            let position = position as usize;
            if position >= digits.len() {
                let padding = "0".repeat(position - digits.len());
                digits + &padding
            } else {
                format!("{}.{}", &digits[..position], &digits[position..])
            }
        };
        if fixed.contains('.') {
            fixed = fixed.trim_end_matches('0').trim_end_matches('.').to_owned();
        }
        format!("{sign}{fixed}")
    } else {
        format!(
            "{sign}{}e{exponent:+03}",
            mantissa.trim_end_matches('0').trim_end_matches('.')
        )
    }
}

pub(crate) const STRICT: &str = r#"
#if defined(__FAST_MATH__) || (defined(__FINITE_MATH_ONLY__) && __FINITE_MATH_ONLY__ > 0)
#error "Rill f32 operations require strict floating-point flags; remove fast/finite-only math"
#endif
_Static_assert(sizeof(float) == 4 && FLT_RADIX == 2 && FLT_MANT_DIG == 24 && FLT_MAX_EXP == 128 && FLT_MIN_EXP == -125, "Rill f32 requires IEEE binary32");
"#;

pub(crate) const RUNTIME: &str = r#"
#include <float.h>
static float rl_f32_from_i32(int32_t value) { volatile float result = (float)value; return result; }
static float rl_f32_arithmetic(int op, float a, float b) {
    switch (op) {
    case 0: { volatile float result = a + b; return result; }
    case 1: { volatile float result = a - b; return result; }
    case 2: { volatile float result = a * b; return result; }
    case 3: { volatile float result = a / b; return result; }
    case 4: { volatile float result = -a; return result; }
    default: rl_trap(); abort();
    }
}
static int rl_f32_predicate(int op, float a, float b) {
    switch (op) {
    case 0: return a < b;
    case 1: return a <= b;
    case 2: return a > b;
    case 3: return a >= b;
    case 4: return a == a && a <= 0x1.fffffep127f && a >= -0x1.fffffep127f;
    default: rl_trap(); abort();
    }
}
static void rl_print_f32(float value) {
    if (value != value) fputs("nan", stdout);
    else printf("%.9g", (double)value);
}
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn float_display_has_native_precision_and_special_value_policy() {
        for (value, expected) in [
            (0.1_f32, "0.100000001"),
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (1e8, "100000000"),
            (1e9, "1e+09"),
            (f32::INFINITY, "inf"),
            (f32::NEG_INFINITY, "-inf"),
            (f32::NAN, "nan"),
            (f32::from_bits(1), "1.40129846e-45"),
            (f32::MAX, "3.40282347e+38"),
        ] {
            assert_eq!(super::display(value), expected);
        }
    }
}
