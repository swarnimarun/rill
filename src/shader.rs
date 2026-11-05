mod inputs;
mod manifest;
mod publish;
mod tool;

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use inputs::Sources;
use manifest::{Entry, Manifest};
use serde_json::{json, Value};

const MAX_SOURCE: usize = 1024 * 1024;
const MAX_ARTIFACT: usize = 16 * 1024 * 1024;
const MAX_METADATA: usize = 4 * 1024 * 1024;

pub(super) struct ShaderProject {
    manifest: Manifest,
}
pub(super) struct Report {
    pub json: Value,
}

impl ShaderProject {
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        Ok(Self {
            manifest: Manifest::load(path)?,
        })
    }
    pub(super) fn root(&self) -> &Path {
        &self.manifest.root
    }
    pub(super) fn manifest(&self) -> &Path {
        &self.manifest.path
    }
    pub(super) fn output(&self) -> &Path {
        &self.manifest.output
    }

    pub(super) fn check(&self) -> Result<Report, String> {
        let stage = publish::Temporary::new(&std::env::temp_dir())?;
        self.compile(&stage.0)
    }
    pub(super) fn build(&self) -> Result<Report, String> {
        publish::prepare_parent(self.root(), self.output())?;
        let stage = publish::Temporary::new(self.output().parent().unwrap())?;
        let report = self.compile(&stage.0)?;
        fs::write(
            stage.0.join("manifest.json"),
            serde_json::to_vec_pretty(&report.json).map_err(|e| error("OUTPUT", e.to_string()))?,
        )
        .map_err(|e| error("OUTPUT", e.to_string()))?;
        fs::write(stage.0.join(".rill-shaders"), publish::MARKER)
            .map_err(|e| error("OUTPUT", e.to_string()))?;
        publish::publish(stage, self.root(), self.output())?;
        Ok(report)
    }

    // Preserve exact declared inputs, then watch potential shader search files.
    // Unrelated assets and symlinks in a source parent are not shader inputs.
    pub(super) fn inputs(&self) -> Result<Vec<PathBuf>, String> {
        let mut paths = BTreeSet::from([self.manifest().to_path_buf()]);
        let mut roots = BTreeSet::new();
        for entry in &self.manifest.entries {
            paths.insert(manifest::confined(self.root(), &entry.source, false)?);
            roots.insert(
                self.root()
                    .join(entry.source.parent().unwrap_or(Path::new(""))),
            );
            for directory in &entry.include_dirs {
                roots.insert(manifest::confined(self.root(), directory, true)?);
            }
        }
        self.published_inputs(&mut paths)?;
        let mut visited = BTreeSet::new();
        for root in roots {
            self.visit(&root, 0, &mut paths, &mut visited)?;
        }
        Ok(paths.into_iter().collect())
    }
    fn published_inputs(&self, paths: &mut BTreeSet<PathBuf>) -> Result<(), String> {
        if read_bounded(&self.output().join(".rill-shaders"), 128)
            .ok()
            .as_deref()
            != Some(publish::MARKER)
        {
            return Ok(());
        }
        publish::validate_existing(self.output())?;
        let bytes = read_bounded(&self.output().join("manifest.json"), MAX_METADATA)?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| error("INPUT", format!("invalid published shader inventory: {e}")))?;
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(error(
                "INPUT",
                "unsupported published shader inventory version",
            ));
        }
        let artifacts = value
            .get("artifacts")
            .and_then(Value::as_array)
            .ok_or_else(|| error("INPUT", "published shader inventory has no artifacts"))?;
        for artifact in artifacts {
            let dependencies = artifact
                .get("dependencies")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    error("INPUT", "published shader inventory has no dependency list")
                })?;
            for dependency in dependencies {
                let text = dependency
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("INPUT", "invalid published shader dependency path"))?;
                let relative = manifest::relative(text)?;
                let path = self.root().join(&relative);
                if path.starts_with(self.output()) {
                    return Err(error(
                        "INPUT",
                        "published dependency points into generated shader output",
                    ));
                }
                // Stale dependencies may have been deleted or replaced. Missing
                // paths remain exact watch candidates; symlinks are never read.
                let mut ancestor = self.root().to_path_buf();
                let mut symlink = false;
                for component in relative.components() {
                    ancestor.push(component);
                    match fs::symlink_metadata(&ancestor) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            symlink = true;
                            break;
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                        Err(e) => return Err(error("INPUT", e.to_string())),
                    }
                }
                if !symlink {
                    paths.insert(path);
                }
                if paths.len() > 4096 {
                    return Err(error("LIMIT", "shader watch inputs exceed 4096 paths"));
                }
            }
        }
        Ok(())
    }
    fn visit(
        &self,
        path: &Path,
        depth: usize,
        paths: &mut BTreeSet<PathBuf>,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        if path.starts_with(self.output()) {
            return Ok(());
        }
        if paths.len() >= 4096 || depth > 32 {
            return Err(error(
                "LIMIT",
                "shader watch inputs exceed 4096 paths or 32 directory levels",
            ));
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| error("INPUT", format!("{}: {e}", path.display())))?;
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_dir() {
            if !visited.insert(path.to_path_buf()) {
                return Ok(());
            }
            paths.insert(path.to_path_buf());
            for entry in fs::read_dir(path).map_err(|e| error("INPUT", e.to_string()))? {
                let entry = entry.map_err(|e| error("INPUT", e.to_string()))?;
                if entry.file_name().to_str().is_some_and(|n| {
                    n.starts_with('.') || matches!(n, "target" | "build" | "node_modules")
                }) {
                    continue;
                }
                self.visit(&entry.path(), depth + 1, paths, visited)?;
            }
        } else if metadata.is_file() && supported_input(path) {
            paths.insert(path.to_path_buf());
        }
        Ok(())
    }
    fn compile(&self, stage: &Path) -> Result<Report, String> {
        let mut artifacts = Vec::new();
        let mut all_sources = Sources::new();
        let mut source_bytes = 0usize;
        for entry in &self.manifest.entries {
            let (artifact, sources) = self.compile_entry(entry, stage)?;
            for (path, bytes) in sources {
                if !all_sources.contains_key(&path) {
                    source_bytes += bytes.len();
                }
                if source_bytes > 64 * 1024 * 1024 {
                    return Err(error("LIMIT", "combined shader dependencies exceed 64 MiB"));
                }
                if let Some(previous) = all_sources.insert(path.clone(), bytes.clone()) {
                    if previous != bytes {
                        return Err(error(
                            "CHANGED",
                            format!("{} changed between shader entries; retry", path.display()),
                        ));
                    }
                }
            }
            artifacts.push(artifact);
        }
        inputs::unchanged(self.root(), &all_sources)?;
        if read_bounded(self.manifest(), 65_536)? != self.manifest.bytes {
            return Err(error(
                "CHANGED",
                "shader manifest changed during compilation; retry",
            ));
        }
        let json = json!({ "version": 1, "artifacts": artifacts });
        if serde_json::to_vec_pretty(&json)
            .map_err(|e| error("OUTPUT", e.to_string()))?
            .len()
            > MAX_METADATA
        {
            return Err(error("LIMIT", "shader artifact metadata exceeds 4 MiB"));
        }
        Ok(Report { json })
    }
    fn compile_entry(&self, entry: &Entry, stage: &Path) -> Result<(Value, Sources), String> {
        let file = format!(
            "{}.{}",
            entry.name,
            if entry.target == "spirv" {
                "spv"
            } else {
                "metal"
            }
        );
        let (bytes, sources, reflection, diagnostics, validation) = if entry.target == "msl-source"
        {
            let (bytes, sources) = inputs::bundle(self.root(), entry)?;
            (
                bytes,
                sources,
                None,
                String::new(),
                "source-bundle; Metal runtime validation required",
            )
        } else {
            let work = publish::Temporary::new(stage)?;
            let compiler = tool::resolve(if entry.language == "slang" {
                "slangc"
            } else {
                "glslangValidator"
            })?;
            let depfile = format!("{}.d", entry.name);
            let reflection_file = format!("{}.reflection.json", entry.name);
            // The first compiler invocation discovers the authoritative include
            // and import graph. The second compiles a private snapshot of exactly
            // those bytes, preventing an output/hash time-of-check race.
            let args = compiler_args(
                entry,
                &self.root().join(&entry.source),
                self.root(),
                Path::new(&file),
                Path::new(&depfile),
                Path::new(&reflection_file),
            );
            tool::run(
                &compiler,
                &args,
                &work.0,
                &format!(
                    "{} ({})",
                    entry.name,
                    self.root().join(&entry.source).display()
                ),
            )?;
            let sources = inputs::capture(
                self.root(),
                &work.0,
                inputs::depfile(&work.0.join(&depfile))?,
            )?;
            if sources
                .keys()
                .any(|path| self.root().join(path).starts_with(self.output()))
            {
                return Err(error(
                    "OUTPUT",
                    "shader dependencies cannot read from the generated output directory",
                ));
            }
            if !sources.contains_key(&entry.source) {
                return Err(error(
                    "INCLUDE",
                    format!(
                        "{}: compiler dependency file omitted the shader source",
                        entry.name
                    ),
                ));
            }
            let snapshot = work.0.join("snapshot");
            inputs::snapshot(&snapshot, entry, &sources)?;
            for file in [&file, &depfile, &reflection_file] {
                let path = work.0.join(file);
                if path.exists() {
                    fs::remove_file(path).map_err(|e| error("OUTPUT", e.to_string()))?;
                }
            }
            let args = compiler_args(
                entry,
                &Path::new(".").join(&entry.source),
                Path::new(""),
                &Path::new("..").join(&file),
                &Path::new("..").join(&depfile),
                &Path::new("..").join(&reflection_file),
            );
            let diagnostics = tool::run(
                &compiler,
                &args,
                &snapshot,
                &format!(
                    "{} ({})",
                    entry.name,
                    self.root().join(&entry.source).display()
                ),
            )
            .map_err(|message| {
                message.replace(
                    &snapshot.to_string_lossy().to_string(),
                    &self.root().to_string_lossy(),
                )
            })?;
            let final_sources = inputs::capture(
                &snapshot,
                &snapshot,
                inputs::depfile(&work.0.join(&depfile))?,
            )?;
            if final_sources != sources {
                return Err(error("INCLUDE", format!("{}: dependency graph changed while snapshotting; use confined relative includes/imports and retry", entry.name)));
            }
            let bytes = read_bounded(&work.0.join(&file), MAX_ARTIFACT)?;
            let mut diagnostics = diagnostics;
            if entry.target == "spirv" {
                let validator = tool::resolve("spirv-val")?;
                let log = tool::run(
                    &validator,
                    &[
                        "--target-env".into(),
                        "vulkan1.0".into(),
                        OsString::from(&file),
                    ],
                    &work.0,
                    &format!("{} SPIR-V validation", entry.name),
                )?;
                diagnostics.push_str(&log);
                validate_spirv(&bytes, entry)?;
            } else {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| error("ARTIFACT", "Slang Metal output must be UTF-8"))?;
                if text.trim().is_empty() {
                    return Err(error("ARTIFACT", "Slang produced empty Metal output"));
                }
            }
            let reflection = if entry.language == "slang" {
                let bytes = read_bounded(&work.0.join(&reflection_file), MAX_METADATA)?;
                let reflection: Value = serde_json::from_slice(&bytes)
                    .map_err(|e| error("REFLECTION", e.to_string()))?;
                let entries = reflection
                    .get("entryPoints")
                    .and_then(Value::as_array)
                    .ok_or_else(|| error("REFLECTION", "Slang reflection omitted entryPoints"))?;
                if !entries.iter().any(|value| {
                    value.get("name").and_then(Value::as_str) == Some(&entry.entry)
                        && value.get("stage").and_then(Value::as_str) == Some(&entry.stage)
                }) {
                    return Err(error(
                        "REFLECTION",
                        format!(
                            "{}: Slang reflection omitted the selected entry/stage",
                            entry.name
                        ),
                    ));
                }
                Some((reflection_file, bytes))
            } else {
                None
            };
            (
                bytes,
                sources,
                reflection,
                diagnostics,
                if entry.target == "spirv" {
                    "spirv-val; Vulkan 1.0"
                } else {
                    "slang-compiled-msl; Metal runtime validation required"
                },
            )
        };
        if bytes.is_empty() || bytes.len() > MAX_ARTIFACT {
            return Err(error(
                "ARTIFACT",
                format!("{}: artifact must be 1 byte through 16 MiB", entry.name),
            ));
        }
        fs::write(stage.join(&file), &bytes).map_err(|e| error("OUTPUT", e.to_string()))?;
        let mut metadata = json!({ "name": entry.name, "source": entry.source, "language": entry.language, "stage": entry.stage, "entry": entry.entry, "target": entry.target, "file": file, "sha256": digest(&bytes), "validation": validation, "dependencies": sources.iter().map(|(path, bytes)| json!({"path":path,"sha256":digest(bytes)})).collect::<Vec<_>>() });
        if let Some((file, bytes)) = reflection {
            fs::write(stage.join(&file), bytes).map_err(|e| error("OUTPUT", e.to_string()))?;
            metadata["reflection"] = Value::String(file);
        }
        if !diagnostics.trim().is_empty() {
            metadata["diagnostics"] = Value::String(diagnostics);
        }
        Ok((metadata, sources))
    }
}

