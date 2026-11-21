//! Bounded fingerprints of files and C frontend search paths.
//! Captured input state is freshness data, not a native ABI certificate.
use crate::modules::ModuleError;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File},
    hash::{DefaultHasher, Hasher},
    io::Read,
    path::{Component, Path, PathBuf},
};

pub(crate) const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024;
pub(crate) const MAX_DEPENDENCIES: usize = 4096;
const MAX_DEPENDENCY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 200_000;
const MAX_DIRECTORY_DEPTH: usize = 64;

fn error(message: impl Into<String>) -> ModuleError {
    ModuleError {
        code: "E-C-IMPORT",
        message: message.into(),
    }
}

/// Search-directory state covers failed __has_include probes and headers that
/// could appear earlier in an include search path after a successful build.
pub fn search_dependencies(
    flags: &[OsString],
    positive: &[PathBuf],
) -> Result<Vec<PathBuf>, ModuleError> {
    let current = std::env::current_dir().map_err(|e| error(e.to_string()))?;
    #[cfg(target_os = "macos")]
    let mut sysroot = std::env::var_os("SDKROOT").map(PathBuf::from);
    #[cfg(not(target_os = "macos"))]
    let mut sysroot = None::<PathBuf>;
    let mut targets = flags.iter();
    while let Some(option) = targets.next() {
        if option == "-isysroot" || option == "--sysroot" {
            sysroot = Some(PathBuf::from(
                targets
                    .next()
                    .ok_or_else(|| error("missing sysroot operand"))?,
            ));
        } else if let Some(value) =
            strip_os_prefix(option, "--sysroot=").or_else(|| strip_os_prefix(option, "-isysroot"))
        {
            if value.is_empty() {
                return Err(error("empty sysroot operand"));
            }
            sysroot = Some(PathBuf::from(value));
        }
    }
    let sysroot = sysroot
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                current.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut paths = BTreeSet::new();
    let mut add = |path: PathBuf| {
        let path = if let Some(relative) = strip_os_prefix(path.as_os_str(), "=") {
            let relative = PathBuf::from(relative);
            sysroot.join(relative.strip_prefix("/").unwrap_or(&relative))
        } else {
            path
        };
        paths.insert(if path.is_absolute() {
            path
        } else {
            current.join(path)
        });
    };
    let mut options = flags.iter();
    while let Some(option) = options.next() {
        if option == "-resource-dir" {
            let path = options
                .next()
                .ok_or_else(|| error("missing resource-directory operand"))?;
            add(PathBuf::from(path).join("include"));
        } else if let Some(path) = strip_os_prefix(option, "-resource-dir=") {
            if path.is_empty() {
                return Err(error("empty resource-directory operand"));
            }
            add(PathBuf::from(path).join("include"));
        } else if ["-I", "-isystem", "-iquote", "-idirafter"]
            .iter()
            .any(|prefix| option == *prefix)
        {
            let path = options
                .next()
                .ok_or_else(|| error("missing include search-directory operand"))?;
            add(path.into());
        } else if let Some(option) = option.to_str() {
            for prefix in ["-idirafter", "-isystem", "-iquote", "-I"] {
                if let Some(path) = option.strip_prefix(prefix).filter(|path| !path.is_empty()) {
                    add(PathBuf::from(path));
                    break;
                }
            }
        }
    }
    for name in ["CPATH", "C_INCLUDE_PATH"] {
        if let Some(value) = std::env::var_os(name) {
            for path in std::env::split_paths(&value) {
                add(path);
            }
        }
    }
    for path in positive {
        if let Some(parent) = path.parent() {
            add(parent.into());
        }
    }
    if paths.len() > MAX_DEPENDENCIES {
        return Err(error("too many header search directories"));
    }
    // One recursive parent listing already covers its descendants. Keep paths
    // lexically distinct across symlinks so retargeting remains observable.
    Ok(paths
        .iter()
        .filter(|path| {
            if path.components().any(|part| part == Component::ParentDir) {
                return true;
            }
            !paths.iter().any(|parent| {
                !parent.components().any(|part| part == Component::ParentDir)
                    && *path != parent
                    && path.starts_with(parent)
            })
        })
        .cloned()
        .collect())
}

#[cfg(unix)]
fn strip_os_prefix(value: &std::ffi::OsStr, prefix: &str) -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt;
    Some(std::ffi::OsStr::from_bytes(value.as_bytes().strip_prefix(prefix.as_bytes())?).to_owned())
}
#[cfg(not(unix))]
fn strip_os_prefix(value: &std::ffi::OsStr, prefix: &str) -> Option<OsString> {
    Some(value.to_str()?.strip_prefix(prefix)?.into())
}

