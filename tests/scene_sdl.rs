use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

#[path = "support/native_launch.rs"]
mod native_launch;

// Wrap only the native boundaries to inject queued input, observe real
// software pixels before presentation, and observe SDL's state after close.
const PROBE: &str = r#"
#define scene_window_open real_open
#define scene_window_present real_present
#define scene_window_close real_close
#include "scene_window.c"
#undef scene_window_open
#undef scene_window_present
#undef scene_window_close
#include <stdlib.h>
#include <string.h>

static unsigned seen;
static bool started;
static bool fail_present;
static void require(bool ok) {
    if (!ok) { fprintf(stderr, "scene probe failed: %s\n", SDL_GetError()); abort(); }
}
bool scene_window_open(void) {
    if (!real_open()) return false;
    started = true;
    fail_present = strcmp(getenv("RILL_SCENE_TEST_MODE"), "present-error") == 0;
    require(strcmp(SDL_GetRendererName(renderer), "software") == 0);
    SDL_Event key = {0};
    key.type = SDL_EVENT_KEY_DOWN;
    key.key.scancode = SDL_SCANCODE_RIGHT;
    for (int n = 0; n < 3; ++n) require(SDL_PushEvent(&key));
    SDL_Event quit = {0};
    quit.type = SDL_EVENT_QUIT;
    require(SDL_PushEvent(&quit));
    return true;
}
static int pixel(SDL_Surface *image, int x, int y) {
    Uint8 r, g, b, a;
    require(SDL_ReadSurfacePixel(image, x * 32 + 16, y * 32 + 16, &r, &g, &b, &a));
    return (r << 16) | (g << 8) | b;
}
bool scene_window_present(int32_t score) {
    SDL_Surface *image = SDL_RenderReadPixels(renderer, NULL);
    require(image != NULL);
    int player = 1;
    while (player <= 4 && pixel(image, player, 3) != 0x84cae8) ++player;
    require(player <= 4);
    require(pixel(image, player, 4) == 0x64ac73);
    require(pixel(image, 0, 0) == 0x161c22);
    require(score == (player == 4));
    require(pixel(image, score ? 11 : 4, score ? 6 : 3) == 0xfac348);
    seen |= 1u << (player - 1);
    SDL_DestroySurface(image);
    return !fail_present && real_present(score);
}
void scene_window_close(void) {
    real_close();
    require(SDL_WasInit(0) == 0 && window == NULL && renderer == NULL);
    if (started) require(seen == (fail_present ? 1u : 15u));
    fputs("clean close\n", stderr);
}
"#;

#[test]
fn real_sdl_software_frames_follow_queued_movement_and_quit() {
    if !Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("SKIP scene SDL3: pkg-config and SDL3 development files required");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scene-engine");
    let dir = std::env::temp_dir().join(format!("rill-scene-sdl-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let probe = dir.join("probe.c");
    fs::write(&probe, PROBE).unwrap();
    let binary = dir.join(format!("game{}", std::env::consts::EXE_SUFFIX));
    for opt in ["0", "3"] {
        let build = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(root.join("window.rill"))
            .args(["--opt-level", opt])
            .args(["--c-arg", "-I", "--c-arg"])
            .arg(&root)
            .args(["--link-arg"])
            .arg(&probe)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        // Exercise failure before and after acquiring the window and renderer.
        for (driver, mode, expected) in [
            ("dummy", "play", "1\n"),
            ("dummy", "present-error", "-1\n"),
            ("rill-missing-video-driver", "init-error", "-1\n"),
        ] {
            let stdout = dir.join("score");
            let stderr = dir.join("log");
            let status = native_launch::status(
                Command::new(&binary)
                    .env("RILL_SCENE_TEST_MODE", mode)
                    .env("SDL_VIDEO_DRIVER", driver)
                    .env("SDL_VIDEODRIVER", driver)
                    .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
                    .stderr(Stdio::from(fs::File::create(&stderr).unwrap())),
                &dir,
                Duration::from_secs(10),
            )
            .unwrap();
            let log = fs::read_to_string(stderr).unwrap();
            assert!(status.success(), "{log}");
            assert_eq!(fs::read_to_string(stdout).unwrap(), expected, "{log}");
            assert!(log.contains("clean close"), "{log}");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
