#![cfg(any(target_os = "macos", target_os = "linux"))]

#[path = "../examples/reload-renderer/adapter.rs"]
mod adapter;
#[path = "../examples/reload-renderer/app.rs"]
mod app;

use adapter::{LoadedRenderer, TemporaryDirectory};
use app::{Frame, Options};
use rill::abi::copy::Value;
use rill::live_reload::State;
use rill::native_reload::Optimization;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-live-renderer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const FIRST: &str = include_str!("../examples/reload-renderer/logic.rill");
const SPEED: &str = include_str!("../examples/reload-renderer/logic-speed.rill");

fn options(tree: &Path, with_renderer: bool, optimization: Optimization) -> Options {
    Options {
        source: tree.join("logic.rill"),
        checkpoint: tree.join("state.json"),
        renderer: with_renderer.then(|| PathBuf::from("protocol-fixture")),
        headless: !with_renderer,
        frames: 9,
        interval: Duration::from_millis(1),
        optimization,
    }
}

fn protocol_fixture(tree: &Path) -> PathBuf {
    let source = tree.join("renderer.c");
    let log = tree.join("renderer.log");
    fs::write(
        &source,
        format!(
            r#"
#include "renderer.h"
#include <stdio.h>
static int opened;
__attribute__((destructor)) static void renderer_unloaded(void) {{
    FILE *file = fopen({:?}, "a");
    if (file) {{ fputs("unload\n", file); fclose(file); }}
}}
int32_t rill_renderer_v1(uint32_t op, const int32_t *args, uint32_t length) {{
    FILE *file = fopen({:?}, "a");
    if (!file) return -1;
    int32_t result = 0;
    if (op == RILL_RENDERER_OPEN && length == 0 && !opened) {{
        opened = 1; fputs("open\n", file);
    }} else if (op == RILL_RENDERER_FRAME && length == 2 && args && opened) {{
        fprintf(file, "frame:%d:%d\n", args[0], args[1]);
    }} else if (op == RILL_RENDERER_CLOSE && length == 0 && opened) {{
        opened = 0; fputs("close\n", file);
    }} else result = -1;
    fclose(file);
    return result;
}}
"#,
            log.to_str().unwrap(),
            log.to_str().unwrap()
        ),
    )
    .unwrap();
    let library = tree.join(if cfg!(target_os = "macos") {
        "fixture.dylib"
    } else {
        "fixture.so"
    });
    let output = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fPIC"])
        .arg(if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        })
        .arg("-I")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/reload-renderer/native"))
        .arg(source)
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    library
}

fn field(state: &State, name: &str) -> i32 {
    state.value().field(name).and_then(Value::as_i32).unwrap()
}

#[test]
fn persistent_renderer_protocol_keeps_window_state_and_unselected_color() {
    for optimization in [Optimization::O0, Optimization::O3] {
        let tree = Tree::new();
        fs::write(tree.0.join("logic.rill"), FIRST).unwrap();
        let library = protocol_fixture(&tree.0);
        let mut renderer = LoadedRenderer::open(&library, None).unwrap();
        let mut frames: Vec<Frame> = Vec::new();
        app::run(
            options(&tree.0, true, optimization),
            Some(&mut renderer),
            |frame| {
                frames.push(frame.clone());
                match frames.len() {
                    2 => fs::write(
                        tree.0.join("logic.rill"),
                        FIRST
                            .replace("state.x + 1", "state.x + 3")
                            .replace("{ 70 }", "{ 199 }"),
                    )
                    .unwrap(),
                    4 => fs::write(
                        tree.0.join("logic.rill"),
                        "// state-version: 1\nfn update( {",
                    )
                    .unwrap(),
                    5 => fs::write(tree.0.join("logic.rill"), SPEED).unwrap(),
                    7 => fs::write(tree.0.join("logic.rill"), FIRST).unwrap(),
                    _ => (),
                }
            },
        )
        .unwrap();
        assert_eq!(frames.len(), 9);
        assert_eq!((frames[1].ticks, frames[1].x), (2, 2));
        assert_eq!((frames[2].ticks, frames[2].x, frames[2].color), (3, 5, 70));
        assert_eq!(
            (
                frames[2].generation,
                frames[2].update_generation,
                frames[2].color_generation
            ),
            (1, 1, 0)
        );
        assert_eq!(
            (frames[4].ticks, frames[4].x, frames[4].generation),
            (5, 11, 1)
        );
        assert_eq!(
            (
                frames[5].ticks,
                frames[5].x,
                frames[5].color,
                frames[5].state_version
            ),
            (6, 13, 210, 2)
        );
        assert_eq!((frames[5].generation, frames[5].color_generation), (2, 2));
        assert_eq!(
            (
                frames[8].ticks,
                frames[8].x,
                frames[8].color,
                frames[8].generation
            ),
            (9, 19, 210, 2)
        );
        drop(renderer);
        let events = fs::read_to_string(tree.0.join("renderer.log")).unwrap();
        assert_eq!(events.lines().filter(|line| *line == "open").count(), 1);
        assert_eq!(events.lines().filter(|line| *line == "close").count(), 1);
        assert_eq!(
            events
                .lines()
                .filter(|line| line.starts_with("frame:"))
                .count(),
            9
        );
        assert_eq!(events.lines().next(), Some("open"));
        assert_eq!(events.lines().rev().nth(1), Some("close"));
        assert_eq!(events.lines().last(), Some("unload"));
        let state =
            State::from_checkpoint_json(&fs::read(tree.0.join("state.json")).unwrap()).unwrap();
        assert_eq!(state.version(), 2);
        assert_eq!(
            (
                field(&state, "ticks"),
                field(&state, "x"),
                field(&state, "speed")
            ),
            (9, 19, 2)
        );
    }
}

