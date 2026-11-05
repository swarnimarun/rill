use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill engine {} {}",
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

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn camera_and_matrix_math_agree_with_geometric_invariants() {
    let tree = Tree::new();
    for (name, source) in [
        ("math", include_str!("../examples/engine-3d/src/math.rill")),
        (
            "scene",
            include_str!("../examples/engine-3d/src/scene.rill"),
        ),
    ] {
        fs::write(tree.0.join(format!("{name}.rill")), source).unwrap();
    }
    fs::write(
        tree.0.join("main.rill"),
        r#"
import math;
import scene;
fn close(actual: f32, expected: f32) {
    let error = f32_sub(actual, expected);
    let tolerance = f32_div(f32(1), f32(10000));
    f32_lt(f32_neg(tolerance), error) && f32_lt(error, tolerance)
}
fn check_model() {
    let point = math.v4(f32(1), f32(2), f32(3), f32(1));
    let same = math.transform(math.identity(), point);
    let rotated = math.model(math.v3(10, 20, 30), f32(2), f32(1), f32(0));
    let result = math.transform(rotated, point);
    let a = math.model(math.v3(4, 0, 0), f32(1), f32(0), f32(1));
    let b = math.model(math.v3(0, 0, 0), f32(1), f32(1), f32(0));
    let composed = math.transform(math.multiply(b, a), point);
    close(same.x, f32(1)) && close(same.y, f32(2)) && close(same.z, f32(3))
        && close(result.x, f32(16)) && close(result.y, f32(24)) && close(result.z, f32(28))
        && close(composed.x, f32(3)) && close(composed.z, f32(-5))
}
fn check_camera() {
    let camera = scene.look(scene.camera(1), 90000, 100000);
    let moved = scene.move_camera(camera, f32(2), f32(3), f32(4), f32(1), f32(0));
    let view = math.view(math.v3(2, 3, 4), f32(1), f32(0), f32(0), f32(1));
    let at_eye = math.transform(view, math.v4(f32(2), f32(3), f32(4), f32(1)));
    let in_front = math.transform(view, math.v4(f32(5), f32(3), f32(4), f32(1)));
    camera.yaw == 90000 && camera.pitch == 89000
        && close(moved.position.x, f32(2)) && close(moved.position.y, f32(4))
        && close(moved.position.z, f32(-10))
        && close(at_eye.x, f32(0)) && close(at_eye.y, f32(0)) && close(at_eye.z, f32(0))
        && close(in_front.x, f32(0)) && close(in_front.z, f32(3))
}
fn check_projection() {
    let projection = math.perspective(200, 100);
    let near = math.transform(projection, math.v4(f32(0), f32(0), f32_div(f32(1), f32(10)), f32(1)));
    let far = math.transform(projection, math.v4(f32(0), f32(0), f32(100), f32(1)));
    let side = math.transform(projection, math.v4(f32(2), f32(1), f32(2), f32(1)));
    close(f32_div(near.z, near.w), f32(0)) && close(f32_div(far.z, far.w), f32(1))
        && close(f32_div(side.x, side.w), f32(1)) && close(f32_div(side.y, side.w), f32(1))
}
fn main() { check_model() && check_camera() && check_projection() }
"#,
    )
    .unwrap();
    let program = rill::modules::compile::load_program(&tree.0.join("main.rill")).unwrap();
    for result in [
        rill::interpreter::run_reference(&program),
        rill::interpreter::run(&program),
    ] {
        assert_eq!(result.unwrap().to_string(), "true");
    }
    for (label, generated) in [
        (
            "reference",
            rill::backend::emit_reference(&program).unwrap(),
        ),
        ("selected", rill::backend::emit(&program).unwrap()),
    ] {
        let c = tree.0.join(format!("{label}.c"));
        let executable = tree.0.join(label);
        fs::write(&c, generated).unwrap();
        for opt in ["-O0", "-O3"] {
            success(
                &Command::new("cc")
                    .args(["-std=c11", "-pedantic-errors", opt])
                    .arg(&c)
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            let output = Command::new(&executable).output().unwrap();
            success(&output);
            assert_eq!(output.stdout, b"true\n", "{label} {opt}");
        }
    }
}

fn copy_sources(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "build" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_sources(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn available(command: &str, arguments: &[&str]) -> bool {
    Command::new(command)
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn native_image_bounds_reject_overflow_before_gpu_access() {
    if !available("pkg-config", &["--exists", "sdl3"]) {
        assert!(std::env::var_os("RILL_REQUIRE_GPU").is_none_or(|value| value != "1"));
        eprintln!("Skipping native renderer bounds: SDL3 development files are unavailable");
        return;
    }
    let tree = Tree::new();
    let native = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/engine-3d/native");
    let harness = tree.0.join("bounds.c");
    fs::write(
        &harness,
        r#"
#include "renderer.c"
#include <assert.h>
int main(void) {
    size_t bytes = 0;
    assert(pixel_bytes(160, 120, &bytes) && bytes == 76800);
    assert(pixel_bytes(3840, 2160, &bytes) && bytes == 33177600);
    assert(pixel_bytes(1, UINT32_MAX / 4, &bytes) && bytes == UINT32_MAX - 3);
    assert(!pixel_bytes(0, 120, &bytes));
    assert(!pixel_bytes(160, 0, &bytes));
    assert(!pixel_bytes(1, UINT32_MAX / 4 + 1, &bytes));
    assert(!pixel_bytes(65536, 65536, &bytes));
    assert(!pixel_bytes(UINT32_MAX, UINT32_MAX, &bytes));
    // Invalid targets and readbacks must fail before dereferencing a GPU device.
    Engine engine = { .width = 65536, .height = 65536, .frames = 1 };
    assert(!targets(&engine, engine.width, engine.height));
    assert(!read_pixels(&engine));
    assert(e3_pixel(&engine, 0, 0, 0) == -1);
    assert(!e3_open(INT32_MIN, 480));
    assert(!e3_open(0, 480));
    assert(e3_status() == 1);
    return 0;
}
"#,
    )
    .unwrap();
    let dependencies = rill::c_import::Config {
        packages: vec!["sdl3".into()],
        links: vec!["m".into()],
        ..Default::default()
    }
    .resolve()
    .unwrap();
    let executable = tree.0.join("bounds");
    success(
        &Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
            .args(dependencies.cflags)
            .arg("-I")
            .arg(native)
            .arg(harness)
            .args(dependencies.link_args)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    success(
        &Command::new(executable)
            .env_remove("RILL_ENGINE_WIDTH")
            .env_remove("RILL_ENGINE_HEIGHT")
            .output()
            .unwrap(),
    );
}

fn engine_run(executable: &Path, capture: &Path, mode: u32, frames: u32) -> Output {
    Command::new(executable)
        .env("RILL_ENGINE_HEADLESS", "1")
        .env("RILL_ENGINE_WIDTH", "160")
        .env("RILL_ENGINE_HEIGHT", "120")
        .env("RILL_ENGINE_FRAMES", frames.to_string())
        .env("RILL_ENGINE_MODE", mode.to_string())
        .env("RILL_ENGINE_CAPTURE", capture)
        .env_remove("RILL_ENGINE_SHADERS")
        .output()
        .unwrap()
}

fn ppm(path: &Path) -> Vec<u8> {
    let data = fs::read(path).unwrap();
    let header = b"P6\n160 120\n255\n";
    assert!(data.starts_with(header));
    assert_eq!(data.len(), header.len() + 160 * 120 * 3);
    data[header.len()..].to_vec()
}

fn assert_rendered(output: &Output, metrics: &str) {
    success(output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().last(), Some("0"), "{output:?}");
    assert!(stdout.contains(metrics), "{output:?}");
}

#[test]
fn gpu_scene_verifies_depth_camera_animation_and_failure_paths() {
    let required = std::env::var_os("RILL_REQUIRE_GPU").is_some_and(|value| value == "1");
    for (tool, arguments) in [
        ("pkg-config", vec!["--exists", "sdl3"]),
        ("slangc", vec!["-version"]),
        ("spirv-val", vec!["--version"]),
    ] {
        if !available(tool, &arguments) {
            assert!(!required, "RILL_REQUIRE_GPU=1 requires {tool}");
            eprintln!("Skipping real GPU scene: {tool} is unavailable");
            return;
        }
    }
    let tree = Tree::new();
    let project = tree.0.join("3d scene");
    copy_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/engine-3d"),
        &project,
    );
    success(
        &Command::new(env!("CARGO_BIN_EXE_rill"))
            .args(["project", "build"])
            .arg(&project)
            .output()
            .unwrap(),
    );
    assert!(project.join("build/shaders/manifest.json").is_file());
    for name in [
        "mesh_vertex_msl.metal",
        "mesh_fragment_msl.metal",
        "mesh_vertex_spv.spv",
        "mesh_fragment_spv.spv",
    ] {
        assert!(project.join("build/shaders").join(name).is_file());
    }
    let executable = project.join(format!("build/engine-3d{}", std::env::consts::EXE_SUFFIX));
    let depth_path = tree.0.join("depth.ppm");
    let depth = engine_run(&executable, &depth_path, 1, 1);
    success(&depth);
    if String::from_utf8_lossy(&depth.stdout).lines().last() == Some("77") {
        assert!(!required, "RILL_REQUIRE_GPU=1: {depth:?}");
        assert!(String::from_utf8_lossy(&depth.stderr).contains("Engine GPU unavailable"));
        assert!(!depth_path.exists());
        eprintln!(
            "Skipping real GPU scene: {}",
            String::from_utf8_lossy(&depth.stderr)
        );
        return;
    }
    assert_rendered(&depth, "ENGINE frames=1 draws=2 vertices=12");
    let depth_pixels = ppm(&depth_path);
    let center = (60 * 160 + 80) * 3;
    assert_eq!(&depth_pixels[center..center + 3], &[255, 0, 0]);

    let scene_path = tree.0.join("scene.ppm");
    assert_rendered(
        &engine_run(&executable, &scene_path, 0, 1),
        "ENGINE frames=1 draws=3 vertices=78",
    );
    let pixels = ppm(&scene_path);
    let orange = pixels
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| p[0] > 80 && u16::from(p[0]) > u16::from(p[1]) * 2)
        .count();
    let cyan = pixels
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| p[1] > 40 && p[2] > 60 && p[0] < 40)
        .count();
    assert!(
        orange > 50 && cyan > 30,
        "missing mesh/material coverage: orange={orange}, cyan={cyan}"
    );

    let moved_path = tree.0.join("moved.ppm");
    assert_rendered(
        &engine_run(&executable, &moved_path, 2, 1),
        "ENGINE frames=1 draws=3 vertices=78",
    );
    let moved = ppm(&moved_path);
    assert!(pixels.iter().zip(&moved).filter(|(a, b)| a != b).count() > 1000);
    let animated_path = tree.0.join("animated.ppm");
    assert_rendered(
        &engine_run(&executable, &animated_path, 0, 20),
        "ENGINE frames=20 draws=60 vertices=1560",
    );
    let animated = ppm(&animated_path);
    assert!(pixels.iter().zip(&animated).filter(|(a, b)| a != b).count() > 300);

    // A shader failure after device creation must fail, never become a GPU skip.
    for artifact in ["mesh_vertex_msl.metal", "mesh_vertex_spv.spv"] {
        fs::write(
            project.join("build/shaders").join(artifact),
            b"invalid shader",
        )
        .unwrap();
    }
    let failure_path = tree.0.join("failure.ppm");
    let failed = engine_run(&executable, &failure_path, 0, 1);
    success(&failed);
    assert_eq!(
        String::from_utf8_lossy(&failed.stdout).lines().last(),
        Some("1")
    );
    assert!(!failure_path.exists());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Engine vertex shader"));
}
