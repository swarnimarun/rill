//! A persistent host window with selected pure Rill function replacement.
//! See examples/reload-renderer/README.md. Headless mode runs logic only.

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "reload-renderer/adapter.rs"]
mod adapter;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "reload-renderer/app.rs"]
mod app;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod host {
    use super::{
        adapter::{LoadedRenderer, TemporaryDirectory},
        app::{self, Options},
    };
    use std::fs;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn build_renderer() -> Result<(TemporaryDirectory, PathBuf), String> {
        let package = rill::c_import::Config {
            packages: vec!["rill-raylib".into()],
            ..Default::default()
        }
        .resolve()?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rill-reload-renderer-{}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
        let directory = TemporaryDirectory(directory);
        let path = directory.0.join(if cfg!(target_os = "macos") {
            "renderer.dylib"
        } else {
            "renderer.so"
        });
        let native = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/reload-renderer/native");
        let output = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", "-fPIC"])
            .arg(if cfg!(target_os = "macos") {
                "-dynamiclib"
            } else {
                "-shared"
            })
            .args(package.cflags)
            .arg(native.join("renderer.c"))
            .args(package.link_args)
            .arg("-o")
            .arg(&path)
            .output()
            .map_err(|e| format!("compile renderer: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "compile renderer: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok((directory, path))
    }

    fn arguments() -> Result<Options, String> {
        let mut options = Options::default();
        let mut arguments = std::env::args_os().skip(1);
        while let Some(argument) = arguments.next() {
            let mut value = || arguments.next().ok_or("option requires a value");
            match argument.to_str() {
                Some("--source") => options.source = value()?.into(),
                Some("--checkpoint") => options.checkpoint = value()?.into(),
                Some("--renderer") => options.renderer = Some(value()?.into()),
                Some("--headless") => options.headless = true,
                Some("--frames") => {
                    options.frames = value()?
                        .to_str()
                        .and_then(|v| v.parse().ok())
                        .filter(|v| *v > 0)
                        .ok_or("--frames requires a positive u32")?
                }
                Some("--interval-ms") => {
                    options.interval = std::time::Duration::from_millis(
                        value()?
                            .to_str()
                            .and_then(|v| v.parse().ok())
                            .filter(|v| (1..=60000).contains(v))
                            .ok_or("--interval-ms requires 1..60000")?,
                    )
                }
                Some("--opt-level") => {
                    options.optimization = match value()?.to_str() {
                        Some("0") => rill::native_reload::Optimization::O0,
                        Some("3") => rill::native_reload::Optimization::O3,
                        _ => return Err("--opt-level requires 0 or 3".into()),
                    }
                }
                _ => return Err(format!("unknown option {argument:?}")),
            }
        }
        Ok(options)
    }

    pub fn run() -> Result<(), String> {
        let options = arguments()?;
        if options.headless {
            eprintln!("logic-only headless run: no window or GPU is created");
            app::run(options, None, print_frame)
        } else {
            let (directory, path) = if let Some(path) = options.renderer.as_ref() {
                (None, path.clone())
            } else {
                let (directory, path) = build_renderer()?;
                (Some(directory), path)
            };
            let mut renderer = LoadedRenderer::open(&path, directory)?;
            app::run(options, Some(&mut renderer), print_frame)
        }
    }

    fn print_frame(frame: &app::Frame) {
        println!(
            "ticks={} x={} color={} state={} generation={} update={} color-generation={}",
            frame.ticks,
            frame.x,
            frame.color,
            frame.state_version,
            frame.generation,
            frame.update_generation,
            frame.color_generation
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn main() -> Result<(), String> {
    host::run()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn main() -> Result<(), String> {
    Err("native reload renderer supports macOS and Linux".into())
}
