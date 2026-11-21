use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn scene_transforms_capacity_and_playable_game_agree_across_builds() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scene-engine");
    let dir = std::env::temp_dir().join(format!("rill-scene-engine-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for module in ["scene.rill", "game.rill"] {
        fs::copy(example.join(module), dir.join(module)).unwrap();
    }
    let source = r#"
import scene;
import std.option as opt;
fn id(value: opt.Option<i32>) { match value { opt.Some(n) => n | opt.None => -1 } }
fn main() {
    let mut s = scene.empty();
    let bad = id(scene.spawn(inout s, scene.Child(0), 0, 0, 1, 1, 65));
    let a = id(scene.spawn(inout s, scene.Root, 2, 3, 2, 2, 65));
    let b = id(scene.spawn(inout s, scene.Child(a), 1, 2, 1, 1, 66));
    let c = id(scene.spawn(inout s, scene.Child(b), 4, 5, 1, 1, 67));
    let moved = scene.position(inout s, a, 8, 9);
    let refused = !scene.position(inout s, a, 256, 0);
    let transformed = match scene.sprite(s, c) { opt.Some(p) => p.x == 13 && p.y == 16 | opt.None => false };
    let mut n = 3;
    while n < 16 {
        scene.spawn(inout s, scene.Root, 0, 0, 1, 1, 68);
        n <- n + 1;
    };
    let full = id(scene.spawn(inout s, scene.Root, 0, 0, 1, 1, 69)) == -1 && scene.count(s) == 16;
    let edge = record scene.Sprite { x = 2147483647; y = 0; width = 1; height = 1; glyph = 65 };
    let adjacent = record scene.Sprite { x = 2147483646; y = 0; width = 1; height = 1; glyph = 66 };
    bad == -1 && a == 0 && b == 1 && c == 2 && moved && refused && transformed && full &&
        scene.overlaps(edge, edge) && !scene.overlaps(adjacent, edge)
}"#;
    let entry = dir.join("probe.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    let cli = env!("CARGO_BIN_EXE_rill");
    for opt in ["0", "3"] {
        let flags = ["--opt-level", opt, "--c-arg", "-Werror=parentheses"];
        assert_eq!(
            success(
                Command::new(cli)
                    .arg("run")
                    .arg(&entry)
                    .args(flags)
                    .output()
                    .unwrap()
            ),
            b"true\n"
        );
        let binary = dir.join(format!("game{}", std::env::consts::EXE_SUFFIX));
        success(
            Command::new(cli)
                .arg("build")
                .arg(example.join("main.rill"))
                .args(flags)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap(),
        );
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"lllq").unwrap();
        let frames = String::from_utf8(success(child.wait_with_output().unwrap())).unwrap();
        let lines: Vec<_> = frames.lines().collect();
        assert_eq!(lines.len(), 33);
        assert_eq!(lines[3], ".@..*...............");
        assert_eq!(lines[27], "....@...............");
        assert_eq!(lines[28], "....o...............");
        assert_eq!(lines[30], "...........*........");
        assert_eq!(lines[32], "1");
    }
    fs::remove_dir_all(dir).unwrap();
}
