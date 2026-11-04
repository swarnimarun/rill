use rill::{backend, interpreter, modules::compile::load_program};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Core(PathBuf);

impl Core {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rill-sdl3-voxel-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sdl3-voxel");
        for name in ["world.rill", "physics.rill", "lighting.rill"] {
            fs::copy(example.join(name), dir.join(name)).unwrap();
        }
        Self(dir)
    }

    fn compare(&self, source: &str) {
        let entry = self.0.join("main.rill");
        fs::write(&entry, source).unwrap();
        let program = load_program(&entry).unwrap();
        assert_eq!(interpreter::run(&program).unwrap().to_string(), "true");
        let cfile = self.0.join("output.c");
        let binary = self.0.join("output");
        fs::write(&cfile, backend::emit(&program).unwrap()).unwrap();
        for opt in ["-O0", "-O2"] {
            let build = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&cfile)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{opt}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(&binary).output().unwrap();
            assert!(
                run.status.success(),
                "{opt}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(run.stdout, b"true\n", "{opt}");
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn storage_round_trips_every_cell_and_clips_all_six_boundaries() {
    Core::new().compare(
        r#"
import world;
fn main() {
    let mut cells = world.empty();
    let mut i = 0;
    while i < 512 {
        world.set(inout cells, i % 8, (i / 8) % 8, i / 64, i % 5);
        i <- i + 1;
    };
    world.set(inout cells, -1, 0, 0, 4);
    world.set(inout cells, 8, 0, 0, 4);
    world.set(inout cells, 0, -1, 0, 4);
    world.set(inout cells, 0, 8, 0, 4);
    world.set(inout cells, 0, 0, -1, 4);
    world.set(inout cells, 0, 0, 8, 4);
    let mut valid = true;
    i <- 0;
    while i < 512 {
        valid <- valid && world.get(cells, i % 8, (i / 8) % 8, i / 64) == i % 5;
        i <- i + 1;
    };
    valid && world.inside(0, 0, 0) && world.inside(7, 7, 7)
        && not world.inside(-1, 0, 0) && not world.inside(0, 8, 0)
        && not world.inside(0, 0, 8)
        && world.get(cells, -1, 0, 0) == 0 && world.get(cells, 8, 0, 0) == 0
        && world.get(cells, 0, -1, 0) == 0 && world.get(cells, 0, 8, 0) == 0
        && world.get(cells, 0, 0, -1) == 0 && world.get(cells, 0, 0, 8) == 0
}
"#,
    );
}

#[test]
fn spherical_blast_preserves_bedrock_and_clips_at_world_edge() {
    Core::new().compare(
        r#"
import world;
import physics;
fn main() {
    let mut cells = world.empty();
    let mut i = 0;
    while i < 512 {
        world.set(inout cells, i % 8, (i / 8) % 8, i / 64, 2);
        i <- i + 1;
    };
    world.set(inout cells, 3, 3, 3, 1);
    let removed = physics.blast(inout cells, 3, 3, 3, 1);
    let corner = physics.blast(inout cells, 0, 0, 0, 1);
    let negative = physics.blast(inout cells, 3, 3, 3, -1);
    let far = physics.blast(inout cells, 2147483647, 2147483647, 2147483647, 3);
    removed == 6 && corner == 4 && negative == 0 && far == 0
        && world.get(cells, 3, 3, 3) == 1
        && world.get(cells, 4, 3, 3) == 0
        && world.get(cells, 4, 4, 3) == 2
        && world.get(cells, 5, 3, 3) == 2
        && physics.blast(inout cells, 3, 3, 3, 0) == 0
}
"#,
    );
}

#[test]
fn support_propagates_sideways_and_collapse_conserves_material() {
    Core::new().compare(
        r#"
import world;
import physics;
fn main() {
    let mut cells = world.empty();
    let mut support = world.empty();
    world.set(inout cells, 1, 0, 1, 1);
    world.set(inout cells, 1, 1, 1, 2);
    world.set(inout cells, 1, 2, 1, 2);
    world.set(inout cells, 2, 2, 1, 3);
    world.set(inout cells, 3, 2, 1, 4);
    let stable = physics.step(inout cells, inout support);
    let supported = world.get(support, 3, 2, 1) != 0;
    let removed = physics.blast(inout cells, 1, 1, 1, 0);
    let moved = physics.step(inout cells, inout support);
    let one_step = world.get(cells, 1, 1, 1) == 2
        && world.get(cells, 2, 1, 1) == 3
        && world.get(cells, 3, 1, 1) == 4
        && world.get(cells, 3, 2, 1) == 0;
    let mut tick = 0;
    while tick < 8 {
        physics.step(inout cells, inout support);
        tick <- tick + 1;
    };
    let mut count = 0;
    let mut material = 0;
    let mut i = 0;
    while i < 512 {
        let value = world.get(cells, i % 8, (i / 8) % 8, i / 64);
        if value != 0 { count <- count + 1; };
        material <- material + value;
        i <- i + 1;
    };
    stable == 0 && supported && removed == 1 && moved == 3 && one_step
        && count == 4 && material == 10
        && world.get(cells, 1, 0, 1) == 1
        && world.get(cells, 2, 1, 1) == 3
        && world.get(cells, 3, 1, 1) == 4
        && physics.step(inout cells, inout support) == 0
}
"#,
    );
}

#[test]
fn signed_distance_selects_nearest_surface_and_rebuilds_after_edits() {
    Core::new().compare(
        r#"
import world;
import lighting;
fn main() {
    let mut cells = world.empty();
    let mut sdf = world.empty();
    let mut light = world.empty();
    world.set(inout cells, 3, 3, 3, 2);
    world.set(inout cells, 7, 7, 7, 4);
    lighting.rebuild(cells, inout sdf, inout light);
    let first = lighting.distance(sdf, 3, 3, 3) == -1
        && lighting.distance(sdf, 4, 3, 3) == 1
        && lighting.distance(sdf, 0, 0, 0) == 9
        && lighting.distance(sdf, 7, 7, 6) == 1;
    world.set(inout cells, 3, 3, 3, 0);
    world.set(inout cells, 4, 3, 3, 2);
    lighting.rebuild(cells, inout sdf, inout light);
    first && lighting.distance(sdf, 3, 3, 3) == 1
        && lighting.distance(sdf, 4, 3, 3) == -1
        && lighting.distance(sdf, 0, 0, 0) == 10
}
"#,
    );
}

#[test]
fn enclosed_air_responds_to_emitter_and_opening_the_roof() {
    Core::new().compare(
        r#"
import world;
import lighting;
fn main() {
    let mut cells = world.empty();
    let mut sdf = world.empty();
    let mut light = world.empty();
    let mut x = 2;
    while x <= 4 {
        let mut y = 2;
        while y <= 4 {
            let mut z = 2;
            while z <= 4 {
                if x == 2 || x == 4 || y == 2 || y == 4 || z == 2 || z == 4 {
                    world.set(inout cells, x, y, z, 2);
                };
                z <- z + 1;
            };
            y <- y + 1;
        };
        x <- x + 1;
    };
    world.set(inout cells, 4, 3, 3, 4);
    lighting.rebuild(cells, inout sdf, inout light);
    let emitted = lighting.sample(light, 3, 3, 3);
    let source = lighting.sample(light, 4, 3, 3);
    world.set(inout cells, 4, 3, 3, 2);
    lighting.rebuild(cells, inout sdf, inout light);
    let enclosed = lighting.sample(light, 3, 3, 3);
    world.set(inout cells, 3, 4, 3, 0);
    lighting.rebuild(cells, inout sdf, inout light);
    let open = lighting.sample(light, 3, 3, 3);
    let mut bounded = true;
    let mut i = 0;
    while i < 512 {
        let value = lighting.sample(light, i % 8, (i / 8) % 8, i / 64);
        bounded <- bounded && value >= 0 && value <= 255;
        i <- i + 1;
    };
    source == 255 && emitted > enclosed && open > enclosed && bounded
}
"#,
    );
}

#[test]
fn sandbox_runs_against_real_sdl3_with_dummy_video() {
    if !Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("SKIP voxel SDL3 smoke: pkg-config and SDL3 development files required");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = Core::new();
    let mut reference = None;
    for opt in ["0", "2"] {
        let capture = temporary.0.join(format!("frame-{opt}.bmp"));
        let output = Command::new("bash")
            .arg(root.join("examples/sdl3-voxel/run.sh"))
            .arg("--cpu")
            .args(["--smoke", "--opt", opt])
            .env("SDL_VIDEODRIVER", "dummy")
            .env("RILL", env!("CARGO_BIN_EXE_rill"))
            .env("RILL_VOXEL_CAPTURE", &capture)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "voxel SDL3 smoke -O{opt} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"0\n");
        let frame = fs::read(capture).unwrap();
        assert_eq!(&frame[..2], b"BM");
        let offset = u32::from_le_bytes(frame[10..14].try_into().unwrap()) as usize;
        let pixels = &frame[offset..];
        assert!(pixels.len() > 100_000);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != &pixels[..4]));
        if let Some(expected) = &reference {
            assert!(
                &frame == expected,
                "SDL framebuffer differs between -O0 and -O2"
            );
        } else {
            reference = Some(frame);
        }
    }
}

