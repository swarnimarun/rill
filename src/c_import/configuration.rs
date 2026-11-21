//! Explicit, unverified selected-C configuration records. No decoding or inspection
//! operation executes tools or confers permission to execute the saved compiler.
use super::{
    selected::{facts::FrontendFacts, Preparation, Selection},
    Resolved,
};
use std::{
    ffi::{OsStr, OsString},
    fmt::Write as _,
    fs,
    io::Read,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Component, Path, PathBuf},
};
mod arguments;
mod codec;
pub use arguments::{validate_c_args, validate_link_args};
pub use codec::{decode_preparation, encode_preparation};

pub const MAX_ARTIFACT: usize = 2 * 1024 * 1024;
pub const MAX_METADATA: usize = 512 * 1024;
pub const MAX_MODULE: usize = 1024 * 1024;
pub const MAX_ARGUMENT: usize = 64 * 1024;
pub const MAX_ITEMS: usize = 1024;
pub const GENERATOR_CONTRACT: u32 = 1;

fn error(message: impl std::fmt::Display) -> String {
    format!("E-C-CONFIG: {message}")
}
fn drift(category: &str) -> String {
    format!("E-C-CONFIG-DRIFT: {category} differs from the saved configuration; regenerate explicitly with --config-out")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    C,
    ZigCc,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerAssertion {
    pub driver: Driver,
    /// Comparison-only. Execution must use an independently resolved CLI selector.
    pub invocation_path: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkDirectory {
    RelativeToArtifactParent(PathBuf),
    Absolute(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnverifiedConfig {
    pub generator_contract: u32,
    pub work_directory: WorkDirectory,
    pub compiler_assertion: CompilerAssertion,
    pub binding_module: String,
    pub selection: Selection,
    pub declared_link_args: Vec<OsString>,
    pub expected_resolution: Resolved,
    pub expected_frontend: FrontendFacts,
    pub module_utf8: String,
}

pub fn validate_selection(selection: &Selection) -> Result<(), String> {
    selection.validate().map_err(error)?;
    for list in [
        &selection.native.includes,
        &selection.native.links,
        &selection.native.packages,
        &selection.functions,
        &selection.constants,
    ] {
        if list.len() > MAX_ITEMS {
            return Err(error("more than 1024 items in a native list"));
        }
        for value in list {
            arguments::bytes(OsStr::new(value), "native name")?;
        }
    }
    validate_c_args(&selection.c_args)?;
    if let Some(path) = &selection.resource_dir {
        arguments::operand(path.as_os_str(), "frontend resource directory")?;
    }
    Ok(())
}

/// Validate each bounded link sequence and its exact effective concatenation.
pub fn validate_link_sequence(resolved: &[OsString], declared: &[OsString]) -> Result<(), String> {
    validate_link_args(resolved)?;
    validate_link_args(declared)?;
    if resolved.len().saturating_add(declared.len()) > MAX_ITEMS {
        return Err(error("more than 1024 combined link arguments"));
    }
    let mut combined = resolved.to_vec();
    combined.extend_from_slice(declared);
    validate_link_args(&combined)
}

pub fn validate_preparation(
    preparation: &Preparation,
    selection: &Selection,
) -> Result<(), String> {
    validate_selection(selection)?;
    validate_c_args(&preparation.resolved.cflags)?;
    validate_link_args(&preparation.resolved.link_args)?;
    let mut expected = preparation.resolved.cflags.clone();
    expected.extend(selection.c_args.iter().cloned());
    validate_c_args(&expected)?;
    if preparation.cflags != expected {
        return Err(error(
            "worker common argv does not match package plus explicit argument order",
        ));
    }
    preparation
        .facts
        .validate(&selection.constants)
        .map_err(error)?;
    super::selected::shape::validate(&preparation.module, selection).map_err(error)?;
    let parsed = crate::parser::parse_module(&preparation.module).map_err(error)?;
    for (constant, fact) in parsed
        .program
        .constants
        .iter()
        .zip(&preparation.facts.constants)
    {
        use super::selected::facts::ConstantType;
        use crate::ast::ExternType;
        let matches = matches!(
            (&constant.ty, fact.rill_type),
            (ExternType::I32, ConstantType::I32)
                | (ExternType::U8, ConstantType::U8)
                | (ExternType::U32, ConstantType::U32)
                | (ExternType::U64, ConstantType::U64)
                | (ExternType::Bool, ConstantType::Bool)
        );
        if !matches {
            return Err(error(
                "constant fact type differs from generated declaration",
            ));
        }
    }
    Ok(())
}

impl UnverifiedConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.generator_contract != GENERATOR_CONTRACT {
            return Err(error("unsupported generator contract"));
        }
        validate_selection(&self.selection)?;
        validate_link_sequence(
            &self.expected_resolution.link_args,
            &self.declared_link_args,
        )?;
        validate_module_name(&self.binding_module)?;
        let path = match &self.work_directory {
            WorkDirectory::RelativeToArtifactParent(path) if !path.is_absolute() => path,
            WorkDirectory::Absolute(path) if path.is_absolute() => path,
            _ => return Err(error("working-directory path has the wrong kind")),
        };
        arguments::bytes(path.as_os_str(), "working directory")?;
        arguments::bytes(
            self.compiler_assertion.invocation_path.as_os_str(),
            "compiler assertion",
        )?;
        if !self.compiler_assertion.invocation_path.is_absolute() {
            return Err(error(
                "compiler assertion must be an absolute invocation path",
            ));
        }
        validate_preparation(&self.preparation(), &self.selection)?;
        Ok(())
    }
    fn preparation(&self) -> Preparation {
        let mut cflags = self.expected_resolution.cflags.clone();
        cflags.extend(self.selection.c_args.iter().cloned());
        Preparation {
            module: self.module_utf8.clone(),
            resolved: self.expected_resolution.clone(),
            cflags,
            facts: self.expected_frontend.clone(),
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        codec::encode(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        codec::decode(bytes)
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| error(format!("cannot read {}: {e}", path.display())))?
            .take((MAX_ARTIFACT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| error(format!("cannot read {}: {e}", path.display())))?;
        Self::decode(&bytes)
    }
    pub fn compare_compiler(&self, explicit: &CompilerAssertion) -> Result<(), String> {
        if &self.compiler_assertion != explicit {
            return Err(drift("explicit compiler driver/invocation path"));
        }
        Ok(())
    }
    pub fn compare_fresh(&self, fresh: &Preparation) -> Result<(), String> {
        validate_preparation(fresh, &self.selection)?;
        if fresh.resolved.cflags != self.expected_resolution.cflags {
            return Err(drift("ordered package C flags"));
        }
        if fresh.resolved.link_args != self.expected_resolution.link_args {
            return Err(drift("ordered package/source link arguments"));
        }
        if fresh.facts.target_triple != self.expected_frontend.target_triple {
            return Err(drift("frontend target triple"));
        }
        if fresh.facts.pointer_bits != self.expected_frontend.pointer_bits {
            return Err(drift("frontend pointer width"));
        }
        if fresh.facts.scalar_probes != self.expected_frontend.scalar_probes {
            return Err(drift("frontend scalar kinds/widths"));
        }
        if fresh.facts.constants != self.expected_frontend.constants {
            return Err(drift("selected constant type/value facts"));
        }
        if fresh.module != self.module_utf8 {
            return Err(drift("generated binding module bytes"));
        }
        Ok(())
    }
    pub fn resolve_directory(&self, artifact_path: &Path) -> Result<PathBuf, String> {
        let candidate = match &self.work_directory {
            WorkDirectory::Absolute(path) => path.clone(),
            WorkDirectory::RelativeToArtifactParent(path) => {
                artifact_parent(artifact_path)?.join(path)
            }
        };
        let directory = fs::canonicalize(&candidate).map_err(|e| {
            error(format!(
                "saved working directory {} is unavailable: {e}",
                candidate.display()
            ))
        })?;
        if !directory.is_dir() {
            return Err(error("saved working directory is not a directory"));
        }
        Ok(directory)
    }
    /// Escaped, deterministic, read-only inspection. This is not fresh validation.
    pub fn inspect(&self) -> Result<String, String> {
        self.validate()?;
        let mut out =
            String::from("Rill native configuration v1 (unverified expectations, not a cache)\n");
        writeln!(
            out,
            "generator contract: {}\nbinding module: {:?}",
            self.generator_contract, self.binding_module
        )
        .unwrap();
        let (basis, path) = match &self.work_directory {
            WorkDirectory::RelativeToArtifactParent(path) => ("artifact-parent relative", path),
            WorkDirectory::Absolute(path) => ("absolute", path),
        };
        writeln!(
            out,
            "working directory ({basis}): {}",
            escaped(path.as_os_str())
        )
        .unwrap();
        writeln!(
            out,
            "compiler assertion ({:?}, never execution authority): {}",
            self.compiler_assertion.driver,
            escaped(self.compiler_assertion.invocation_path.as_os_str())
        )
        .unwrap();
        for (label, list) in [
            ("headers", &self.selection.native.includes),
            ("source libraries", &self.selection.native.links),
            ("packages", &self.selection.native.packages),
            ("functions", &self.selection.functions),
            ("constants", &self.selection.constants),
        ] {
            writeln!(out, "{label}: {list:?}").unwrap();
        }
        for (label, list) in [
            ("explicit C arguments", &self.selection.c_args),
            ("declared link arguments", &self.declared_link_args),
            ("expected package C flags", &self.expected_resolution.cflags),
            (
                "expected package/source link arguments",
                &self.expected_resolution.link_args,
            ),
        ] {
            writeln!(out, "{label}:").unwrap();
            for (index, value) in list.iter().enumerate() {
                writeln!(out, "  [{index}] {}", escaped(value)).unwrap();
            }
        }
        if let Some(path) = &self.selection.resource_dir {
            writeln!(
                out,
                "frontend-only resource directory: {}",
                escaped(path.as_os_str())
            )
            .unwrap();
        }
        writeln!(
            out,
            "expected frontend: {:?}\nmodule UTF-8 bytes ({}):\n{}",
            self.expected_frontend,
            self.module_utf8.len(),
            escaped(OsStr::new(&self.module_utf8))
        )
        .unwrap();
        Ok(out)
    }
}

pub fn validate_module_name(name: &str) -> Result<(), String> {
    let segments: Vec<_> = name.split('.').collect();
    if name.len() > 4096
        || segments.len() > 64
        || segments.first() == Some(&"std")
        || segments.iter().any(|segment| {
            let mut bytes = segment.bytes();
            !bytes.next().is_some_and(|b| b.is_ascii_lowercase())
                || !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                || matches!(*segment, "import" | "as")
        })
    {
        return Err(error(format!(
            "invalid logical binding module name {name:?}"
        )));
    }
    Ok(())
}
fn escaped(value: &OsStr) -> String {
    let mut result = String::from("\"");
    for byte in value.as_bytes() {
        for escaped in std::ascii::escape_default(*byte) {
            result.push(char::from(escaped));
        }
    }
    result.push('"');
    result
}
fn artifact_parent(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::canonicalize(parent).map_err(|e| {
        error(format!(
            "cannot resolve configuration parent {}: {e}",
            parent.display()
        ))
    })
}
/// Both operands are actual absolute directories, so relative spelling is safe
/// without treating symlink components of arbitrary input arguments as lexical.
pub fn capture_directory(directory: &Path, artifact_path: &Path) -> Result<WorkDirectory, String> {
    let directory = fs::canonicalize(directory)
        .map_err(|e| error(format!("cannot resolve working directory: {e}")))?;
    let parent = artifact_parent(artifact_path)?;
    let a: Vec<_> = parent.components().collect();
    let b: Vec<_> = directory.components().collect();
    let shared = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    if shared == 0 {
        return Ok(WorkDirectory::Absolute(directory));
    }
    let mut relative = PathBuf::new();
    for _ in shared..a.len() {
        relative.push("..");
    }
    for part in &b[shared..] {
        relative.push(part.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(WorkDirectory::RelativeToArtifactParent(relative))
}

/// Resolve only the explicit CLI value. Do not canonicalize the final executable:
/// symlink spelling can select driver behavior and is part of the assertion.
pub fn resolve_explicit_compiler(
    selector: &OsStr,
    zig_cc: bool,
    caller: &Path,
) -> Result<CompilerAssertion, String> {
    arguments::bytes(selector, "explicit compiler")?;
    if !caller.is_absolute() {
        return Err(error(
            "compiler resolution requires an absolute caller directory",
        ));
    }
    let path = Path::new(selector);
    let executable = |path: &Path| {
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let invocation_path = if path.components().count() > 1
        || selector.as_bytes().contains(&b'/')
        || matches!(path.components().next(), Some(Component::RootDir))
    {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            caller.join(path)
        };
        if !executable(&path) {
            return Err(error(format!(
                "explicit compiler is not an executable file: {}",
                escaped(path.as_os_str())
            )));
        }
        path
    } else {
        std::env::var_os("PATH")
            .and_then(|value| {
                std::env::split_paths(&value).find_map(|part| {
                    let root = if part.is_absolute() {
                        part
                    } else {
                        caller.join(part)
                    };
                    let candidate = root.join(selector);
                    executable(&candidate).then_some(candidate)
                })
            })
            .ok_or_else(|| {
                error(format!(
                    "cannot resolve explicit compiler {} in PATH",
                    escaped(selector)
                ))
            })?
    };
    Ok(CompilerAssertion {
        driver: if zig_cc { Driver::ZigCc } else { Driver::C },
        invocation_path,
    })
}
