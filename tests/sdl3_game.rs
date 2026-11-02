use std::{fs, process::Command};

#[test]
fn collector_gameplay_runs_without_sdl() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("rill-sdl-game-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let executable = dir.join("collector");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(root.join("examples/sdl3/collector.rill"))
        .arg("-o")
        .arg(&executable)
        .arg("--link-arg")
        .arg(root.join("tests/fixtures/sdl3_stub.c"))
        .output()
        .expect("build requires a C compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0\n");
    fs::remove_dir_all(dir).unwrap();
}