#[test]
fn solid_distance_accounts_for_exterior_air_and_new_interior_cavity() {
    Core::new().compare(
        r#"
import world;
import lighting;
fn main() {
    let mut cells = world.empty();
    let mut sdf = world.empty();
    let mut light = world.empty();
    let mut i = 0;
    while i < 512 {
        world.set(inout cells, i % 8, (i / 8) % 8, i / 64, 2);
        i <- i + 1;
    };
    lighting.rebuild(cells, inout sdf, inout light);
    let exterior = lighting.distance(sdf, 3, 3, 3) == -4
        && lighting.distance(sdf, 0, 3, 3) == -1
        && lighting.distance(sdf, 7, 7, 7) == -1;
    world.set(inout cells, 3, 3, 3, 0);
    lighting.rebuild(cells, inout sdf, inout light);
    exterior && lighting.distance(sdf, 3, 3, 3) == 1
        && lighting.distance(sdf, 4, 3, 3) == -1
        && lighting.distance(sdf, 5, 3, 3) == -2
}
"#,
    );
}

#[test]
fn unsupported_stack_falls_one_cell_per_tick_without_overwrite() {
    Core::new().compare(
        r#"
import world;
import physics;
fn main() {
    let mut cells = world.empty();
    let mut support = world.empty();
    world.set(inout cells, 6, 5, 6, 2);
    world.set(inout cells, 6, 6, 6, 4);
    let moved = physics.step(inout cells, inout support);
    let once = moved == 2 && world.get(cells, 6, 4, 6) == 2
        && world.get(cells, 6, 5, 6) == 4 && world.get(cells, 6, 6, 6) == 0;
    let mut tick = 0;
    while tick < 8 {
        physics.step(inout cells, inout support);
        tick <- tick + 1;
    };
    let mut total = 0;
    let mut occupied = 0;
    let mut i = 0;
    while i < 512 {
        let value = world.get(cells, i % 8, (i / 8) % 8, i / 64);
        total <- total + value;
        if value != 0 { occupied <- occupied + 1; };
        i <- i + 1;
    };
    once && total == 6 && occupied == 2 && world.get(cells, 6, 0, 6) == 2
        && world.get(cells, 6, 1, 6) == 4
        && physics.step(inout cells, inout support) == 0
}
"#,
    );
}

