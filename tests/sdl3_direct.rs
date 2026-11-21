use std::{fs, process::Command};

#[test]
fn direct_sdl3_game_runs_against_real_library() {
    run_sdl_game(false, false);
}

#[test]
fn direct_sdl3_game_runs_with_zig_against_real_library() {
    if !Command::new("zig")
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("SKIP Zig SDL3 native test: Zig required");
        return;
    }
    run_sdl_game(false, true);
}

#[cfg(all(feature = "c-import", unix))]
#[test]
fn generated_sdl3_bindings_run_against_real_library() {
    run_sdl_game(true, false);
}

fn run_sdl_game(generated: bool, zig_cc: bool) {
    let available = Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status();
    if !matches!(available, Ok(status) if status.success()) {
        eprintln!(
            "SKIP {} SDL3 native test: pkg-config and SDL3 development files required",
            if generated { "generated" } else { "direct" }
        );
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!(
        "rill-sdl3-direct-{}-{generated}-{zig_cc}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    let source = fs::read_to_string(root.join("examples/sdl3-direct/main.rill")).unwrap();
    // Exercise the actual example and SDL ABI without a 15-second game round.
    let source = source
        .replace("frame < 900", "frame < 3")
        .replace("SDL_Delay(u32(16))", "SDL_Delay(u32(0))");
    if generated {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.args([
            "import-c",
            "--header",
            "SDL3/SDL.h",
            "--pkg-config",
            "sdl3",
            "--link",
            "SDL3",
        ]);
        #[cfg(target_os = "macos")]
        if std::env::var_os("SDKROOT").is_none() {
            let sdk = Command::new("xcrun")
                .arg("--show-sdk-path")
                .output()
                .unwrap();
            assert!(
                sdk.status.success(),
                "selected SDL3 import needs the active macOS SDK"
            );
            command.env("SDKROOT", String::from_utf8(sdk.stdout).unwrap().trim());
        }
        for name in ["SDL_INIT_VIDEO", "SDL_EVENT_QUIT"] {
            command.args(["--constant", name]);
        }
        for name in [
            "SDL_Init",
            "SDL_CreateWindow",
            "SDL_CreateRenderer",
            "SDL_PumpEvents",
            "SDL_HasEvent",
            "SDL_GetMouseState",
            "SDL_SetRenderDrawColor",
            "SDL_RenderClear",
            "SDL_RenderLine",
            "SDL_RenderPresent",
            "SDL_Delay",
            "SDL_DestroyRenderer",
            "SDL_DestroyWindow",
            "SDL_Quit",
        ] {
            command.args(["--function", name]);
        }
        if let Some(resource) = std::env::var_os("RILL_TEST_CLANG_RESOURCE_DIR") {
            command.arg("--clang-resource-dir").arg(resource);
        } else if let Ok(output) =
            Command::new(std::env::var_os("CLANG_PATH").unwrap_or_else(|| "clang".into()))
                .arg("-print-resource-dir")
                .output()
        {
            if output.status.success() {
                command
                    .arg("--clang-resource-dir")
                    .arg(String::from_utf8_lossy(&output.stdout).trim());
            }
        }
        let output = command
            .arg("-o")
            .arg(dir.join("sdl.rill"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "SDL3 import failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        fs::copy(
            root.join(if cfg!(feature = "c-import") {
                "examples/sdl3-direct/sdl.rill"
            } else {
                "tests/fixtures/sdl3_manual.rill"
            }),
            dir.join("sdl.rill"),
        )
        .unwrap();
    }
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let emitted = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("emit-c")
        .arg(&entry)
        .output()
        .unwrap();
    assert!(
        emitted.status.success(),
        "SDL3 typed emission failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let c = String::from_utf8(emitted.stdout).unwrap();
    assert!(
        !c.contains("rl_value") && !c.contains("RL_CAP"),
        "SDL3 must use checked typed native transport"
    );
    let executable = dir.join("game");
    for optimization in ["-O0", "-O2"] {
        let mut build = Command::new(env!("CARGO_BIN_EXE_rill"));
        build
            .arg("build")
            .arg(&entry)
            .arg("-o")
            .arg(&executable)
            .args(["--c-arg", optimization]);
        if zig_cc {
            build.arg("--zig-cc");
            build.env("ZIG_GLOBAL_CACHE_DIR", dir.join("zig-cache"));
            build.env("ZIG_LOCAL_CACHE_DIR", dir.join("zig-local-cache"));
        }
        let output = build.output().expect("native build requires a C compiler");
        assert!(
            output.status.success(),
            "build {optimization} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&executable)
            .env("SDL_VIDEODRIVER", "dummy")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "SDL3 game {optimization} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"0\n");
    }
    let mut run = Command::new(env!("CARGO_BIN_EXE_rill"));
    run.arg("run").arg(&entry).env("SDL_VIDEODRIVER", "dummy");
    if zig_cc {
        run.arg("--zig-cc");
        run.env("ZIG_GLOBAL_CACHE_DIR", dir.join("zig-cache"));
        run.env("ZIG_LOCAL_CACHE_DIR", dir.join("zig-local-cache"));
    }
    let output = run.output().unwrap();
    assert!(
        output.status.success(),
        "native CLI run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0\n");
    fs::remove_dir_all(dir).unwrap();
}