fn compiler_args(
    entry: &Entry,
    source: &Path,
    root: &Path,
    output: &Path,
    depfile: &Path,
    reflection: &Path,
) -> Vec<OsString> {
    let mut args = vec![source.as_os_str().to_os_string()];
    if entry.language == "slang" {
        args.extend([
            "-lang".into(),
            "slang".into(),
            "-entry".into(),
            entry.entry.clone().into(),
            "-stage".into(),
            entry.stage.clone().into(),
            "-target".into(),
            if entry.target == "spirv" {
                "spirv"
            } else {
                "metal"
            }
            .into(),
            "-profile".into(),
            if entry.target == "spirv" {
                "spirv_1_0"
            } else {
                "sm_6_0"
            }
            .into(),
            "-matrix-layout-column-major".into(),
            "-fvk-use-entrypoint-name".into(),
            "-depfile".into(),
            depfile.as_os_str().to_owned(),
            "-reflection-json".into(),
            reflection.as_os_str().to_owned(),
        ]);
        for include in &entry.include_dirs {
            args.push("-I".into());
            args.push(root.join(include).into_os_string());
        }
    } else {
        if entry.language == "hlsl" {
            args.push("-D".into());
        }
        let stage = match entry.stage.as_str() {
            "vertex" => "vert",
            "fragment" => "frag",
            _ => "comp",
        };
        args.extend([
            "-V".into(),
            "--target-env".into(),
            "vulkan1.0".into(),
            "-S".into(),
            stage.into(),
            "-e".into(),
            entry.entry.clone().into(),
            "--depfile".into(),
            depfile.as_os_str().to_owned(),
        ]);
        for include in &entry.include_dirs {
            let mut argument = OsString::from("-I");
            argument.push(root.join(include));
            args.push(argument);
        }
    }
    args.push("-o".into());
    args.push(output.as_os_str().to_owned());
    args
}