#[test]
fn distance_guided_ray_cannot_skip_a_thin_occluder() {
    Core::new().compare(
        r#"
import world;
import lighting;
fn main() {
    let mut cells = world.empty();
    let mut sdf = world.empty();
    let mut light = world.empty();
    let direct = world.empty();
    lighting.rebuild(cells, inout sdf, inout light);
    let empty = lighting.distance(sdf, 3, 3, 3) == 24
        && lighting.distance(sdf, 0, 0, 0) == 24;
    world.set(inout cells, 7, 3, 3, 4);
    world.set(inout cells, 4, 3, 3, 2);
    lighting.rebuild(cells, inout sdf, inout light);
    let blocked = lighting.ray(cells, sdf, direct, 0, 3, 3, 1, 0, 0);
    world.set(inout cells, 4, 3, 3, 0);
    lighting.rebuild(cells, inout sdf, inout light);
    let visible = lighting.ray(cells, sdf, direct, 0, 3, 3, 1, 0, 0);
    empty && blocked == 0 && visible > 0
}
"#,
    );
}

#[test]
fn sdl_adapter_retains_clicks_released_between_frames() {
    if !Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("SKIP voxel SDL3 events: pkg-config and SDL3 development files required");
        return;
    }
    let temporary = Core::new();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sdl3-voxel");
    let driver = temporary.0.join("events.c");
    fs::write(
        &driver,
        r#"
#include "platform.h"
#include <SDL3/SDL.h>
#include <assert.h>
int main(void) {
    assert(vx_open());
    assert(vx_poll());
    SDL_Event event = {0};
    event.type = SDL_EVENT_MOUSE_BUTTON_DOWN;
    event.button.button = SDL_BUTTON_LEFT;
    event.button.x = 321;
    event.button.y = 234;
    assert(SDL_PushEvent(&event));
    event.type = SDL_EVENT_MOUSE_BUTTON_UP;
    assert(SDL_PushEvent(&event));
    assert(vx_poll());
    assert(vx_buttons() == 1);
    assert(vx_mouse_x() == 321 && vx_mouse_y() == 234);
    event.type = SDL_EVENT_MOUSE_BUTTON_DOWN;
    event.button.x = 345;
    assert(SDL_PushEvent(&event));
    event.type = SDL_EVENT_MOUSE_BUTTON_UP;
    assert(SDL_PushEvent(&event));
    assert(vx_poll());
    assert(vx_buttons() == 1 && vx_mouse_x() == 345);
    assert(vx_poll());
    assert(vx_buttons() == 0);
    event = (SDL_Event){0};
    event.type = SDL_EVENT_KEY_DOWN;
    event.key.scancode = SDL_SCANCODE_SPACE;
    assert(SDL_PushEvent(&event));
    assert(vx_poll());
    assert(vx_key(2) == 1);
    assert(vx_poll());
    assert(vx_key(2) == 0);
    event.type = SDL_EVENT_QUIT;
    assert(SDL_PushEvent(&event));
    assert(!vx_poll());
    vx_close();
    return 0;
}
"#,
    )
    .unwrap();
    let flags = Command::new("python3")
        .args([
            "-c",
            "import json,shlex,subprocess; print(json.dumps(shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','sdl3'],text=True))))",
        ])
        .output()
        .unwrap();
    assert!(
        flags.status.success(),
        "{}",
        String::from_utf8_lossy(&flags.stderr)
    );
    let flags: Vec<String> = serde_json::from_slice(&flags.stdout).unwrap();
    let executable = temporary.0.join("events");
    let build = Command::new("cc")
        .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg("-I")
        .arg(&example)
        .arg(&driver)
        .arg(example.join("platform.c"))
        .args(flags)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(executable)
        .env("SDL_VIDEODRIVER", "dummy")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
