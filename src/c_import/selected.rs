//! Bounded, explicitly selected declaration import. No header text is parsed here.
use super::{Config, Resolved};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "c-import")]
mod clang;
pub mod facts;
pub mod shape;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub native: Config,
    pub functions: Vec<String>,
    pub constants: Vec<String>,
    pub c_args: Vec<OsString>,
    /// Optional Clang builtin-header directory. Not a target/sysroot override.
    pub resource_dir: Option<std::path::PathBuf>,
}

/// Compiler-selected uses of declarations made available by source `@include`.
/// Missing names are left to the ordinary Rill resolver.
#[derive(Clone, Debug, Default)]
pub struct Automatic {
    pub native: Config,
    pub names: Vec<String>,
    pub c_args: Vec<OsString>,
    pub resource_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Generated {
    pub module: String,
    /// Actual included headers, including transitive and frontend resource
    /// headers. Paths are absolute and preserve raw Unix path bytes.
    pub dependencies: Vec<PathBuf>,
    /// Header and search-path state captured before exact target validation.
    pub inputs: crate::c_import::inputs::InputState,
}

impl Automatic {
    fn validate(&self) -> Result<(), String> {
        self.native.validate()?;
        if self.native.includes.is_empty() {
            return Err("E-C-IMPORT: automatic import requires at least one @include".into());
        }
        if self.names.len() > 256 {
            return Err("E-C-IMPORT: automatic import exceeds 256 referenced C names".into());
        }
        let mut names = HashSet::new();
        for name in &self.names {
            if !identifier(name) || name.len() > 256 || name.starts_with("__rill_import_") {
                return Err(format!("E-C-IMPORT: invalid automatic identifier {name:?}"));
            }
            if !names.insert(name) {
                return Err(format!("E-C-IMPORT: duplicate automatic name {name}"));
            }
        }
        Ok(())
    }
}

impl Selection {
    pub fn validate(&self) -> Result<(), String> {
        self.native.validate()?;
        if self.native.includes.is_empty() {
            return Err("E-C-IMPORT: import-c requires at least one --header".into());
        }
        let count = self.functions.len() + self.constants.len();
        if count == 0 || count > 256 {
            return Err("E-C-IMPORT: select between 1 and 256 functions/constants".into());
        }
        let mut names = HashSet::new();
        for name in self.functions.iter().chain(&self.constants) {
            if !identifier(name) || name.len() > 256 {
                return Err(format!("E-C-IMPORT: invalid selected identifier {name:?}"));
            }
            if !names.insert(name) {
                return Err(format!("E-C-IMPORT: duplicate selection {name}"));
            }
        }
        Ok(())
    }
}

/// Fresh frontend observations and the exact arguments used for both checks.
/// Public fields allow bounded transport; a decoded value is not a validation
/// certificate and must be checked before use by a configured caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preparation {
    pub module: String,
    pub resolved: Resolved,
    pub cflags: Vec<OsString>,
    pub facts: facts::FrontendFacts,
}

pub(super) fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Generate and validate everything before returning output to the caller.
/// Neither this API nor the CLI publishes a partial module on failure.
/// This library API runs libclang and external tools in the caller process
/// without a hard timeout. The Unix CLI uses a separate supervised worker.
pub fn generate(selection: &Selection, compiler: &OsStr, zig_cc: bool) -> Result<String, String> {
    prepare(selection, compiler, zig_cc).map(|prepared| prepared.module)
}

/// Prepare an import in the caller process, with the same validation as
/// `generate`. This API has no hard timeout or crash isolation.
pub fn prepare(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
) -> Result<Preparation, String> {
    prepare_impl(selection, compiler, zig_cc, |_, _| Ok(()), false)
}

/// Resolve the recipe once, then apply a caller's stricter argument policy
/// before invoking libclang or the target compiler. The callback observes
/// package resolution and common C arguments in their exact original order.
/// It cannot replace them or cause a second resolution inside this operation.
pub fn prepare_checked(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
    validate: impl FnOnce(&Resolved, &[OsString]) -> Result<(), String>,
) -> Result<Preparation, String> {
    prepare_impl(selection, compiler, zig_cc, validate, true)
}

