use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct CaptureDirectory(PathBuf);

impl CaptureDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-voxel-gpu-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for CaptureDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn required() -> bool {
    std::env::var_os("RILL_REQUIRE_GPU").is_some_and(|value| value != "0")
}

fn sdl_available() -> bool {
    let available = Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status()
        .is_ok_and(|status| status.success());
    assert!(
        available || !required(),
        "RILL_REQUIRE_GPU: SDL3 development files unavailable"
    );
    if !available {
        eprintln!("SKIP voxel GPU: pkg-config and SDL3 development files required");
    }
    available
}

fn launch(arguments: &[&str], capture: Option<&Path>) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new("bash");
    command
        .arg(root.join("examples/sdl3-voxel/gpu/run.sh"))
        .args(arguments)
        .env("RILL", env!("CARGO_BIN_EXE_rill"));
    // Hardware tests use the host video backend, never SDL's dummy renderer.
    command.env_remove("SDL_VIDEODRIVER");
    if let Some(path) = capture {
        command.arg("--capture").arg(path);
    }
    command
        .output()
        .expect("GPU launcher requires bash and Python 3")
}

fn diagnostics(output: &Output) -> String {
    format!(
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn preflight() -> bool {
    if !sdl_available() {
        return false;
    }
    let output = launch(&["--self-test", "--opt", "0"], None);
    if output.status.code() == Some(77) && !required() {
        eprintln!(
            "SKIP voxel GPU: native GPU unavailable\n{}",
            diagnostics(&output)
        );
        return false;
    }
    assert!(
        output.status.success(),
        "GPU self-test failed: {}",
        diagnostics(&output)
    );
    true
}

fn check_metrics(
    output: &Output,
    world: u32,
    width: u32,
    height: u32,
    frames: u32,
) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let metrics = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value.get("backend").is_some())
        .expect("GPU smoke must report completed-frame metrics");
    assert_eq!(metrics["backend"], "metal");
    assert_eq!(metrics["world"], world);
    assert_eq!(metrics["voxels"], u64::from(world).pow(3));
    assert_eq!(metrics["frames"], frames);
    assert_eq!(metrics["render_width"], width);
    assert_eq!(metrics["render_height"], height);
    let duration = metrics["frame_ms"]
        .as_f64()
        .expect("frame_ms must be numeric");
    assert!(duration.is_finite() && duration > 0.0);
    for key in [
        "support_error",
        "capacity_overflow",
        "trace_budget",
        "player_stuck",
    ] {
        assert_eq!(metrics[key], 0, "GPU {key} must remain zero");
    }
    assert_eq!(metrics["structures"], 9);
    for key in ["removed", "structure_voxels"] {
        assert!(
            metrics[key].as_u64().is_some_and(|value| value > 0),
            "GPU smoke must exercise {key}"
        );
    }
    metrics
}

fn pixels(path: &Path, width: u32, height: u32) -> Vec<u8> {
    let bmp = fs::read(path).expect("GPU capture must be written");
    assert!(bmp.len() >= 54, "truncated BMP header");
    assert_eq!(&bmp[..2], b"BM");
    let word = |offset| u32::from_le_bytes(bmp[offset..offset + 4].try_into().unwrap());
    assert_eq!(word(18), width);
    assert_eq!((word(22) as i32).unsigned_abs(), height);
    let bits = u16::from_le_bytes(bmp[28..30].try_into().unwrap());
    assert!(matches!(bits, 24 | 32), "unexpected BMP bit depth {bits}");
    let bytes_per_pixel = usize::from(bits / 8);
    let stride = (width as usize * bytes_per_pixel).div_ceil(4) * 4;
    let offset = word(10) as usize;
    assert!(
        offset + stride * height as usize <= bmp.len(),
        "truncated BMP pixels"
    );
    let mut image = Vec::with_capacity(width as usize * height as usize * 3);
    let mut colors = BTreeSet::new();
    for row in bmp[offset..offset + stride * height as usize].chunks(stride) {
        for x in 0..width as usize {
            let pixel = &row[x * bytes_per_pixel..x * bytes_per_pixel + 3];
            colors.insert([pixel[0], pixel[1], pixel[2]]);
            image.extend_from_slice(pixel);
        }
    }
    assert!(
        colors.len() > 16,
        "GPU image is blank or lacks scene detail: {} colors",
        colors.len()
    );
    image
}