#[test]
fn renderer_build_directory_is_cleaned_when_compiler_cannot_start() {
    let tree = Tree::new();
    let path = tree.0.join("renderer-build");
    fs::create_dir(&path).unwrap();
    let compile = || -> std::io::Result<()> {
        let directory = TemporaryDirectory(path.clone());
        Command::new(directory.0.join("missing-compiler")).output()?;
        Ok(())
    };
    assert_eq!(compile().unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert!(!path.exists());
}

#[test]
fn checkpoint_survives_host_process_relaunch() {
    const CHILD: &str = "RILL_LIVE_RENDERER_CHECKPOINT_CHILD";
    if let Some(tree) = std::env::var_os(CHILD) {
        let tree = PathBuf::from(tree);
        let mut options = options(&tree, false, Optimization::O3);
        options.frames = 2;
        app::run(options, None, |_| ()).unwrap();
        return;
    }
    let tree = Tree::new();
    fs::write(tree.0.join("logic.rill"), FIRST).unwrap();
    for expected in [2, 4] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "checkpoint_survives_host_process_relaunch",
                "--nocapture",
            ])
            .env(CHILD, &tree.0)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let state =
            State::from_checkpoint_json(&fs::read(tree.0.join("state.json")).unwrap()).unwrap();
        assert_eq!(
            (field(&state, "ticks"), field(&state, "x")),
            (expected, expected)
        );
    }
}

#[test]
fn headless_mode_is_logic_only_and_corrupt_checkpoint_rejects() {
    let tree = Tree::new();
    fs::write(tree.0.join("logic.rill"), FIRST).unwrap();
    let mut run = options(&tree.0, false, Optimization::O0);
    run.frames = 1;
    app::run(run, None, |frame| {
        assert_eq!((frame.ticks, frame.x), (1, 1))
    })
    .unwrap();
    fs::write(tree.0.join("state.json"), b"not a checkpoint").unwrap();
    assert!(app::run(options(&tree.0, false, Optimization::O0), None, |_| ()).is_err());
    assert_eq!(
        fs::read(tree.0.join("state.json")).unwrap(),
        b"not a checkpoint"
    );
}

#[test]
fn real_raylib_window_runs_without_restarting_for_logic_replacement() {
    let required = std::env::var_os("RILL_REQUIRE_GPU").is_some_and(|value| value == "1");
    let Some(executable) = std::env::var_os("RILL_RELOAD_RENDERER_EXE") else {
        assert!(!required, "required real renderer test needs RILL_RELOAD_RENDERER_EXE pointing to the built standalone example");
        eprintln!("Skipping real reload-renderer GPU test: RILL_RELOAD_RENDERER_EXE is unset");
        return;
    };
    let available = rill::c_import::Config {
        packages: vec!["rill-raylib".into()],
        ..Default::default()
    }
    .resolve();
    if let Err(error) = available {
        assert!(
            !required,
            "required real Raylib provider unavailable: {error}"
        );
        eprintln!("Skipping real reload-renderer GPU test: {error}");
        return;
    }
    let tree = Tree::new();
    fs::write(tree.0.join("logic.rill"), FIRST).unwrap();
    let capture = tree.0.join("frame.png");
    let mut child = Command::new(executable)
        .arg("--source")
        .arg(tree.0.join("logic.rill"))
        .arg("--checkpoint")
        .arg(tree.0.join("state.json"))
        .args(["--frames", "20", "--interval-ms", "25", "--opt-level", "3"])
        .env("RILL_RAYLIB_HIDDEN", "1")
        .env("RILL_RELOAD_CAPTURE", &capture)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut changed = false;
    loop {
        if !changed {
            if let Ok(bytes) = fs::read(tree.0.join("state.json")) {
                let state = State::from_checkpoint_json(&bytes).unwrap();
                if field(&state, "ticks") >= 2 {
                    fs::write(
                        tree.0.join("logic.rill"),
                        FIRST.replace("state.x + 1", "state.x + 3"),
                    )
                    .unwrap();
                    changed = true;
                }
            }
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("real reload-renderer timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success()
        && stderr.contains("renderer could not open a real window/device")
        && !required
    {
        eprintln!("Skipping real reload-renderer GPU test: {stderr}");
        return;
    }
    assert!(output.status.success(), "{stderr}");
    assert!(changed);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("generation=1 update=1 color-generation=0"),
        "{stdout}"
    );
    let state = State::from_checkpoint_json(&fs::read(tree.0.join("state.json")).unwrap()).unwrap();
    assert_eq!(field(&state, "ticks"), 20);
    assert!(field(&state, "x") > 20);
    let png = fs::read(capture).unwrap();
    assert!(png.len() > 24 && &png[..8] == b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 640);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 360);
}
