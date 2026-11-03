#[cfg(all(feature = "c-import", unix))]
mod import_worker;

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
    #[cfg(all(feature = "c-import", unix))]
    if command == OsStr::new(import_worker::COMMAND) {
        if args.next().is_some() {
            return Err("E-C-IMPORT: worker does not accept command-line arguments".into());
        }
        return import_worker::run();
    }
    if command == OsStr::new("fmt") {
        return format_source(args);
    }
    if command == OsStr::new("import-c") {
        return import_c(args);
    }
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

fn format_source(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let path = args.next().ok_or_else(usage)?;
    let check = match args.next().as_deref() {
        None => false,
        Some(option) if option == OsStr::new("--check") => true,
        _ => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("E-FMT-IO: cannot read {}: {error}", path.to_string_lossy()))?;
    let formatted = rill::formatter::format(&source)?;
    if check {
        if source == formatted {
            Ok(())
        } else {
            Err(format!(
                "E-FMT-CHECK: {} needs formatting",
                path.to_string_lossy()
            ))
        }
    } else {
        std::io::stdout()
            .lock()
            .write_all(formatted.as_bytes())
            .map_err(|error| format!("E-FMT-IO: cannot write formatted source: {error}"))
    }
}

fn import_c(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let mut selection = rill::c_import::selected::Selection::default();
    let mut output = None;
    let mut compiler = None;
    let mut zig_cc = false;
    let mut timeout_ms = None;
    while let Some(option) = args.next() {
        let value = |args: &mut dyn Iterator<Item = OsString>| {
            args.next()
                .ok_or_else(|| format!("{} requires a value", option.to_string_lossy()))
        };
        let text = |args: &mut dyn Iterator<Item = OsString>| {
            value(args)?
                .into_string()
                .map_err(|_| format!("{} requires UTF-8", option.to_string_lossy()))
        };
        match option.to_str() {
            Some("--timeout-ms") if timeout_ms.is_none() => {
                let timeout = text(&mut args)?
                    .parse::<u64>()
                    .map_err(|_| "E-C-IMPORT: --timeout-ms requires an integer from 1 to 600000")?;
                if !(1..=600_000).contains(&timeout) {
                    return Err(
                        "E-C-IMPORT: --timeout-ms requires an integer from 1 to 600000".into(),
                    );
                }
                timeout_ms = Some(timeout);
            }
            Some("--header") => selection.native.includes.push(text(&mut args)?),
            Some("--function") => selection.functions.push(text(&mut args)?),
            Some("--constant") => selection.constants.push(text(&mut args)?),
            Some("--link") => selection.native.links.push(text(&mut args)?),
            Some("--pkg-config") => selection.native.packages.push(text(&mut args)?),
            Some("--c-arg") => selection.c_args.push(value(&mut args)?),
            Some("--clang-resource-dir") if selection.resource_dir.is_none() => {
                selection.resource_dir = Some(value(&mut args)?.into())
            }
            Some("-o") if output.is_none() => output = Some(value(&mut args)?),
            Some("--cc") if compiler.is_none() && !zig_cc => compiler = Some(value(&mut args)?),
            Some("--zig-cc") if compiler.is_none() && !zig_cc => {
                compiler = Some("zig".into());
                zig_cc = true;
            }
            _ => {
                return Err(format!(
                    "invalid or repeated import-c option {option:?}\n{}",
                    usage()
                ))
            }
        }
    }
    let compiler = compiler.unwrap_or_else(|| env::var_os("CC").unwrap_or_else(|| "cc".into()));
    let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));
    #[cfg(all(feature = "c-import", unix))]
    let module = import_worker::generate(&selection, &compiler, zig_cc, timeout)?;
    #[cfg(not(all(feature = "c-import", unix)))]
    let module = {
        let _ = timeout;
        #[cfg(feature = "c-import")]
        {
            let _ = (&selection, &compiler, zig_cc);
            Err::<String, String>("E-C-IMPORT: isolated import-c is currently supported only on Unix; the library generate API remains in-process".into())?
        }
        #[cfg(not(feature = "c-import"))]
        rill::c_import::selected::generate(&selection, &compiler, zig_cc)?
    };
    if let Some(path) = output {
        rill::c_import::selected::write_atomic(Path::new(&path), &module)
    } else {
        std::io::stdout()
            .lock()
            .write_all(module.as_bytes())
            .map_err(|e| format!("E-C-IMPORT: cannot write bindings: {e}"))
    }
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
    "usage: rill fmt <source.rill> [--check]\n       rill <check|emit-c> <source.rill> [--cc <compiler> | --zig-cc] [--c-arg <arg>]...\n       rill run <source.rill> [--native] [compiler options]\n       rill build <source.rill> -o <executable> [--cc <compiler> | --zig-cc] [--simd scalar|neon] [--c-arg <arg>]... [--link-arg <arg>]...\n       rill import-c --header <header> [--function <name>]... [--constant <name>]... [-o <bindings.rill>] [--pkg-config <package>]... [--link <library>]... [--cc <compiler> | --zig-cc] [--c-arg <arg>]... [--clang-resource-dir <path>] [--timeout-ms <1..600000>]"
        .into()
}
