use std::{fs, process::Command};

#[test]
fn source_to_ir_then_eval_or_emit_llvm_through_the_cli() {
    let rill = env!("CARGO_BIN_EXE_rill");
    let file = std::env::temp_dir().join(format!("rill-kernel-cli-{}.rir", std::process::id()));
    let lower = Command::new(rill)
        .args(["kernel", "lower", "examples/gpu-ir/clamp.rill", "clamp"])
        .output()
        .unwrap();
    assert!(
        lower.status.success(),
        "{}",
        String::from_utf8_lossy(&lower.stderr)
    );
    fs::write(&file, lower.stdout).unwrap();
    let evaluated = Command::new(rill)
        .args(["kernel", "eval"])
        .arg(&file)
        .args(["-2147483648", "0", "2147483647"])
        .output()
        .unwrap();
    assert!(evaluated.status.success());
    assert_eq!(evaluated.stdout, b"0\n0\n2147483647\n");
    for (target, triple) in [
        ("nvptx", "nvptx64-nvidia-cuda"),
        ("vulkan", "spirv1.5-unknown-vulkan1.2-compute"),
    ] {
        let emitted = Command::new(rill)
            .args(["kernel", "emit-llvm", target])
            .arg(&file)
            .output()
            .unwrap();
        assert!(emitted.status.success());
        assert!(String::from_utf8(emitted.stdout).unwrap().contains(triple));
    }
    fs::write(&file, b"rir 9\n").unwrap();
    let rejected = Command::new(rill)
        .args(["kernel", "emit-llvm", "nvptx"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8(rejected.stderr)
        .unwrap()
        .contains("line 1: expected 'rir 0'"));
    fs::remove_file(file).unwrap();
}
