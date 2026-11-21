//! Hosted allocation operations and their ordinary Result construction.
use super::*;
use crate::typed::{AllocationResultLayout, SystemAllocationKind};

pub(super) const RUNTIME: &str = r#"
typedef struct { size_t len; unsigned char data[]; } rl_owned_bytes;
_Static_assert(sizeof(int32_t *) <= 64 && sizeof(rl_owned_bytes *) <= 64,
               "unsupported owner pointer layout");
/* Volatile slots preserve observable allocation attempts and release. */
static void *(*volatile rl_allocate)(size_t) = malloc;
static void (*volatile rl_release)(void *) = free;
enum rl_allocation_status {
    RL_ALLOC_OK, RL_ALLOC_INVALID_LENGTH, RL_ALLOC_SIZE_OVERFLOW, RL_ALLOC_OUT_OF_MEMORY
};

/* Check before narrowing or adding; explicit bounds also allow target-size tests. */
static int rl_owned_bytes_size(int32_t n, size_t header, size_t maximum, size_t *total) {
    if (n < 0) return RL_ALLOC_INVALID_LENGTH;
    if (header > maximum || (uintmax_t)n > (uintmax_t)(maximum - header))
        return RL_ALLOC_SIZE_OVERFLOW;
    *total = header + (size_t)n;
    return RL_ALLOC_OK;
}
static int32_t *rl_box_i32_try(int32_t value) {
    int32_t *p = rl_allocate(sizeof(*p));
    if (p) *p = value;
    return p;
}
static rl_owned_bytes *rl_owned_bytes_try(int32_t n, int *status) {
    size_t total;
    *status = rl_owned_bytes_size(n, sizeof(rl_owned_bytes), SIZE_MAX, &total);
    if (*status != RL_ALLOC_OK) return NULL;
    rl_owned_bytes *p = rl_allocate(total);
    if (!p) { *status = RL_ALLOC_OUT_OF_MEMORY; return NULL; }
    p->len = (size_t)n;
    for (size_t i = 0; i < p->len; ++i) p->data[i] = 0;
    return p;
}
static int32_t *rl_box_i32_abort(int32_t value) {
    int32_t *p = rl_box_i32_try(value);
    if (!p) rl_trap();
    return p;
}
static rl_owned_bytes *rl_owned_bytes_new(int32_t n) {
    int status;
    rl_owned_bytes *p = rl_owned_bytes_try(n, &status);
    if (status != RL_ALLOC_OK) rl_trap();
    return p;
}
static int32_t rl_owned_bytes_get(const rl_owned_bytes *p, int32_t i) {
    if (i < 0 || (uintmax_t)i >= (uintmax_t)p->len) rl_trap();
    return p->data[(size_t)i];
}
static void rl_owned_bytes_set(rl_owned_bytes *p, int32_t i, int32_t v) {
    if (i < 0 || (uintmax_t)i >= (uintmax_t)p->len || v < 0 || v > 255) rl_trap();
    p->data[(size_t)i] = (unsigned char)v;
}
"#;

