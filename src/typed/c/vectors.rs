//! Compact vector storage with portable scalar and explicit NEON operations.
use super::*;
use crate::typed::VectorOp;

pub(super) const RUNTIME: &str = r#"
typedef struct { uint8_t data[16]; } rl_u8x16;
_Static_assert(sizeof(rl_u8x16) == 16, "unsupported u8x16 layout");
static rl_u8x16 rl_vector_load(const unsigned char *bytes, size_t length, int32_t offset) {
    if (offset < 0 || length < 16 || (size_t)offset > length - 16) rl_trap();
    rl_u8x16 result;
    for (size_t i = 0; i < 16; ++i) result.data[i] = bytes[(size_t)offset + i];
    return result;
}
static int32_t rl_vector_lane(rl_u8x16 value, int32_t index) {
    if (index < 0 || index >= 16) rl_trap();
    return (int32_t)value.data[index];
}
static rl_u8x16 rl_vector_binary(int op, rl_u8x16 a, rl_u8x16 b) {
    rl_u8x16 result;
#if defined(RILL_SIMD_NEON)
    uint8x16_t x = vld1q_u8(a.data), y = vld1q_u8(b.data), z;
    switch (op) {
    case 0: z = vandq_u8(x, y); break;
    case 1: z = vorrq_u8(x, y); break;
    case 2: z = veorq_u8(x, y); break;
    case 3: z = vceqq_u8(x, y); break;
    default: rl_trap(); z = vdupq_n_u8(0);
    }
    vst1q_u8(result.data, z);
#else
    for (size_t i = 0; i < 16; ++i) {
        switch (op) {
        case 0: result.data[i] = (uint8_t)(a.data[i] & b.data[i]); break;
        case 1: result.data[i] = (uint8_t)(a.data[i] | b.data[i]); break;
        case 2: result.data[i] = (uint8_t)(a.data[i] ^ b.data[i]); break;
        case 3: result.data[i] = a.data[i] == b.data[i] ? 255 : 0; break;
        default: rl_trap(); result.data[i] = 0;
        }
    }
#endif
    return result;
}
static int rl_vector_any(rl_u8x16 value) {
    for (size_t i = 0; i < 16; ++i) if (value.data[i]) return 1;
    return 0;
}
static int rl_vector_all(rl_u8x16 value) {
    for (size_t i = 0; i < 16; ++i) if (!value.data[i]) return 0;
    return 1;
}
static void rl_print_vector(rl_u8x16 value) {
    fputs("u8x16[", stdout);
    for (size_t i = 0; i < 16; ++i) {
        if (i) fputs(", ", stdout);
        printf("%u", (unsigned)value.data[i]);
    }
    fputc(']', stdout);
}
"#;

impl Emitter<'_> {
    pub(super) fn vector(&mut self, result: &str, ty: &str, op: VectorOp, args: &[ExprNode]) {
        let values = args.iter().map(|arg| self.expr(arg)).collect::<Vec<_>>();
        let call = match op {
            VectorOp::Load => {
                let Type::Bytes(n) = args[0].ty else {
                    unreachable!()
                };
                format!("rl_vector_load({}.data, {n}, {})", values[0], values[1])
            }
            VectorOp::Lane => format!("rl_vector_lane({}, {})", values[0], values[1]),
            VectorOp::Any => format!("rl_vector_any({})", values[0]),
            VectorOp::All => format!("rl_vector_all({})", values[0]),
            op => {
                let index = match op {
                    VectorOp::And => 0,
                    VectorOp::Or => 1,
                    VectorOp::Xor => 2,
                    VectorOp::Eq => 3,
                    _ => unreachable!(),
                };
                format!("rl_vector_binary({index}, {}, {})", values[0], values[1])
            }
        };
        writeln!(self.out, "{ty} {result} = {call};").unwrap();
    }
}
