use std::{
    env,
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let command = args.next().ok_or_else(usage)?;
    let path = args.next().ok_or_else(usage)?;
    if !matches!(command.to_str(), Some("check" | "run" | "emit-c" | "build")) {
        return Err(usage());
    }
    let options = BuildOptions::parse(command.to_str().unwrap(), args)?;
    let program =
        rill::modules::compile::load_program(Path::new(&path)).map_err(|e| e.to_string())?;
    let native_source = !program.native.includes.is_empty()
        || !program.native.links.is_empty()
        || !program.native.packages.is_empty();
    let resolved = program.native.resolve()?;
    let compiler = options
        .compiler
        .clone()
        .unwrap_or_else(|| env::var_os("CC").unwrap_or_else(|| "cc".into()));
    let mut cflags = resolved.cflags;
    cflags.extend(options.c_args.iter().cloned());
    if options.simd == Simd::Neon {
        cflags.push("-DRILL_SIMD_NEON=1".into());
    }
    let mut link_args = resolved.link_args;
    link_args.extend(options.link_args.iter().cloned());
    match command.to_str() {
        Some("check") => {
            if native_source {
                rill::c_import::validate_c(
                    &rill::backend::emit(&program)?,
                    &compiler,
                    options.zig_cc,
                    &cflags,
                )?;
            }
        }
        Some("run") if !(native_source || options.native) => {
            println!("{}", rill::interpreter::run(&program)?);
        }
        Some("emit-c") => {
            let c = rill::backend::emit(&program)?;
            if native_source {
                rill::c_import::validate_c(&c, &compiler, options.zig_cc, &cflags)?;
            }
            print!("{c}");
        }
        Some("build") => {
            build_c(
                &rill::backend::emit(&program)?,
                &compiler,
                options.zig_cc,
                &cflags,
                &link_args,
                Path::new(options.output.as_ref().unwrap()),
            )?;
        }
        Some("run") => {
            let directory = RunDirectory::create()?;
            let binary = directory.0.join("program");
            build_c(
                &rill::backend::emit(&program)?,
                &compiler,
                options.zig_cc,
                &cflags,
                &link_args,
                &binary,
            )?;
            let status = Command::new(&binary)
                .status()
                .map_err(|e| format!("failed to run native program: {e}"))?;
            if !status.success() {
                return Err(format!("native program failed with {status}"));
            }
        }
        _ => return Err(usage()),
    }
    Ok(())
}

fn build_c(
    c: &str,
    compiler: &OsStr,
    zig_cc: bool,
    cflags: &[OsString],
    link_args: &[OsString],
    output: &Path,
) -> Result<(), String> {
    let mut command = Command::new(compiler);
    if zig_cc {
        command.arg("cc");
    }
    command.args(cflags);
    command.args(["-std=c11", "-x", "c", "-", "-o"]).arg(output);
    if !link_args.is_empty() {
        command.args(["-x", "none"]).args(link_args);
    }
    let mut child = command.stdin(Stdio::piped()).spawn().map_err(|e| {
        format!(
            "failed to start C compiler {}: {e}",
            compiler.to_string_lossy()
        )
    })?;
    let written = child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(c.as_bytes());
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for C compiler: {e}"))?;
    if !status.success() {
        return Err(format!("C compiler failed with {status}"));
    }
    written.map_err(|e| format!("failed to write C source: {e}"))
}

struct RunDirectory(PathBuf);
impl RunDirectory {
    fn create() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = env::temp_dir().join(format!(
                "rill-native-run-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("failed to create native run directory: {e}")),
            }
        }
    }
}
impl Drop for RunDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct BuildOptions {
    output: Option<OsString>,
    compiler: Option<OsString>,
    zig_cc: bool,
    link_args: Vec<OsString>,
    c_args: Vec<OsString>,
    native: bool,
    simd: Simd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Simd {
    Scalar,
    Neon,
}

impl BuildOptions {
    fn parse(command: &str, mut args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut output = None;
        let mut compiler = None;
        let mut zig_cc = false;
        let mut link_args = Vec::new();
        let mut c_args = Vec::new();
        let mut native = false;
        let mut simd = None;
        while let Some(arg) = args.next() {
            match arg.as_os_str() {
                arg if arg == OsStr::new("-o") => {
                    if command != "build" {
                        return Err("-o is only supported by build".into());
                    }
                    if output.is_some() {
                        return Err("-o may be specified only once".into());
                    }
                    output = Some(args.next().ok_or("-o requires a path")?);
                }
                arg if arg == OsStr::new("--cc") => {
                    if compiler.is_some() || zig_cc {
                        return Err("--cc and --zig-cc cannot be combined or repeated".into());
                    }
                    compiler = Some(args.next().ok_or("--cc requires an executable")?);
                    native = true;
                }
                arg if arg == OsStr::new("--zig-cc") => {
                    if compiler.is_some() || zig_cc {
                        return Err("--cc and --zig-cc cannot be combined or repeated".into());
                    }
                    zig_cc = true;
                    compiler = Some("zig".into());
                    native = true;
                }
                arg if arg == OsStr::new("--link-arg") => {
                    if !matches!(command, "build" | "run") {
                        return Err("--link-arg requires build or run".into());
                    }
                    link_args.push(args.next().ok_or("--link-arg requires one argument")?);
                    native = true;
                }
                arg if arg == OsStr::new("--c-arg") => {
                    c_args.push(args.next().ok_or("--c-arg requires one argument")?);
                    native = true;
                }
                arg if arg == OsStr::new("--native") && command == "run" => {
                    native = true;
                }
                arg if arg == OsStr::new("--simd") => {
                    if simd.is_some() {
                        return Err("--simd may be specified only once".into());
                    }
                    let mode = args.next().ok_or("--simd requires scalar or neon")?;
                    simd = Some(match mode.to_str() {
                        Some("scalar") => Simd::Scalar,
                        Some("neon") => Simd::Neon,
                        _ => return Err("--simd requires scalar or neon".into()),
                    });
                    native = true;
                }
                _ => return Err(usage()),
            }
        }
        if command == "build" && output.is_none() {
            return Err("build requires -o <executable>".into());
        }
        Ok(Self {
            output,
            compiler,
            zig_cc,
            link_args,
            c_args,
            native,
            simd: simd.unwrap_or(Simd::Scalar),
        })
    }
}

fn usage() -> String {
    "usage: rill <check|emit-c> <source.rill> [--cc <compiler> | --zig-cc] [--c-arg <arg>]...\n       rill run <source.rill> [--native] [compiler options]\n       rill build <source.rill> -o <executable> [--cc <compiler> | --zig-cc] [--simd scalar|neon] [--c-arg <arg>]... [--link-arg <arg>]..."
        .into()
}