fn prepare_impl(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
    validate: impl FnOnce(&Resolved, &[OsString]) -> Result<(), String>,
    strict_kinds: bool,
) -> Result<Preparation, String> {
    selection.validate()?;
    #[cfg(not(feature = "c-import"))]
    {
        let _ = (compiler, zig_cc, validate, strict_kinds);
        Err("E-C-IMPORT: import-c requires a build with --features c-import and libclang 12 or newer".into())
    }
    #[cfg(feature = "c-import")]
    {
        let resolved = selection.native.resolve()?;
        let mut flags = resolved.cflags.clone();
        flags.extend(selection.c_args.iter().cloned());
        validate_flags(&flags)?;
        validate(&resolved, &flags)?;
        let imported = clang::extract(selection, &flags)?;
        let program = crate::modules::compile::check_binding_source(&imported.module)
            .map_err(|e| format!("E-C-IMPORT: generated binding module: {e}"))?;
        let mut c = crate::backend::emit(&program)?;
        let assertions = if strict_kinds {
            imported.facts.render_assertions(&selection.constants)?
        } else {
            imported
                .facts
                .render_legacy_assertions(&selection.constants)?
        };
        c.push_str(&assertions);
        super::validate_c(&c, compiler, zig_cc, &flags)?;
        Ok(Preparation {
            module: imported.module,
            resolved,
            cflags: flags,
            facts: imported.facts,
        })
    }
}

/// Read referenced C declarations and validate their exact target ABI before
/// returning any bindings. Like `generate`, this library API is in-process;
/// the CLI runs it through the supervised import worker.
pub fn generate_automatic(
    request: &Automatic,
    compiler: &OsStr,
    zig_cc: bool,
) -> Result<Generated, String> {
    request.validate()?;
    #[cfg(not(feature = "c-import"))]
    {
        let _ = (compiler, zig_cc);
        Err("E-C-IMPORT: automatic @include import requires a build with --features c-import and libclang 12 or newer".into())
    }
    #[cfg(feature = "c-import")]
    {
        let resolved = request.native.resolve()?;
        let mut flags = resolved.cflags;
        flags.extend(request.c_args.iter().cloned());
        validate_flags(&flags)?;
        let (imported, dependencies) = clang::extract_automatic(request, &flags)?;
        let inputs = crate::c_import::inputs::capture_inputs(
            &flags,
            &dependencies,
            request.resource_dir.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        validate_imported(&imported, compiler, zig_cc, &flags)?;
        if !inputs.current() {
            return Err("E-C-IMPORT: headers or include search paths changed during automatic import; retry the build".into());
        }
        Ok(Generated {
            module: imported.module,
            dependencies,
            inputs,
        })
    }
}

#[cfg(feature = "c-import")]
fn validate_imported(
    imported: &clang::Imported,
    compiler: &OsStr,
    zig_cc: bool,
    flags: &[OsString],
) -> Result<(), String> {
    let program = crate::modules::compile::check_binding_source(&imported.module)
        .map_err(|e| format!("E-C-IMPORT: generated binding module: {e}"))?;
    let mut c = crate::backend::emit(&program)?;
    let constants = imported
        .facts
        .constants
        .iter()
        .map(|fact| fact.name.clone())
        .collect::<Vec<_>>();
    c.push_str(&imported.facts.render_assertions(&constants)?);
    super::validate_c(&c, compiler, zig_cc, flags)
}

/// Accept only target/preprocessing options whose effect is shared by both
/// frontends. In particular -E, -S, -o, response files and extra source inputs
/// must never turn the final semantic check into preprocessing or another job.
#[cfg(feature = "c-import")]
fn validate_flags(flags: &[OsString]) -> Result<(), String> {
    if flags.len() > 1024 {
        return Err("E-C-IMPORT: too many C arguments (maximum 1024)".into());
    }
    let mut args = flags.iter();
    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str() else {
            return Err(
                "E-C-IMPORT: a C option must be UTF-8 (path operands may be non-UTF-8)".into(),
            );
        };
        if [
            "-I",
            "-D",
            "-U",
            "-isystem",
            "-iquote",
            "-idirafter",
            "-include",
            "-imacros",
            "-isysroot",
            "--sysroot",
            "-target",
            "--target",
            "-arch",
            "-resource-dir",
        ]
        .contains(&text)
        {
            let operand = args
                .next()
                .ok_or_else(|| format!("E-C-IMPORT: missing operand for {text}"))?;
            if operand.is_empty() {
                return Err(format!("E-C-IMPORT: empty operand for {text}"));
            }
        } else if [
            "-m32",
            "-m64",
            "-fshort-wchar",
            "-fshort-enums",
            "-fsigned-char",
            "-funsigned-char",
            "-fms-extensions",
            "-nostdinc",
            "-pthread",
            "-std=c11",
            "-O0",
            "-O1",
            "-O2",
            "-O3",
            "-Os",
            "-Oz",
            "-Og",
            "-Ofast",
            "-g",
            "-g0",
            "-g1",
            "-g2",
            "-g3",
            "-w",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-Werror",
            "-Wno-error",
            "-Wconversion",
            "-Wsign-conversion",
            "-Wshadow",
            "-Wno-unused-parameter",
            "-Wno-unused-function",
            "-Wno-unused-variable",
            "-Wno-unused-value",
            "-Wno-missing-field-initializers",
        ]
        .contains(&text)
            || [
                "-I",
                "-D",
                "-U",
                "--sysroot=",
                "--target=",
                "-march=",
                "-mcpu=",
                "-mabi=",
                "-mfpu=",
                "-mfloat-abi=",
                "-resource-dir=",
            ]
            .iter()
            .any(|prefix| text.starts_with(prefix) && text.len() > prefix.len())
        {
            // Preserved verbatim and in order, not interpreted by a shell.
        } else {
            return Err(format!("E-C-IMPORT: unsupported import C argument {arg:?}; only documented target/preprocessing flags are accepted"));
        }
    }
    Ok(())
}