fn directory_fingerprint(path: &Path, work: &mut usize) -> Result<Value, ModuleError> {
    fn walk(
        path: &Path,
        known_canonical: Option<PathBuf>,
        depth: usize,
        work: &mut usize,
        expanded: &mut BTreeSet<PathBuf>,
        hash: &mut DefaultHasher,
    ) -> Result<(), ModuleError> {
        *work += 1;
        if *work > MAX_DIRECTORY_ENTRIES || depth > MAX_DIRECTORY_DEPTH {
            return Err(error("header search-directory traversal limit exceeded"));
        }
        hash.write(path_value(path).to_string().as_bytes());
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                hash.write(b"missing");
                return Ok(());
            }
            Err(e) => return Err(error(format!("{}: {e}", path.display()))),
        };
        let kind = metadata.file_type();
        hash.write(metadata_identity(&metadata).to_string().as_bytes());
        if kind.is_symlink() {
            hash.write(b"symlink");
            hash.write(
                path_value(&fs::read_link(path).map_err(|e| error(e.to_string()))?)
                    .to_string()
                    .as_bytes(),
            );
        }
        let resolved = match known_canonical {
            Some(canonical) if !kind.is_symlink() => Ok(canonical),
            _ => path.canonicalize(),
        };
        let canonical = match resolved {
            Ok(canonical) => canonical,
            Err(_) if kind.is_symlink() => {
                hash.write(b"unresolved-symlink");
                return Ok(());
            }
            Err(e) => return Err(error(e.to_string())),
        };
        hash.write(path_value(&canonical).to_string().as_bytes());
        let metadata = if kind.is_symlink() {
            fs::metadata(path).map_err(|e| error(e.to_string()))?
        } else {
            metadata
        };
        if kind.is_symlink() {
            hash.write(metadata_identity(&metadata).to_string().as_bytes());
        }
        if metadata.is_dir() {
            hash.write(b"directory");
            if !expanded.insert(canonical.clone()) {
                hash.write(b"shared-directory");
                return Ok(());
            }
            let mut entries = vec![];
            for entry in fs::read_dir(path).map_err(|e| error(e.to_string()))? {
                if entries.len() + *work >= MAX_DIRECTORY_ENTRIES {
                    return Err(error("header search-directory traversal limit exceeded"));
                }
                entries.push(entry.map_err(|e| error(e.to_string()))?.path());
            }
            entries.sort();
            for entry in entries {
                let resolved = canonical.join(
                    entry
                        .file_name()
                        .ok_or_else(|| error("invalid header search entry"))?,
                );
                walk(&entry, Some(resolved), depth + 1, work, expanded, hash)?;
            }
        } else if metadata.is_file() {
            hash.write(b"file");
        } else {
            hash.write(b"special");
        }
        Ok(())
    }
    let mut hash = DefaultHasher::new();
    let before = *work;
    walk(path, None, 0, work, &mut BTreeSet::new(), &mut hash)?;
    Ok(json!({"path":path_value(path),"kind":"search","entries":*work-before,"hash":hash.finish()}))
}

pub(crate) fn fingerprint(
    path: &Path,
    total: &mut u64,
    work: &mut usize,
) -> Result<Value, ModuleError> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| error(e.to_string()))?
            .join(path)
    };
    if !path.is_file() {
        return directory_fingerprint(&path, work);
    }
    let mut file = File::open(&path).map_err(|e| error(format!("{}: {e}", path.display())))?;
    let metadata = file.metadata().map_err(|e| error(e.to_string()))?;
    if !metadata.is_file() {
        return Err(error("header dependency is not a regular file"));
    }
    let mut hash = DefaultHasher::new();
    let mut length = 0u64;
    let mut chunk = [0u8; 16384];
    loop {
        let n = file.read(&mut chunk).map_err(|e| error(e.to_string()))?;
        if n == 0 {
            break;
        }
        length += n as u64;
        *total += n as u64;
        if length > MAX_DEPENDENCY_BYTES || *total > MAX_TOTAL_BYTES {
            return Err(error("header dependency fingerprint limit exceeded"));
        }
        hash.write(&chunk[..n]);
    }
    let canonical = path.canonicalize().map_err(|e| error(e.to_string()))?;
    Ok(
        json!({"path":path_value(&path),"canonical":path_value(&canonical),"bytes":length,"hash":hash.finish(),"identity":metadata_identity(&metadata)}),
    )
}

