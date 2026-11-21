//! Closed signatures and evaluation for the existing sixteen-byte Copy vector.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorOp {
    Load,
    Lane,
    And,
    Or,
    Xor,
    Eq,
    Any,
    All,
}
impl VectorOp {
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "u8x16_load" => Self::Load,
            "u8x16_lane" => Self::Lane,
            "u8x16_and" => Self::And,
            "u8x16_or" => Self::Or,
            "u8x16_xor" => Self::Xor,
            "u8x16_eq" => Self::Eq,
            "u8x16_any" => Self::Any,
            "u8x16_all" => Self::All,
            _ => return None,
        })
    }
    pub(super) fn result(self, args: &[Type]) -> Result<Type, String> {
        match (self, args) {
            (Self::Load, [Type::Bytes(n), Type::Int]) if *n <= 256 => Ok(Type::U8x16),
            (Self::Lane, [Type::U8x16, Type::Int]) => Ok(Type::Int),
            (Self::And | Self::Or | Self::Xor | Self::Eq, [Type::U8x16, Type::U8x16]) => {
                Ok(Type::U8x16)
            }
            (Self::Any | Self::All, [Type::U8x16]) => Ok(Type::Bool),
            _ => Err("E-TYPED-VECTOR: invalid operation operands or arity".into()),
        }
    }
}

pub(super) fn evaluate(
    op: VectorOp,
    args: &[ExprNode],
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<crate::interpreter::Value, String> {
    use crate::interpreter::Value;
    let values = args
        .iter()
        .map(|arg| evaluate_expr(arg, locals, program, depth, state, owners))
        .collect::<Result<Vec<_>, _>>()?;
    let trap = || "runtime trap".to_string();
    match (op, values.as_slice()) {
        (VectorOp::Load, [Value::Bytes(bytes), Value::Int(offset)]) => {
            if *offset < 0 || bytes.len() < 16 || *offset as usize > bytes.len() - 16 {
                return Err(trap());
            }
            let mut lanes = [0; 16];
            lanes.copy_from_slice(&bytes[*offset as usize..*offset as usize + 16]);
            Ok(Value::U8x16(lanes))
        }
        (VectorOp::Lane, [Value::U8x16(lanes), Value::Int(index)]) => {
            if !(0..16).contains(index) {
                return Err(trap());
            }
            Ok(Value::Int(i32::from(lanes[*index as usize])))
        }
        (VectorOp::Any, [Value::U8x16(lanes)]) => Ok(Value::Bool(lanes.iter().any(|x| *x != 0))),
        (VectorOp::All, [Value::U8x16(lanes)]) => Ok(Value::Bool(lanes.iter().all(|x| *x != 0))),
        (
            VectorOp::And | VectorOp::Or | VectorOp::Xor | VectorOp::Eq,
            [Value::U8x16(a), Value::U8x16(b)],
        ) => {
            let mut lanes = [0; 16];
            for (i, lane) in lanes.iter_mut().enumerate() {
                *lane = match op {
                    VectorOp::And => a[i] & b[i],
                    VectorOp::Or => a[i] | b[i],
                    VectorOp::Xor => a[i] ^ b[i],
                    VectorOp::Eq => {
                        if a[i] == b[i] {
                            255
                        } else {
                            0
                        }
                    }
                    _ => unreachable!(),
                };
            }
            Ok(Value::U8x16(lanes))
        }
        _ => Err("E-TYPED-VECTOR: invalid runtime vector operands".into()),
    }
}
