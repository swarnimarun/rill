#[cfg(unix)]
mod config_driver;
mod docgen;
#[cfg(all(feature = "c-import", unix))]
mod import_worker;
mod native_headers;
mod project;
mod watch;

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
    if command == OsStr::new("lsp") {
        if args.next().is_some() {
            return Err("rill lsp does not accept arguments".into());
        }
        return rill::lsp::run_stdio();
    }
    if command == OsStr::new("fmt") {
        return format_source(args);
    }
    if command == OsStr::new("import-c") {
        return import_c(args);
    }
    if command == OsStr::new("new") {
        return project::create(args);
    }
    if command == OsStr::new("project") {
        return project::run(args);
    }
    if command == OsStr::new("watch") {
        return watch::run(args);
    }
    if command == OsStr::new("docs") {
        return docgen::run(args);
    }
    let path = args.next().ok_or_else(usage)?;
    if !matches!(command.to_str(), Some("check" | "run" | "emit-c" | "build")) {
        return Err(usage());
    }
    let options = BuildOptions::parse(command.to_str().unwrap(), args)?;
    if options.native_config.is_some() {
        #[cfg(unix)]
        return config_driver::consume(command.to_str().unwrap(), Path::new(&path), &options);
        #[cfg(not(unix))]
        return Err("E-C-CONFIG: configured native commands require Unix supervision and the c-import feature".into());
    }
    let compiler = options
        .compiler
        .clone()
        .unwrap_or_else(|| env::var_os("CC").unwrap_or_else(|| "cc".into()));
    let mut arguments = vec![options.optimization.flag().into()];
    arguments.extend(options.c_args.iter().cloned());
    if options.simd == Simd::Neon {
        arguments.push("-DRILL_SIMD_NEON=1".into());
    }
    let mut prepared = native_headers::load(
        Path::new(&path),
        &compiler,
        options.zig_cc,
        arguments,
        options.interpret,
    )?;
    let program = &prepared.program;
    let native_source = !program.native.includes.is_empty()
        || !program.native.links.is_empty()
        || !program.native.packages.is_empty();
    let cflags = &prepared.cflags;
    let mut link_args = prepared.link_args.clone();
    link_args.extend(options.link_args.iter().cloned());
    let mut output = None;
    match command.to_str() {
        Some("check") => {
            if native_source {
                rill::c_import::validate_c(
                    &rill::backend::emit(program)?,
                    &compiler,
                    options.zig_cc,
                    cflags,
                )?;
            }
            prepared.publish()?;
        }
        Some("run") if options.interpret => {
            output = Some(format!("{}\n", rill::interpreter::run(program)?));
            prepared.publish()?;
        }
        Some("emit-c") => {
            let c = rill::backend::emit(program)?;
            if native_source {
                rill::c_import::validate_c(&c, &compiler, options.zig_cc, cflags)?;
            }
            prepared.publish()?;
            output = Some(c);
        }
        Some("build") => {
            build_c(
                &rill::backend::emit(program)?,
                &compiler,
                options.zig_cc,
                cflags,
                &link_args,
                Path::new(options.output.as_ref().unwrap()),
            )?;
            prepared.publish()?;
        }
        Some("run") => {
            let directory = RunDirectory::create()?;
            let binary = directory.0.join("program");
            build_c(
                &rill::backend::emit(program)?,
                &compiler,
                options.zig_cc,
                cflags,
                &link_args,
                &binary,
            )?;
            prepared.publish()?;
            let status = Command::new(&binary)
                .status()
                .map_err(|e| format!("failed to run native program: {e}"))?;
            if !status.success() {
                return Err(format!("native program failed with {status}"));
            }
        }
        _ => return Err(usage()),
    }
    if let Some(output) = output {
        print!("{output}");
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
    let mut config_out = None;
    let mut config = None;
    let mut inspect_config = None;
    let mut binding_module = None;
    let mut declared_links = Vec::new();
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
            Some("--config-out") if config_out.is_none() => config_out = Some(value(&mut args)?),
            Some("--config") if config.is_none() => config = Some(value(&mut args)?),
            Some("--inspect-config") if inspect_config.is_none() => {
                inspect_config = Some(value(&mut args)?)
            }
            Some("--binding-module") if binding_module.is_none() => {
                binding_module = Some(text(&mut args)?)
            }
            Some("--link-arg") => declared_links.push(value(&mut args)?),
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
    let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));
    let modes = usize::from(config_out.is_some())
        + usize::from(config.is_some())
        + usize::from(inspect_config.is_some());
    if modes > 1 {
        return Err(
            "E-C-CONFIG: --config-out, --config and --inspect-config are mutually exclusive".into(),
        );
    }
    if modes == 1 {
        #[cfg(not(unix))]
        return Err("E-C-CONFIG: native configuration records currently require Unix".into());
        #[cfg(unix)]
        {
            if let Some(path) = inspect_config {
                if selection != Default::default()
                    || output.is_some()
                    || compiler.is_some()
                    || timeout_ms.is_some()
                    || binding_module.is_some()
                    || !declared_links.is_empty()
                {
                    return Err(
                        "E-C-CONFIG: --inspect-config accepts no other import options".into(),
                    );
                }
                return config_driver::inspect(Path::new(&path));
            }
            if let Some(path) = config_out {
                if output.is_some() {
                    return Err("E-C-CONFIG: --config-out publishes only the configuration; -o is a separate materialization command".into());
                }
                let binding =
                    binding_module.ok_or("E-C-CONFIG: --config-out requires --binding-module")?;
                return config_driver::capture(
                    &selection,
                    &declared_links,
                    &binding,
                    Path::new(&path),
                    compiler.as_deref(),
                    zig_cc,
                    timeout,
                );
            }
            if selection != Default::default()
                || binding_module.is_some()
                || !declared_links.is_empty()
            {
                return Err("E-C-CONFIG: --config supplies the entire recipe; extra selection, dependency, C, resource or link options are not allowed".into());
            }
            return config_driver::materialize(
                Path::new(&config.unwrap()),
                output.as_deref().map(Path::new),
                compiler.as_deref(),
                zig_cc,
                timeout,
            );
        }
    }
    if binding_module.is_some() || !declared_links.is_empty() {
        return Err(
            "E-C-CONFIG: --binding-module and import --link-arg require --config-out".into(),
        );
    }
    let compiler = compiler.unwrap_or_else(|| env::var_os("CC").unwrap_or_else(|| "cc".into()));
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
    build_c_in(c, compiler, zig_cc, cflags, link_args, output, None)
}

