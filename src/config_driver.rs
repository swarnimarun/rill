//! CLI-only configured native commands. Decoding never constructs the private
//! invocation below: only a fresh supervised preparation can populate it.
use super::*;
#[cfg(feature = "c-import")]
use config::{CompilerAssertion, Driver, UnverifiedConfig};
use rill::c_import::{configuration as config, selected::Selection};
use std::time::Duration;

pub(super) fn inspect(path: &Path) -> Result<(), String> {
    let output = config::UnverifiedConfig::read(path)?.inspect()?;
    std::io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|e| format!("E-C-CONFIG: cannot write inspection: {e}"))
}

#[cfg(feature = "c-import")]
fn explicit(
    compiler: Option<&OsStr>,
    zig_cc: bool,
) -> Result<(PathBuf, CompilerAssertion), String> {
    let compiler =
        compiler.ok_or("E-C-CONFIG: configured commands require explicit --cc or --zig-cc")?;
    let caller = env::current_dir()
        .map_err(|e| format!("E-C-CONFIG: cannot read working directory: {e}"))?;
    let assertion = config::resolve_explicit_compiler(compiler, zig_cc, &caller)?;
    Ok((caller, assertion))
}

pub(super) fn capture(
    selection: &Selection,
    declared_links: &[OsString],
    binding: &str,
    path: &Path,
    compiler: Option<&OsStr>,
    zig_cc: bool,
    timeout: Duration,
) -> Result<(), String> {
    #[cfg(not(feature = "c-import"))]
    {
        let _ = (
            selection,
            declared_links,
            binding,
            path,
            compiler,
            zig_cc,
            timeout,
        );
        Err(
            "E-C-CONFIG: configured native commands require a build with --features c-import"
                .into(),
        )
    }
    #[cfg(feature = "c-import")]
    {
        config::validate_selection(selection)?;
        config::validate_link_args(declared_links)?;
        config::validate_module_name(binding)?;
        let (caller, explicit) = explicit(compiler, zig_cc)?;
        let directory = config::capture_directory(&caller, path)?;
        let fresh = import_worker::prepare(
            selection,
            explicit.invocation_path.as_os_str(),
            zig_cc,
            declared_links,
            &caller,
            timeout,
        )?;
        let artifact = UnverifiedConfig {
            generator_contract: config::GENERATOR_CONTRACT,
            work_directory: directory,
            compiler_assertion: explicit,
            binding_module: binding.into(),
            selection: selection.clone(),
            declared_link_args: declared_links.to_vec(),
            expected_resolution: fresh.resolved,
            expected_frontend: fresh.facts,
            module_utf8: fresh.module,
        };
        let bytes = artifact.encode()?;
        rill::c_import::selected::write_atomic_bytes(path, &bytes)
    }
}

pub(super) fn materialize(
    path: &Path,
    output: Option<&Path>,
    compiler: Option<&OsStr>,
    zig_cc: bool,
    timeout: Duration,
) -> Result<(), String> {
    #[cfg(not(feature = "c-import"))]
    {
        let _ = (path, output, compiler, zig_cc, timeout);
        Err(
            "E-C-CONFIG: configured native commands require a build with --features c-import"
                .into(),
        )
    }
    #[cfg(feature = "c-import")]
    {
        let (_, explicit) = explicit(compiler, zig_cc)?;
        let artifact = UnverifiedConfig::read(path)?;
        artifact.compare_compiler(&explicit)?;
        let directory = artifact.resolve_directory(path)?;
        let fresh = import_worker::prepare(
            &artifact.selection,
            explicit.invocation_path.as_os_str(),
            zig_cc,
            &artifact.declared_link_args,
            &directory,
            timeout,
        )?;
        artifact.compare_fresh(&fresh)?;
        if let Some(output) = output {
            rill::c_import::selected::write_atomic(output, &fresh.module)
        } else {
            std::io::stdout()
                .lock()
                .write_all(fresh.module.as_bytes())
                .map_err(|e| format!("E-C-CONFIG: cannot write bindings: {e}"))
        }
    }
}

