use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde_json::{Map, Value};

use super::{error, read_bounded, MAX_SOURCE};

#[derive(Clone)]
pub(super) struct Entry {
    pub name: String,
    pub language: String,
    pub stage: String,
    pub entry: String,
    pub source: PathBuf,
    pub target: String,
    pub include_dirs: Vec<PathBuf>,
}

pub(super) struct Manifest {
    pub path: PathBuf,
    pub root: PathBuf,
    pub output: PathBuf,
    pub entries: Vec<Entry>,
    pub json: Value,
    pub bytes: Vec<u8>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = fs::canonicalize(path)
            .map_err(|e| error("MANIFEST", format!("{}: {e}", path.display())))?;
        let root = path
            .parent()
            .ok_or_else(|| error("MANIFEST", "manifest has no parent"))?
            .to_path_buf();
        let bytes = read_bounded(&path, 65_536)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| error("MANIFEST", "manifest must be UTF-8"))?;
        let json: Value =
            serde_json::from_str(text).map_err(|e| error("MANIFEST", e.to_string()))?;
        reject_duplicates(text)?;
        let top = object(&json)?;
        fields(top, &["version", "output", "shaders"])?;
        if top.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(error("MANIFEST", "version must be the integer 1"));
        }
        let output = relative(
            top.get("output")
                .and_then(Value::as_str)
                .unwrap_or("build/shaders"),
        )?;
        if top.contains_key("output") && top["output"].as_str().is_none() {
            return Err(error("MANIFEST", "output must be a string"));
        }
        let shaders = top
            .get("shaders")
            .and_then(Value::as_array)
            .ok_or_else(|| error("MANIFEST", "shaders must be an array"))?;
        if shaders.is_empty() || shaders.len() > 64 {
            return Err(error("LIMIT", "manifest must contain 1 through 64 shaders"));
        }
        let mut entries = Vec::new();
        let mut names = BTreeSet::new();
        for value in shaders {
            let value = object(value)?;
            fields(
                value,
                &[
                    "name",
                    "language",
                    "stage",
                    "entry",
                    "source",
                    "target",
                    "include_dirs",
                ],
            )?;
            let name = string(value, "name")?;
            if !identifier(name, 64) || !names.insert(name.to_ascii_lowercase()) {
                return Err(error("MANIFEST", format!("shader name {name:?} must be an ASCII identifier of at most 64 bytes, unique without case distinctions")));
            }
            let language = string(value, "language")?;
            let target = string(value, "target")?;
            if !matches!(
                (language, target),
                ("slang", "spirv" | "msl") | ("glsl" | "hlsl", "spirv") | ("metal", "msl-source")
            ) {
                return Err(error("TARGET", format!("{name}: unsupported language/target {language:?}/{target:?}; use Slang to spirv/msl, GLSL/HLSL to spirv, or Metal to explicit msl-source")));
            }
            let stage = string(value, "stage")?;
            if !matches!(stage, "vertex" | "fragment" | "compute") {
                return Err(error(
                    "STAGE",
                    format!("{name}: stage must be vertex, fragment or compute"),
                ));
            }
            let entry = string(value, "entry")?;
            if !identifier(entry, 128) || (language == "glsl" && entry != "main") {
                return Err(error(
                    "ENTRY",
                    format!(
                        "{name}: entry must be an ASCII identifier; GLSL currently requires main"
                    ),
                ));
            }
            let source = relative(string(value, "source")?)?;
            if source.starts_with(&output)
                || root.join(&source) == path
                || output.starts_with(&source)
            {
                return Err(error(
                    "OUTPUT",
                    format!("{name}: source, manifest and shader output must not overlap"),
                ));
            }
            confined(&root, &source, false)?;
            if fs::metadata(root.join(&source))
                .map_err(|e| error("INPUT", e.to_string()))?
                .len()
                > MAX_SOURCE as u64
            {
                return Err(error("LIMIT", format!("{name}: source exceeds 1 MiB")));
            }
            let mut include_dirs = Vec::new();
            if let Some(dirs) = value.get("include_dirs") {
                let dirs = dirs
                    .as_array()
                    .ok_or_else(|| error("MANIFEST", "include_dirs must be an array"))?;
                if dirs.len() > 32 {
                    return Err(error("LIMIT", "include_dirs exceeds 32 entries"));
                }
                for dir in dirs {
                    let dir =
                        relative(dir.as_str().ok_or_else(|| {
                            error("MANIFEST", "include directory must be a string")
                        })?)?;
                    confined(&root, &dir, true)?;
                    if dir.starts_with(&output) || output.starts_with(&dir) {
                        return Err(error(
                            "OUTPUT",
                            "include directories and shader output must not contain each other",
                        ));
                    }
                    include_dirs.push(dir);
                }
            }
            entries.push(Entry {
                name: name.to_owned(),
                language: language.to_owned(),
                stage: stage.to_owned(),
                entry: entry.to_owned(),
                source,
                target: target.to_owned(),
                include_dirs,
            });
        }
        let output = root.join(output);
        if path.starts_with(&output) {
            return Err(error("OUTPUT", "output would replace the shader manifest"));
        }
        Ok(Self {
            path,
            root,
            output,
            entries,
            json,
            bytes,
        })
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| error("MANIFEST", "expected an object"))
}
fn string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| error("MANIFEST", format!("{name} must be a string")))
}
fn fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(error("MANIFEST", format!("unknown field {key:?}")));
        }
    }
    Ok(())
}
pub(super) fn identifier(name: &str, limit: usize) -> bool {
    let mut bytes = name.bytes();
    !name.is_empty()
        && name.len() <= limit
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
pub(super) fn relative(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if value.is_empty()
        || value.bytes().any(|b| b < 32 || b == 127 || b == b'\\')
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(error("PATH", format!("{value:?} must be a nonempty relative path without traversal or control characters")));
    }
    Ok(path)
}
pub(super) fn confined(root: &Path, relative: &Path, directory: bool) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, Component::Normal(_)) {
            return Err(error("PATH", "path traversal is unsupported"));
        }
        path.push(part);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| error("INPUT", format!("{}: {e}", path.display())))?;
        if metadata.file_type().is_symlink() {
            return Err(error(
                "PATH",
                format!("shader paths cannot use symlinks: {}", path.display()),
            ));
        }
    }
    let metadata = fs::metadata(&path).map_err(|e| error("INPUT", e.to_string()))?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(error(
            "PATH",
            format!("{} has the wrong file kind", path.display()),
        ));
    }
    Ok(path)
}

// JSON was validated first. Decode keys before comparing, including nested
// objects, so escaped duplicate names cannot silently select another recipe.
fn reject_duplicates(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Option<BTreeSet<String>>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => stack.push(Some(BTreeSet::new())),
            b'[' => stack.push(None),
            b'}' | b']' => {
                stack.pop();
            }
            b'"' => {
                let start = i;
                i += 1;
                while bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if text[i + 1..].trim_start().starts_with(':') {
                    if let Some(Some(keys)) = stack.last_mut() {
                        let key: String = serde_json::from_str(&text[start..=i])
                            .map_err(|e| error("MANIFEST", e.to_string()))?;
                        if !keys.insert(key.clone()) {
                            return Err(error("MANIFEST", format!("duplicate field {key:?}")));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}
