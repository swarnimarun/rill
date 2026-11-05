//! Relocatable tools and native packages shipped with a Rill distribution.
//!
//! Source checkouts fall back to PATH/pkg-config. An explicitly selected or
//! adjacent SDK must be valid; a broken bundle never silently changes providers.
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde_json::{Map, Value};

const MARKER: &str = "toolchain.json";
const LIMIT: u64 = 65_536;

struct Bundle {
    root: PathBuf,
    value: Map<String, Value>,
}

/// Locate a named bundled executable, without consulting PATH.
/// Explicit tool overrides belong to the caller and take precedence.
pub fn tool(name: &str) -> Result<Option<PathBuf>, String> {
    Bundle::discover()?
        .map(|bundle| bundle.tool(name))
        .transpose()
        .map(Option::flatten)
}

/// Resolve a bundled native package while retaining its declared link order.
pub fn package(name: &str) -> Result<Option<crate::c_import::Resolved>, String> {
    Bundle::discover()?
        .map(|bundle| bundle.package(name))
        .transpose()
        .map(Option::flatten)
}

/// Add bundled DLL/runtime search paths to a child process. Existing paths follow
/// the SDK entries. Unix link commands also receive explicit runtime search paths.
pub fn configure_runtime(command: &mut Command) -> Result<(), String> {
    let Some(bundle) = Bundle::discover()? else {
        return Ok(());
    };
    // Preserve an explicitly relative SDK selection when a child changes cwd.
    command.env("RILL_TOOLCHAIN_ROOT", &bundle.root);
    let paths = bundle.directories("runtime_dirs", &bundle.value)?;
    if paths.is_empty() {
        return Ok(());
    }
    let variable = if cfg!(target_os = "windows") {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let mut combined = paths;
    if let Some(existing) = std::env::var_os(variable) {
        combined.extend(std::env::split_paths(&existing));
    }
    let joined = std::env::join_paths(combined)
        .map_err(|e| format!("E-TOOLCHAIN: cannot form runtime search path: {e}"))?;
    command.env(variable, joined);
    Ok(())
}

impl Bundle {
    fn discover() -> Result<Option<Self>, String> {
        if let Some(root) = std::env::var_os("RILL_TOOLCHAIN_ROOT") {
            if root.is_empty() {
                return Err("E-TOOLCHAIN: RILL_TOOLCHAIN_ROOT is empty".into());
            }
            return Self::load(Path::new(&root)).map(Some);
        }
        let executable = std::env::current_exe()
            .map_err(|e| format!("E-TOOLCHAIN: cannot locate Rill executable: {e}"))?;
        let Some(prefix) = executable.parent().and_then(Path::parent) else {
            return Ok(None);
        };
        let root = prefix.join("libexec/rill");
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("E-TOOLCHAIN: cannot inspect SDK: {error}")),
            Ok(_) => {}
        }
        Self::load(&root).map(Some)
    }

    fn load(root: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(root)
            .map_err(|e| format!("E-TOOLCHAIN: cannot open SDK {}: {e}", root.display()))?;
        let path = root.join(MARKER);
        let metadata = fs::metadata(&path)
            .map_err(|e| format!("E-TOOLCHAIN: cannot read {}: {e}", path.display()))?;
        if !metadata.is_file() || metadata.len() > LIMIT {
            return Err("E-TOOLCHAIN: toolchain.json must be a file of at most 64 KiB".into());
        }
        let mut text = String::new();
        fs::File::open(&path)
            .and_then(|file| file.take(LIMIT + 1).read_to_string(&mut text))
            .map_err(|e| format!("E-TOOLCHAIN: {e}"))?;
        if text.len() as u64 > LIMIT {
            return Err("E-TOOLCHAIN: toolchain.json exceeds 64 KiB".into());
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("E-TOOLCHAIN: invalid toolchain.json: {e}"))?;
        reject_duplicates(&text)?;
        let value = value
            .as_object()
            .ok_or("E-TOOLCHAIN: manifest must be an object")?
            .clone();
        fields(
            &value,
            &["version", "rill", "tools", "packages", "runtime_dirs"],
        )?;
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return Err("E-TOOLCHAIN: expected manifest version 1".into());
        }
        if value.get("rill").and_then(Value::as_str) != Some(env!("CARGO_PKG_VERSION")) {
            return Err(format!(
                "E-TOOLCHAIN: SDK must match Rill {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        for key in ["tools", "packages"] {
            if !value.get(key).is_some_and(Value::is_object) {
                return Err(format!("E-TOOLCHAIN: {key} must be an object"));
            }
        }
        let bundle = Self { root, value };
        bundle.directories("runtime_dirs", &bundle.value)?;
        Ok(bundle)
    }

    fn path(&self, text: &str, directory: bool) -> Result<PathBuf, String> {
        let relative = Path::new(text);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(format!(
                "E-TOOLCHAIN: expected confined relative path, got {text:?}"
            ));
        }
        let path = fs::canonicalize(self.root.join(relative))
            .map_err(|e| format!("E-TOOLCHAIN: missing SDK path {text:?}: {e}"))?;
        if !path.starts_with(&self.root)
            || if directory {
                !path.is_dir()
            } else {
                !path.is_file()
            }
        {
            return Err(format!(
                "E-TOOLCHAIN: invalid or escaping SDK path {text:?}"
            ));
        }
        Ok(path)
    }

    fn directories(&self, key: &str, object: &Map<String, Value>) -> Result<Vec<PathBuf>, String> {
        let values = object
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("E-TOOLCHAIN: {key} must be an array"))?;
        values
            .iter()
            .map(|value| {
                self.path(
                    value
                        .as_str()
                        .ok_or_else(|| format!("E-TOOLCHAIN: {key} paths must be strings"))?,
                    true,
                )
            })
            .collect()
    }

    fn tool(&self, name: &str) -> Result<Option<PathBuf>, String> {
        let tools = self.value["tools"].as_object().unwrap();
        let Some(value) = tools.get(name) else {
            return Ok(None);
        };
        let text = value
            .as_str()
            .ok_or("E-TOOLCHAIN: tool paths must be strings")?;
        self.path(text, false).map(Some)
    }

    fn package(&self, name: &str) -> Result<Option<crate::c_import::Resolved>, String> {
        let packages = self.value["packages"].as_object().unwrap();
        let Some(value) = packages.get(name) else {
            return Ok(None);
        };
        let package = value
            .as_object()
            .ok_or("E-TOOLCHAIN: package must be an object")?;
        fields(package, &["include_dirs", "library_dirs", "libraries"])?;
        let includes = self.directories("include_dirs", package)?;
        let libraries = self.directories("library_dirs", package)?;
        let names = package
            .get("libraries")
            .and_then(Value::as_array)
            .ok_or("E-TOOLCHAIN: libraries must be an array")?;
        let mut result = crate::c_import::Resolved::default();
        for path in includes {
            result
                .cflags
                .extend([OsString::from("-I"), path.into_os_string()]);
        }
        for path in libraries {
            result
                .link_args
                .extend([OsString::from("-L"), path.as_os_str().to_owned()]);
            #[cfg(unix)]
            result.link_args.extend([
                OsString::from("-Xlinker"),
                OsString::from("-rpath"),
                OsString::from("-Xlinker"),
                path.into_os_string(),
            ]);
        }
        for name in names {
            let name = name
                .as_str()
                .ok_or("E-TOOLCHAIN: library names must be strings")?;
            crate::c_import::Config {
                links: vec![name.into()],
                ..Default::default()
            }
            .validate()?;
            result.link_args.push(format!("-l{name}").into());
        }
        Ok(Some(result))
    }
}

fn fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("E-TOOLCHAIN: unknown field {key:?}"));
        }
    }
    Ok(())
}

// The JSON parser has already validated the grammar. Track every object, so a
// last-value-wins map cannot conceal a duplicate SDK setting at any depth.
fn reject_duplicates(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Option<BTreeSet<String>>> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => stack.push(Some(BTreeSet::new())),
            b'[' => stack.push(None),
            b'}' | b']' => {
                stack.pop();
            }
            b'"' => {
                let start = index;
                index += 1;
                while bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                let mut next = index + 1;
                while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':') {
                    let key: String = serde_json::from_str(&text[start..=index]).unwrap();
                    if let Some(Some(keys)) = stack.last_mut() {
                        if !keys.insert(key.clone()) {
                            return Err(format!("E-TOOLCHAIN: duplicate field {key:?}"));
                        }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
