use std::{fs, process::Command};

#[test]
fn direct_sdl3_game_runs_against_real_library() {
    let available = Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status();
    if !matches!(available, Ok(status) if status.success()) {
        eprintln!("SKIP direct SDL3 native test: pkg-config and SDL3 development files required");
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("rill-sdl3-direct-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = fs::read_to_string(root.join("examples/sdl3-direct/main.rill")).unwrap();
    // Exercise the actual example and SDL ABI without a 15-second game round.
    let source = source
        .replace("frame < 900", "frame < 3")
        .replace("SDL_Delay(u32(16))", "SDL_Delay(u32(0))");
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    fs::copy(
        root.join("examples/sdl3-direct/sdl.rill"),
        dir.join("sdl.rill"),
    )
    .unwrap();
    let executable = dir.join("game");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(&entry)
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("native build requires a C compiler");
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(&executable)
        .env("SDL_VIDEODRIVER", "dummy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "SDL3 game failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0\n");
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("run")
        .arg(&entry)
        .env("SDL_VIDEODRIVER", "dummy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native CLI run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0\n");
    fs::remove_dir_all(dir).unwrap();
}