#[cfg(feature = "c-import")]
struct ValidatedInvocation {
    compiler: CompilerAssertion,
    directory: PathBuf,
    cflags: Vec<OsString>,
    link_args: Vec<OsString>,
    assertions: String,
}
#[cfg(feature = "c-import")]
impl ValidatedInvocation {
    fn prepare(
        artifact: &UnverifiedConfig,
        path: &Path,
        compiler: CompilerAssertion,
        timeout: Duration,
    ) -> Result<Self, String> {
        artifact.compare_compiler(&compiler)?;
        let directory = artifact.resolve_directory(path)?;
        let fresh = import_worker::prepare(
            &artifact.selection,
            compiler.invocation_path.as_os_str(),
            compiler.driver == Driver::ZigCc,
            &artifact.declared_link_args,
            &directory,
            timeout,
        )?;
        artifact.compare_fresh(&fresh)?;
        let assertions = fresh
            .facts
            .render_assertions(&artifact.selection.constants)?;
        let mut link_args = fresh.resolved.link_args;
        link_args.extend(artifact.declared_link_args.iter().cloned());
        // Validate the composed ordered sequence too; separate groups may not
        // smuggle state between package resolution and declared arguments.
        config::validate_link_args(&link_args)?;
        Ok(Self {
            compiler,
            directory,
            cflags: fresh.cflags,
            link_args,
            assertions,
        })
    }
    fn validate(&self, c: &str) -> Result<(), String> {
        rill::c_import::validate_c_in(
            c,
            self.compiler.invocation_path.as_os_str(),
            self.compiler.driver == Driver::ZigCc,
            &self.cflags,
            Some(&self.directory),
        )
    }
    fn build(&self, c: &str, output: &Path) -> Result<(), String> {
        // Reserve a same-directory temporary and publish one complete executable
        // only after successful compilation. This does not change legacy builds.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Temporary(PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let parent = output.parent().unwrap_or(Path::new("."));
        let temporary = loop {
            let path = parent.join(format!(
                ".rill-native-{}-{}.tmp",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    drop(file);
                    break Temporary(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("E-C-CONFIG: cannot prepare executable output: {e}")),
            }
        };
        build_c_in(
            c,
            self.compiler.invocation_path.as_os_str(),
            self.compiler.driver == Driver::ZigCc,
            &self.cflags,
            &self.link_args,
            &temporary.0,
            Some(&self.directory),
        )?;
        std::fs::File::open(&temporary.0)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("E-C-CONFIG: cannot sync executable: {e}"))?;
        std::fs::rename(&temporary.0, output).map_err(|e| {
            format!(
                "E-C-CONFIG: cannot publish executable {}: {e}",
                output.display()
            )
        })
    }
}

pub(super) fn consume(command: &str, entry: &Path, options: &BuildOptions) -> Result<(), String> {
    #[cfg(not(feature = "c-import"))]
    {
        let _ = (command, entry, options.timeout_ms);
        Err(
            "E-C-CONFIG: configured native commands require a build with --features c-import"
                .into(),
        )
    }
    #[cfg(feature = "c-import")]
    {
        let (caller, explicit) = explicit(options.compiler.as_deref(), options.zig_cc)?;
        let path = Path::new(options.native_config.as_ref().unwrap());
        let artifact = UnverifiedConfig::read(path)?;
        artifact.compare_compiler(&explicit)?;
        let snapshot =
            rill::modules::compile::load_program_snapshot(entry).map_err(|e| e.to_string())?;
        let source = snapshot.source(&artifact.binding_module).ok_or_else(|| {
            format!(
                "E-C-CONFIG-DRIFT: binding module {:?} is missing from the loaded program",
                artifact.binding_module
            )
        })?;
        if source != artifact.module_utf8 {
            return Err("E-C-CONFIG-DRIFT: loaded binding module bytes differ; materialize the saved configuration explicitly".into());
        }
        rill::c_import::selected::shape::validate(source, &artifact.selection)?;
        let program = snapshot.program();
        if program.native != artifact.selection.native {
            return Err("E-C-CONFIG-DRIFT: flattened program has additional or different native dependencies; capture a combined configuration explicitly".into());
        }
        let invocation = ValidatedInvocation::prepare(
            &artifact,
            path,
            explicit,
            Duration::from_millis(options.timeout_ms.unwrap_or(30_000)),
        )?;
        let mut c = rill::backend::emit(program)?;
        c.push_str(&invocation.assertions);
        match command {
            "check" => invocation.validate(&c),
            "emit-c" => {
                invocation.validate(&c)?;
                std::io::stdout()
                    .lock()
                    .write_all(c.as_bytes())
                    .map_err(|e| format!("E-C-CONFIG: cannot write C output: {e}"))
            }
            "build" => {
                let output = Path::new(options.output.as_ref().unwrap());
                let output = if output.is_absolute() {
                    output.to_path_buf()
                } else {
                    caller.join(output)
                };
                invocation.build(&c, &output)
            }
            "run" => {
                let directory = RunDirectory::create()?;
                let binary = directory.0.join("program");
                invocation.build(&c, &binary)?;
                // The compiler's fixed directory is not the user's runtime CWD.
                let status = Command::new(&binary)
                    .status()
                    .map_err(|e| format!("failed to run native program: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("native program failed with {status}"))
                }
            }
            _ => Err(usage()),
        }
    }
}
