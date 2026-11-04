use std::{fs, process::Command};

#[test]
fn direct_game_checks_rendering_and_destroys_resources_on_every_exit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("rill-sdl3-lifecycle-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = fs::read_to_string(root.join("examples/sdl3-direct/main.rill"))
        .unwrap()
        .replace("frame < 900", "frame < 40");
    assert!(source.contains("frame < 40"));
    fs::write(dir.join("main.rill"), source).unwrap();
    let bindings = fs::read_to_string(root.join(if cfg!(feature = "c-import") {
        "examples/sdl3-direct/sdl.rill"
    } else {
        "tests/fixtures/sdl3_manual.rill"
    }))
    .unwrap()
    .replace("@link(\"SDL3\");", "")
    .replace("@pkg_config(\"sdl3\");", "");
    fs::write(dir.join("sdl.rill"), bindings).unwrap();
    let include = root.join("tests/fixtures/sdl3_mock");
    let executable = dir.join("game");
    for optimization in ["-O0", "-O2"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(dir.join("main.rill"))
            .arg("-o")
            .arg(&executable)
            .args(["--c-arg", optimization, "--c-arg", "-I", "--c-arg"])
            .arg(&include)
            .arg("--link-arg")
            .arg(include.join("sdl.c"))
            .output()
            .expect("native build requires a C compiler");
        assert!(
            output.status.success(),
            "{optimization} build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for (case, result, trace) in [
            (
                "success",
                "1\n",
                "init\nwindow\nrenderer\ndestroy-renderer\ndestroy-window\nquit\n",
            ),
            (
                "miss",
                "0\n",
                "init\nwindow\nrenderer\ndestroy-renderer\ndestroy-window\nquit\n",
            ),
            (
                "quit",
                "0\n",
                "init\nwindow\nrenderer\ndestroy-renderer\ndestroy-window\nquit\n",
            ),
            ("init", "-3\n", "init\nquit\n"),
            ("window", "-1\n", "init\nwindow\nquit\n"),
            (
                "renderer",
                "-2\n",
                "init\nwindow\nrenderer\ndestroy-window\nquit\n",
            ),
        ] {
            assert_case(&executable, optimization, case, result, trace);
        }
        for case in [
            "color-1", "color-2", "color-3", "clear", "line-1", "line-2", "line-3", "line-4",
            "present",
        ] {
            assert_case(
                &executable,
                optimization,
                case,
                "-4\n",
                "init\nwindow\nrenderer\nrender-failed\ndestroy-renderer\ndestroy-window\nquit\n",
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

fn assert_case(
    executable: &std::path::Path,
    optimization: &str,
    case: &str,
    result: &str,
    trace: &str,
) {
    let output = Command::new(executable)
        .env("RILL_SDL_TEST_CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{optimization} {case} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, result.as_bytes(), "{optimization} {case}");
    assert_eq!(output.stderr, trace.as_bytes(), "{optimization} {case}");
}