#[test]
fn real_gpu_readbacks_and_rendering_agree_across_c_optimizations() {
    if !preflight() {
        return;
    }
    // Once a real device has passed preflight, no later GPU error is a skip.
    let output = launch(&["--self-test", "--opt", "2"], None);
    assert!(
        output.status.success(),
        "optimized GPU self-test failed: {}",
        diagnostics(&output)
    );
    let output = launch(&["--self-test"], None);
    assert!(
        output.status.success(),
        "default optimized GPU self-test failed: {}",
        diagnostics(&output)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Native game build: -O3"),
        "the default launcher must exercise the optimized native build"
    );
    let directory = CaptureDirectory::new();
    let mut reference = None;
    for opt in ["0", "2"] {
        let capture = directory.0.join(format!("frame-{opt}.bmp"));
        let output = launch(
            &[
                "--smoke", "--world", "64", "--width", "320", "--height", "180", "--opt", opt,
            ],
            Some(&capture),
        );
        assert!(
            output.status.success(),
            "GPU smoke -O{opt} failed: {}",
            diagnostics(&output)
        );
        let metrics = check_metrics(&output, 64, 320, 180, 12);
        assert!(
            metrics["unsupported_mask"]
                .as_u64()
                .is_some_and(|mask| mask > 0),
            "scripted demolition must fully sever a structure"
        );
        let image = pixels(&capture, 320, 180);
        if let Some(expected) = &reference {
            assert!(
                &image == expected,
                "GPU captures differ between -O0 and -O2"
            );
        } else {
            reference = Some(image);
        }
    }
}

#[test]
fn optional_256_cubed_gpu_world_renders() {
    if std::env::var_os("RILL_TEST_GPU_256").is_none() {
        eprintln!("SKIP 256-cubed GPU capacity test: set RILL_TEST_GPU_256=1");
        return;
    }
    if !preflight() {
        return;
    }
    let directory = CaptureDirectory::new();
    let capture = directory.0.join("large.bmp");
    let output = launch(
        &[
            "--smoke", "--world", "256", "--width", "160", "--height", "90",
        ],
        Some(&capture),
    );
    assert!(
        output.status.success(),
        "256-cubed GPU smoke failed: {}",
        diagnostics(&output)
    );
    let metrics = check_metrics(&output, 256, 160, 90, 12);
    assert!(
        metrics["unsupported_mask"]
            .as_u64()
            .is_some_and(|mask| mask > 0),
        "scripted demolition must fully sever a structure"
    );
    pixels(&capture, 160, 90);
}

#[test]
fn damaged_building_leans_during_first_person_gpu_execution() {
    if !preflight() {
        return;
    }
    let directory = CaptureDirectory::new();
    let capture = directory.0.join("lean-walk.bmp");
    let output = launch(
        &[
            "--benchmark",
            "90",
            "--lean",
            "--walk",
            "--world",
            "128",
            "--width",
            "320",
            "--height",
            "180",
        ],
        Some(&capture),
    );
    assert!(
        output.status.success(),
        "leaning first-person GPU run failed: {}",
        diagnostics(&output)
    );
    let metrics = check_metrics(&output, 128, 320, 180, 90);
    assert!(
        metrics["tilted_mask"].as_u64().is_some_and(|mask| mask > 0),
        "asymmetric foundation destruction must visibly tilt a building"
    );
    pixels(&capture, 320, 180);
}

#[test]
fn real_sdl_window_preserves_cursor_and_focus_input_contract() {
    if !preflight() {
        return;
    }
    let output = launch(
        &[
            "--input-test",
            "--world",
            "64",
            "--width",
            "320",
            "--height",
            "180",
        ],
        None,
    );
    assert!(
        output.status.success(),
        "native SDL input test failed: {}",
        diagnostics(&output)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("native cursor capture/release and queued focus input pass"),
        "native SDL input assertions must execute"
    );
}