fn build_c_in(
    c: &str,
    compiler: &OsStr,
    zig_cc: bool,
    cflags: &[OsString],
    link_args: &[OsString],
    output: &Path,
    directory: Option<&Path>,
) -> Result<(), String> {
    rill::c_import::validate_compile_args(cflags)?;
    let mut command = Command::new(compiler);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
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
            let root = std::env::temp_dir();
            let root = if root.is_absolute() {
                root
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve temporary directory: {e}"))?
                    .join(root)
            };
            let path = root.join(format!(
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
    interpret: bool,
    optimization: Optimization,
    simd: Simd,
    native_config: Option<OsString>,
    #[cfg(unix)]
    timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Simd {
    Scalar,
    Neon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Optimization {
    Level0,
    Level1,
    Level2,
    Level3,
    Size,
    SizeMin,
}

impl Optimization {
    fn parse(level: &OsStr) -> Result<Self, String> {
        match level.to_str() {
            Some("0") => Ok(Self::Level0),
            Some("1") => Ok(Self::Level1),
            Some("2") => Ok(Self::Level2),
            Some("3") => Ok(Self::Level3),
            Some("s") => Ok(Self::Size),
            Some("z") => Ok(Self::SizeMin),
            _ => Err("--opt-level requires 0, 1, 2, 3, s or z".into()),
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Level0 => "-O0",
            Self::Level1 => "-O1",
            Self::Level2 => "-O2",
            Self::Level3 => "-O3",
            Self::Size => "-Os",
            Self::SizeMin => "-Oz",
        }
    }
}

impl BuildOptions {
    fn parse(command: &str, mut args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut output = None;
        let mut compiler = None;
        let mut zig_cc = false;
        let mut link_args = Vec::new();
        let mut c_args = Vec::new();
        let mut native = false;
        let mut interpret = false;
        let mut optimization = None;
        let mut simd = None;
        let mut native_config = None;
        let mut timeout_ms = None;
        while let Some(arg) = args.next() {
            match arg.as_os_str() {
                arg if arg == OsStr::new("--native-config") => {
                    if native_config.is_some() {
                        return Err("E-C-CONFIG: only one --native-config is supported".into());
                    }
                    native_config = Some(
                        args.next()
                            .ok_or("E-C-CONFIG: --native-config requires a path")?,
                    );
                    native = true;
                }
                arg if arg == OsStr::new("--timeout-ms") => {
                    if timeout_ms.is_some() {
                        return Err("E-C-CONFIG: --timeout-ms may be specified only once".into());
                    }
                    let value = args
                        .next()
                        .ok_or("E-C-CONFIG: --timeout-ms requires an integer from 1 to 600000")?;
                    let value = value
                        .to_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|n| (1..=600_000).contains(n))
                        .ok_or("E-C-CONFIG: --timeout-ms requires an integer from 1 to 600000")?;
                    timeout_ms = Some(value);
                }
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
                arg if arg == OsStr::new("--interpret") && command == "run" => {
                    if interpret {
                        return Err("--interpret may be specified only once".into());
                    }
                    interpret = true;
                }
                arg if arg == OsStr::new("--opt-level") => {
                    if optimization.is_some() {
                        return Err("--opt-level may be specified only once".into());
                    }
                    optimization = Some(Optimization::parse(
                        &args.next().ok_or("--opt-level requires a level")?,
                    )?);
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
        if interpret && native {
            return Err("--interpret cannot be combined with native compiler options".into());
        }
        if native_config.is_some() {
            if compiler.is_none() {
                return Err(
                    "E-C-CONFIG: configured commands require explicit --cc or --zig-cc".into(),
                );
            }
            if !c_args.is_empty()
                || !link_args.is_empty()
                || simd == Some(Simd::Neon)
                || optimization.is_some()
            {
                return Err("E-C-CONFIG: saved configuration cannot be combined with --c-arg, --link-arg, --opt-level or nondefault --simd".into());
            }
        } else if timeout_ms.is_some() {
            return Err("E-C-CONFIG: --timeout-ms requires --native-config".into());
        }
        Ok(Self {
            output,
            compiler,
            zig_cc,
            link_args,
            c_args,
            interpret,
            optimization: optimization.unwrap_or(Optimization::Level3),
            simd: simd.unwrap_or(Simd::Scalar),
            native_config,
            #[cfg(unix)]
            timeout_ms,
        })
    }
}

fn usage() -> String {
    let mut help = String::from("usage: rill lsp\n       rill fmt <source.rill> [--check]\n       rill <check|emit-c> <source.rill> [--cc <compiler> | --zig-cc] [--opt-level <0|1|2|3|s|z>] [--c-arg <arg>]...\n       rill run <source.rill> [--interpret | --native] [compiler options]\n       rill build <source.rill> -o <executable> [--cc <compiler> | --zig-cc] [--opt-level <0|1|2|3|s|z>] [--simd scalar|neon] [--c-arg <arg>]... [--link-arg <arg>]...\n       rill import-c --header <header> [--function <name>]... [--constant <name>]... [-o <bindings.rill>] [--pkg-config <package>]... [--link <library>]... [--cc <compiler> | --zig-cc] [--c-arg <arg>]... [--clang-resource-dir <path>] [--timeout-ms <1..600000>]\n       rill import-c <selection> --config-out <native.rillcfg> --binding-module <logical.name> --cc <compiler> [--link-arg <validated-input>]...\n       rill import-c --config <native.rillcfg> --cc <compiler> [-o <bindings.rill>]\n       rill import-c --inspect-config <native.rillcfg>\n       rill <check|emit-c|build|run> <source.rill> --native-config <native.rillcfg> --cc <compiler> [--timeout-ms <1..600000>]");
    help.push_str("\n       rill new <directory>\n       rill project <check|build|run|watch|docs> [directory] [options]\n       rill watch <source.rill> [--interval-ms <10..60000>] [--watch <path>]... [--once] [compiler options]\n       rill docs [project-directory] [-o <directory>] [--language] [--open]");
    help
}