fn validate_spirv(bytes: &[u8], entry: &Entry) -> Result<(), String> {
    if bytes.len() < 20 || !bytes.len().is_multiple_of(4) {
        return Err(error(
            "ARTIFACT",
            "SPIR-V must contain a complete word-aligned header",
        ));
    }
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    if words[0] != 0x0723_0203 || words[3] == 0 || words[4] != 0 {
        return Err(error("ARTIFACT", "invalid SPIR-V header"));
    }
    let model = match entry.stage.as_str() {
        "vertex" => 0,
        "fragment" => 4,
        _ => 5,
    };
    let mut found = 0;
    let mut offset = 5;
    while offset < words.len() {
        let count = (words[offset] >> 16) as usize;
        if count == 0 || count > words.len() - offset {
            return Err(error("ARTIFACT", "invalid SPIR-V instruction extent"));
        }
        if words[offset] & 0xffff == 15 {
            if count < 4 {
                return Err(error("ARTIFACT", "invalid SPIR-V entry point"));
            }
            let name: Vec<u8> = words[offset + 3..offset + count]
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .take_while(|b| *b != 0)
                .collect();
            if words[offset + 1] == model && name == entry.entry.as_bytes() {
                found += 1;
            }
        }
        offset += count;
    }
    if found != 1 {
        return Err(error(
            "ENTRY",
            format!(
                "{}: SPIR-V does not contain exactly one {:?} entry {:?}",
                entry.name, entry.stage, entry.entry
            ),
        ));
    }
    Ok(())
}