#[cfg(unix)]
fn metadata_identity(metadata: &fs::Metadata) -> Value {
    use std::os::unix::fs::MetadataExt;
    json!([metadata.dev(), metadata.ino()])
}
#[cfg(not(unix))]
fn metadata_identity(_: &fs::Metadata) -> Value {
    Value::Null
}

#[cfg(unix)]
pub(crate) fn path_value(path: &Path) -> Value {
    use std::os::unix::ffi::OsStrExt;
    json!(path.as_os_str().as_bytes())
}
#[cfg(not(unix))]
pub(crate) fn path_value(path: &Path) -> Value {
    json!(path.to_str())
}

#[cfg(unix)]
pub(crate) fn value_path(value: &Value) -> Option<PathBuf> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let bytes: Option<Vec<_>> = value
        .as_array()?
        .iter()
        .map(|v| u8::try_from(v.as_u64()?).ok())
        .collect();
    Some(PathBuf::from(OsString::from_vec(bytes?)))
}
#[cfg(not(unix))]
pub(crate) fn value_path(value: &Value) -> Option<PathBuf> {
    Some(PathBuf::from(value.as_str()?))
}

/// Opaque fingerprints captured before the target compiler validates an
/// imported interface. The worker protocol carries only its bounded encoding.
#[derive(Clone, Debug)]
pub struct InputState {
    pub(crate) dependencies: Vec<Value>,
}

impl InputState {
    pub fn capture(paths: &[PathBuf]) -> Result<Self, ModuleError> {
        let paths: BTreeSet<_> = paths.iter().collect();
        if paths.is_empty() || paths.len() > MAX_DEPENDENCIES {
            return Err(error("invalid header dependency count"));
        }
        let mut total = 0;
        let mut work = 0;
        let dependencies = paths
            .into_iter()
            .map(|p| fingerprint(p, &mut total, &mut work))
            .collect::<Result<_, _>>()?;
        let state = Self { dependencies };
        state.to_wire()?;
        Ok(state)
    }

