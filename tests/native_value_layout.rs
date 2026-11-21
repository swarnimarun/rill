use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-native-layout-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(source: &str, fixture: &str, assertions: &str, expected: Result<String, ()>) {
    let directory = Directory::new();
    let generated = directory.0.join("generated.c");
    let foreign = directory.0.join("fixture.c");
    let executable = directory.0.join("program");
    let program = rill::parser::parse(source).unwrap();
    let c = rill::backend::emit_reference(&program).unwrap();
    let typed = rill::backend::emit(&program).unwrap();
    assert!(
        c.contains("rl_value"),
        "fixture must exercise native values"
    );
    fs::write(&foreign, format!("#include <stdint.h>\n{fixture}")).unwrap();
    for c in [format!("{c}\n{assertions}"), typed] {
        fs::write(&generated, c).unwrap();
        for optimization in ["-O0", "-O2", "-O3"] {
            let build = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", optimization])
                .arg(&generated)
                .arg(&foreign)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{optimization}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            match &expected {
                Ok(value) => {
                    assert!(result.status.success(), "{optimization}");
                    assert_eq!(result.stdout, value.as_bytes(), "{optimization}");
                    assert!(result.stderr.is_empty(), "{optimization}");
                }
                Err(()) => {
                    assert!(!result.status.success(), "{optimization}");
                    assert!(result.stdout.is_empty(), "{optimization}");
                    assert_eq!(result.stderr, b"rill: runtime trap\n", "{optimization}");
                }
            }
        }
    }
}

#[test]
fn native_payload_layout_reduces_copy_size_and_preserves_byte_values() {
    let mut bytes = [0u8; 256];
    let mut checksum = 0i32;
    for n in 0..1000 {
        bytes[n % 256] = (n % 251) as u8;
        checksum += i32::from(bytes[(n + 43) % 256]);
    }
    run(
        "fn main() { let marker = u32(0); let mut bytes = bytes_new(256); let mut n = 0; let mut answer = 0; while n < 1000 { bytes <- bytes_set(bytes, n % 256, n % 251); answer <- answer + bytes_get(bytes, (n + 43) % 256); n <- n + 1; }; answer }",
        "",
        r#"typedef struct {
    int tag; int32_t value; size_t size; const unsigned char *text; size_t len;
    uint32_t data; uint64_t wide; float real; const void *pointer;
} prior_node;
typedef struct {
    int tag; int32_t value; size_t size; const unsigned char *text; size_t len;
    uint64_t wide; float real; const void *pointer; prior_node nodes[RL_CAP];
} prior_value;
_Static_assert(sizeof(rl_node) < sizeof(prior_node), "native node copy size must shrink");
_Static_assert(sizeof(rl_value) < sizeof(prior_value), "native value copy size must shrink");
"#,
        Ok(format!("{checksum}\n")),
    );
}

#[test]
fn nested_native_payloads_preserve_strings_widths_float_bits_and_pointers() {
    run(
        r#"
extern fn fixture_word() -> u32;
extern fn fixture_wide() -> u64;
extern fn fixture_real() -> f32;
extern fn fixture_address() -> ptr<void>;
extern fn fixture_check_word(value: u32) -> bool;
extern fn fixture_check_wide(value: u64) -> bool;
extern fn fixture_check_real(value: f32) -> bool;
extern fn fixture_check_address(value: ptr<void>) -> bool;
type Payload<a> = { text: str; small: u8; word: u32; wide: u64; real: f32; address: ptr<void>; buffer: a }
type Packet<a> = Packed(a)
fn main() {
    let payload = record Payload {
        text = "hé"; small = u8(251); word = fixture_word(); wide = fixture_wide();
        real = fixture_real(); address = fixture_address();
        buffer = bytes_set(bytes_new(256), 255, 42)
    };
    let packet = Packed(payload);
    let copied = match packet { Packed(value) => value };
    if byte_len(copied.text) == 3 && byte_at(copied.text, 1) == 195
        && copied.small == u8(251) && fixture_check_word(copied.word)
        && fixture_check_wide(copied.wide) && fixture_check_real(copied.real)
        && fixture_check_address(copied.address) {
        bytes_get(copied.buffer, 255)
    } else { -1 }
}
"#,
        r#"#include <stdint.h>
#include <stdbool.h>
#include <string.h>
static unsigned char storage;
uint32_t fixture_word(void) { return UINT32_MAX; }
uint64_t fixture_wide(void) { return UINT64_MAX; }
float fixture_real(void) { return -0.0f; }
void *fixture_address(void) { return &storage; }
bool fixture_check_word(uint32_t value) { return value == UINT32_MAX; }
bool fixture_check_wide(uint64_t value) { return value == UINT64_MAX; }
bool fixture_check_real(float value) { float expected = -0.0f; return memcmp(&value, &expected, sizeof(value)) == 0; }
bool fixture_check_address(void *value) { return value == &storage; }
"#,
        "",
        Ok("42\n".into()),
    );
}

#[test]
fn native_arithmetic_and_byte_bounds_still_trap_at_all_optimization_levels() {
    for expression in ["2147483647 + 1", "1 / 0", "bytes_get(bytes_new(256), 256)"] {
        run(
            &format!("fn main() {{ let marker = u32(0); {expression} }}"),
            "",
            "",
            Err(()),
        );
    }
}