pub(super) fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let action = args.next().ok_or_else(usage)?;
    if action == "--help" || action == "-h" {
        println!("{}", usage());
        return Ok(());
    }
    if !matches!(action.to_str(), Some("check" | "build" | "inspect")) {
        return Err(usage());
    }
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| "rill.shaders.json".into());
    if args.next().is_some() {
        return Err(usage());
    }
    let project = ShaderProject::load(&path)?;
    let value = match action.to_str().unwrap() {
        "check" => project.check()?.json,
        "build" => project.build()?.json,
        _ => {
            json!({ "manifest": project.manifest.json, "output": project.output(), "note": "inspect parses recipes without invoking compilers or validating shader code" })
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| error("OUTPUT", e.to_string()))?
    );
    Ok(())
}
fn usage() -> String {
    "usage: rill shader <check|build|inspect> [rill.shaders.json]".into()
}
fn error(code: &str, message: impl std::fmt::Display) -> String {
    format!("E-SHADER-{code}: {message}")
}
fn digest(bytes: &[u8]) -> String {
    crate::fingerprint::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| error("IO", format!("{}: {e}", path.display())))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit as u64 {
        return Err(error(
            "LIMIT",
            format!(
                "{} must be a regular file of at most {limit} bytes",
                path.display()
            ),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| error("IO", format!("{}: {e}", path.display())))?;
    if bytes.len() > limit {
        return Err(error(
            "LIMIT",
            format!("{} grew beyond {limit} bytes", path.display()),
        ));
    }
    Ok(bytes)
}

pub(super) fn supported_input(path: &Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some(
            "slang"
                | "slangh"
                | "glsl"
                | "vert"
                | "frag"
                | "comp"
                | "hlsl"
                | "hlsli"
                | "metal"
                | "inc"
                | "h"
                | "glsli"
        )
    )
}