    pub fn to_wire(&self) -> Result<Vec<u8>, ModuleError> {
        let bytes = serde_json::to_vec(&json!({"version":2,"dependencies":self.dependencies}))
            .map_err(|e| error(e.to_string()))?;
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return Err(error("header input state exceeds 2 MiB"));
        }
        Ok(bytes)
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, ModuleError> {
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return Err(error("header input state exceeds 2 MiB"));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| error(format!("invalid header input state: {e}")))?;
        let object = value
            .as_object()
            .ok_or_else(|| error("invalid header input state object"))?;
        if object.len() != 2 || object.get("version").and_then(Value::as_u64) != Some(2) {
            return Err(error("invalid header input state version or fields"));
        }
        let dependencies = object
            .get("dependencies")
            .and_then(Value::as_array)
            .ok_or_else(|| error("invalid header input dependencies"))?;
        if dependencies.is_empty() || dependencies.len() > MAX_DEPENDENCIES {
            return Err(error("invalid header dependency count"));
        }
        let mut paths = BTreeSet::new();
        let mut total = 0u64;
        let mut work = 0u64;
        for record in dependencies {
            let fields = record
                .as_object()
                .ok_or_else(|| error("invalid header dependency record"))?;
            if fields.get("hash").and_then(Value::as_u64).is_none() {
                return Err(error("invalid header dependency fields"));
            }
            let path = checked_state_path(
                fields
                    .get("path")
                    .ok_or_else(|| error("missing header dependency path"))?,
            )?;
            if !paths.insert(path) {
                return Err(error("duplicate header input dependency"));
            }
            if fields.get("kind").and_then(Value::as_str) == Some("search") {
                let entries = fields
                    .get("entries")
                    .and_then(Value::as_u64)
                    .filter(|n| *n > 0)
                    .ok_or_else(|| error("invalid directory fingerprint work"))?;
                work = work
                    .checked_add(entries)
                    .ok_or_else(|| error("directory fingerprint work overflow"))?;
                if fields.len() != 4
                    || fields.contains_key("canonical")
                    || fields.contains_key("bytes")
                    || work > MAX_DIRECTORY_ENTRIES as u64
                {
                    return Err(error("invalid directory fingerprint bounds"));
                }
            } else {
                let identity = fields
                    .get("identity")
                    .ok_or_else(|| error("missing header file identity"))?;
                #[cfg(unix)]
                if identity
                    .as_array()
                    .is_none_or(|v| v.len() != 2 || v.iter().any(|v| v.as_u64().is_none()))
                {
                    return Err(error("invalid header file identity"));
                }
                #[cfg(not(unix))]
                if !identity.is_null() {
                    return Err(error("invalid header file identity"));
                }
                checked_state_path(
                    fields
                        .get("canonical")
                        .ok_or_else(|| error("missing canonical dependency path"))?,
                )?;
                let length = fields
                    .get("bytes")
                    .and_then(Value::as_u64)
                    .filter(|n| *n <= MAX_DEPENDENCY_BYTES)
                    .ok_or_else(|| error("invalid header fingerprint size"))?;
                total = total
                    .checked_add(length)
                    .ok_or_else(|| error("header fingerprint size overflow"))?;
                if fields.len() != 5
                    || fields.contains_key("kind")
                    || fields.contains_key("entries")
                    || total > MAX_TOTAL_BYTES
                {
                    return Err(error("invalid header fingerprint bounds"));
                }
            }
        }
        Ok(Self {
            dependencies: dependencies.clone(),
        })
    }

    pub fn matches(&self) -> Result<bool, ModuleError> {
        let mut total = 0;
        let mut work = 0;
        for old in &self.dependencies {
            let path = old
                .get("path")
                .and_then(value_path)
                .ok_or_else(|| error("invalid captured dependency path"))?;
            if fingerprint(&path, &mut total, &mut work)? != *old {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn current(&self) -> bool {
        self.matches().unwrap_or(false)
    }

    pub fn covers(&self, positive: &[PathBuf]) -> bool {
        positive.iter().all(|path| {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                let Ok(current) = std::env::current_dir() else {
                    return false;
                };
                current.join(path)
            };
            self.dependencies.iter().any(|record| {
                record.get("bytes").is_some() && record.get("path") == Some(&path_value(&absolute))
            })
        })
    }
}

fn checked_state_path(value: &Value) -> Result<PathBuf, ModuleError> {
    #[cfg(unix)]
    if value.as_array().is_none_or(|bytes| {
        bytes.is_empty() || bytes.len() > 16 * 1024 || bytes.iter().any(|v| v.as_u64() == Some(0))
    }) {
        return Err(error("invalid header input path bytes"));
    }
    #[cfg(not(unix))]
    if value
        .as_str()
        .is_none_or(|path| path.is_empty() || path.len() > 16 * 1024 || path.contains('\0'))
    {
        return Err(error("invalid header input path string"));
    }
    let path = value_path(value)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| error("header input paths must be absolute"))?;
    Ok(path)
}

pub fn capture_inputs(
    flags: &[OsString],
    positive: &[PathBuf],
    resource_dir: Option<&Path>,
) -> Result<InputState, ModuleError> {
    let mut paths = positive.to_vec();
    paths.extend(search_dependencies(flags, positive)?);
    if let Some(resource) = resource_dir {
        paths.push(resource.join("include"));
    }
    InputState::capture(&paths)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_work_budget_includes_previously_visited_roots() {
        let root =
            std::env::temp_dir().join(format!("rill-header-work-budget-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("one.h"), "").unwrap();
        fs::write(root.join("two.h"), "").unwrap();
        let result = directory_fingerprint(&root, &mut (MAX_DIRECTORY_ENTRIES - 2));
        fs::remove_dir_all(root).unwrap();
        assert!(result.unwrap_err().message.contains("traversal limit"));
    }
    #[cfg(unix)]
    #[test]
    fn joined_search_roots_preserve_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let sysroot = OsString::from_vec(b"/chosen-sdk \xff".to_vec());
        let resource = OsString::from_vec(b"/chosen-resource \xfe".to_vec());
        let mut sdk_flag = OsString::from("-isysroot");
        sdk_flag.push(&sysroot);
        let mut resource_flag = OsString::from("-resource-dir=");
        resource_flag.push(&resource);
        let paths =
            search_dependencies(&[sdk_flag, "-I=include".into(), resource_flag], &[]).unwrap();
        assert!(paths.contains(&PathBuf::from(sysroot).join("include")));
        assert!(paths.contains(&PathBuf::from(resource).join("include")));
        // Clang's joined spelling retains an initial '=' in the operand.
        let literal =
            search_dependencies(&["-isysroot=/chosen-sdk".into(), "-I=include".into()], &[])
                .unwrap();
        assert!(literal.contains(
            &std::env::current_dir()
                .unwrap()
                .join("=/chosen-sdk/include")
        ));
    }
}