impl Emitter<'_> {
    pub(super) fn try_allocation(
        &mut self,
        result: &str,
        kind: SystemAllocationKind,
        input: &ExprNode,
        layout: &AllocationResultLayout,
    ) -> EmitOutcome {
        let input = emit_value!(self.expr(input));
        let pointer = self.temp();
        let status = self.temp();
        let owner = match kind {
            SystemAllocationKind::BoxI32 => Type::OwnedI32,
            SystemAllocationKind::BytesZeroed => Type::OwnedBytes,
        };
        self.add_cost(owner);
        self.add_cost(Type::Int);
        writeln!(self.out, "int {status} = RL_ALLOC_OK;").unwrap();
        match kind {
            SystemAllocationKind::BoxI32 => {
                writeln!(self.out, "int32_t *{pointer} = rl_box_i32_try({input});").unwrap();
                writeln!(
                    self.out,
                    "if (!{pointer}) {status} = RL_ALLOC_OUT_OF_MEMORY;"
                )
                .unwrap();
            }
            SystemAllocationKind::BytesZeroed => {
                writeln!(
                    self.out,
                    "rl_owned_bytes *{pointer} = rl_owned_bytes_try({input}, &{status});"
                )
                .unwrap();
            }
        }
        let result_type = c_type(Type::Adt {
            id: layout.result_adt,
            owned: true,
        });
        let error_type = c_type(Type::Adt {
            id: layout.error_adt,
            owned: false,
        });
        writeln!(self.out, "{result_type} {result} = ({result_type}){{0}};").unwrap();
        writeln!(self.out, "if ({status} == RL_ALLOC_OK) {{").unwrap();
        writeln!(
            self.out,
            "{result}.tag = {}; {result}.payload.v{}.f0 = {pointer};",
            layout.ok_variant, layout.ok_variant
        )
        .unwrap();
        writeln!(
            self.out,
            "}} else {{ {result}.tag = {}; switch ({status}) {{",
            layout.err_variant
        )
        .unwrap();
        for (status, tag) in [
            ("RL_ALLOC_INVALID_LENGTH", layout.invalid_length_variant),
            ("RL_ALLOC_SIZE_OVERFLOW", layout.size_overflow_variant),
            ("RL_ALLOC_OUT_OF_MEMORY", layout.out_of_memory_variant),
        ] {
            writeln!(
                self.out,
                "case {status}: {result}.payload.v{}.f0 = ({error_type}){{.tag = {tag}}}; break;",
                layout.err_variant
            )
            .unwrap();
        }
        self.out.push_str("default: rl_trap(); } }\n");
        EmitOutcome::Value(result.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    #[test]
    fn c_size_checks_precede_narrowing_and_addition() {
        let dir = std::env::temp_dir().join(format!("rill-c-alloc-size-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let source = dir.join("sizes.c");
        let binary = dir.join("sizes");
        fs::write(
            &source,
            format!(
                "{}{}{}",
                super::super::RUNTIME,
                super::RUNTIME,
                r#"
int main(void) {
    size_t total = 123;
    /* The 16-bit cases force representability failures on larger hosts. */
    if (rl_owned_bytes_size(65527, 8, 65535, &total) != RL_ALLOC_OK || total != 65535) return 1;
    total = 123;
    if (rl_owned_bytes_size(65528, 8, 65535, &total) != RL_ALLOC_SIZE_OVERFLOW || total != 123) return 2;
    if (rl_owned_bytes_size(INT32_MAX, 8, 65535, &total) != RL_ALLOC_SIZE_OVERFLOW || total != 123) return 3;
    if (rl_owned_bytes_size(INT32_MIN, 8, 65535, &total) != RL_ALLOC_INVALID_LENGTH || total != 123) return 4;
    if (rl_owned_bytes_size(0, 16, 15, &total) != RL_ALLOC_SIZE_OVERFLOW || total != 123) return 5;
    if (rl_owned_bytes_size(1, SIZE_MAX, SIZE_MAX, &total) != RL_ALLOC_SIZE_OVERFLOW || total != 123) return 6;
    if (rl_owned_bytes_size(0, SIZE_MAX, SIZE_MAX, &total) != RL_ALLOC_OK || total != SIZE_MAX) return 7;
    if (rl_owned_bytes_size(0, sizeof(rl_owned_bytes), SIZE_MAX, &total) != RL_ALLOC_OK || total != sizeof(rl_owned_bytes)) return 8;
    return 0;
}
"#,
            ),
        )
        .unwrap();
        for optimization in ["-O0", "-O2"] {
            let build = Command::new("cc")
                .args(["-std=c11", optimization])
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let output = Command::new(&binary).output().unwrap();
            assert!(
                output.status.success(),
                "size fixture: {:?}",
                output.status.code()
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