/// Publish in the destination directory after validation. A failed write/rename
/// leaves an existing destination untouched and cleans up the temporary file.
pub fn write_atomic(path: &Path, module: &str) -> Result<(), String> {
    write_atomic_bytes(path, module.as_bytes())
}

/// Publish one complete binary or textual output with the same visibility and
/// cleanup guarantees as `write_atomic`. The caller validates before entering
/// this function; no multi-file or crash-durable directory commit is promised.
pub fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temporary, mut file) = loop {
        let candidate = parent.join(format!(
            ".rill-import-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (Temporary(candidate), file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "E-C-IMPORT: cannot prepare output {}: {e}",
                    path.display()
                ))
            }
        }
    };
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("E-C-IMPORT: cannot write output {}: {e}", path.display()))?;
    drop(file);
    std::fs::rename(&temporary.0, path)
        .map_err(|e| format!("E-C-IMPORT: cannot publish output {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_are_bounded_and_unique() {
        let mut s = Selection {
            native: Config {
                includes: vec!["api.h".into()],
                ..Config::default()
            },
            ..Selection::default()
        };
        assert!(s.validate().is_err());
        s.functions.push("add".into());
        assert!(s.validate().is_ok());
        s.constants.push("add".into());
        assert!(s.validate().is_err());
        s.constants = vec!["FLAG);\n#error injected".into()];
        assert!(s.validate().is_err());
    }
    #[cfg(feature = "c-import")]
    #[test]
    fn check_mode_cannot_be_overridden() {
        for flag in [
            "-E",
            "-S",
            "-c",
            "-o",
            "-x",
            "-xc++",
            "-std=c++17",
            "--",
            "@args",
            "extra.c",
            "-Xclang",
            "-Wp,-P",
        ] {
            assert!(validate_flags(&[flag.into()]).is_err(), "{flag}");
        }
        assert!(validate_flags(&[
            "-I".into(),
            "a path with spaces".into(),
            "-DVALUE=2".into(),
            "--target=aarch64-linux-gnu".into()
        ])
        .is_ok());
        for flag in ["-O0", "-O2"] {
            assert!(validate_flags(&[flag.into()]).is_ok(), "{flag}");
        }
    }
}
