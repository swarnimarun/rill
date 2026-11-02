use std::{fs, path::PathBuf, process::Command};

#[test]
fn click_challenge_counts_hits_and_stops_at_timeout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("rill-raylib-game-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = fs::read_to_string(root.join("examples/raylib-click/main.rill")).unwrap();
    let c_file = dir.join("game.c");
    fs::write(&c_file, rill::compile(&source).unwrap()).unwrap();
    let executable = dir.join("game");
    let output = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra"])
        .arg("-I")
        .arg(root.join("tests/fixtures/raylib_mock"))
        .arg(&c_file)
        .arg(root.join("examples/raylib-click/shim.c"))
        .arg(root.join("tests/fixtures/raylib_mock/raylib.c"))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("cc is required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = Command::new(&executable).output().unwrap();
    assert!(result.status.success());
    assert_eq!(
        result.stdout,
        b"Score: 1   Time: 29\nScore: 1   Time: 28\nScore: 2   Time: 27\nScore: 2   Time: 0\n()\n"
    );
    fs::remove_dir_all(dir).unwrap();
}
