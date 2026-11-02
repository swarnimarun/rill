use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rill-neon-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn explicitly_requested_neon_rejects_unsupported_target() {
    for source in ["let main = 1", "let main = u8x16_load(bytes_new(16), 0)"] {
        let c = rill::compile(source).unwrap();
        let dir = temp_dir();
        let path = dir.join("program.c");
        fs::write(&path, c).unwrap();
        let result = Command::new("cc")
            .args([
                "-std=c11",
                "-DRILL_SIMD_NEON=1",
                "-U__ARM_NEON",
                "-fsyntax-only",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr)
            .contains("RILL_SIMD_NEON requires an AArch64 target with NEON"));
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(target_arch = "aarch64")]
#[test]
fn explicitly_selected_neon_executes_and_emits_vector_xor() {
    // Dynamic C input prevents constant folding of the vector operations.
    let prefix = "extern fn fixture_input(i: i32) -> i32;\nlet main = let mut b = bytes_new(32) in let mut i = 0 in let init = while i < 32 do let changed = b <- bytes_set(b, i, fixture_input(i)) in i <- i + 1 in let a = u8x16_load(b, 0) in let c = u8x16_load(b, 16) in ";
    let dir = temp_dir();
    let cfile = dir.join("program.c");
    let fixture = dir.join("fixture.c");
    let exe = dir.join("program");
    let asm = dir.join("program.s");
    fs::write(
        &fixture,
        "#include <stdint.h>\nint32_t fixture_input(int32_t i) { return (i * 7 + 13) & 255; }\n",
    )
    .unwrap();
    for op in ["and", "or", "xor", "eq"] {
        let source = format!("{prefix}u8x16_{op}(a, c)");
        fs::write(&cfile, rill::compile(&source).unwrap()).unwrap();
        let lanes: Vec<String> = (0..16)
            .map(|i| {
                let a = (i * 7 + 13) & 255;
                let b = ((i + 16) * 7 + 13) & 255;
                match op {
                    "and" => a & b,
                    "or" => a | b,
                    "xor" => a ^ b,
                    "eq" => {
                        if a == b {
                            255
                        } else {
                            0
                        }
                    }
                    _ => unreachable!(),
                }
                .to_string()
            })
            .collect();
        let expected = format!("u8x16[{}]\n", lanes.join(", "));
        for mode in [false, true] {
            for opt in ["-O0", "-O2"] {
                let mut build = Command::new("cc");
                build.args(["-std=c11", opt]);
                if mode {
                    build.arg("-DRILL_SIMD_NEON=1");
                }
                let output = build
                    .arg(&cfile)
                    .arg(&fixture)
                    .arg("-o")
                    .arg(&exe)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let output = Command::new(&exe).output().unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout, expected.as_bytes(), "{op} {opt} neon={mode}");
            }
        }
        if op == "xor" {
            let output = Command::new("cc")
                .args(["-std=c11", "-O2", "-DRILL_SIMD_NEON=1", "-S"])
                .arg(&cfile)
                .arg("-o")
                .arg(&asm)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let assembly = fs::read_to_string(&asm).unwrap();
            assert!(
                assembly
                    .lines()
                    .any(|line| line.contains("eor") && line.contains(".16b")),
                "expected vector XOR in generated assembly"
            );
        }
    }
    for source in [
        "let main = u8x16_load(bytes_new(15), 0)",
        "let main = u8x16_load(bytes_new(16), -1)",
        "let main = u8x16_lane(u8x16_load(bytes_new(16), 0), 16)",
    ] {
        fs::write(&cfile, rill::compile(source).unwrap()).unwrap();
        let output = Command::new("cc")
            .args(["-std=c11", "-O2", "-DRILL_SIMD_NEON=1"])
            .arg(&cfile)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&exe).output().unwrap();
        assert!(!output.status.success());
        assert_eq!(output.stderr, b"rill: runtime trap\n");
    }
    fs::remove_dir_all(dir).unwrap();
}
