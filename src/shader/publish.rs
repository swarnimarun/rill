use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;

use super::{error, read_bounded};

pub(super) const MARKER: &[u8] = b"rill shader artifacts v1\n";
static SERIAL: AtomicU64 = AtomicU64::new(0);

pub(super) struct Temporary(pub PathBuf);
impl Temporary {
    pub fn new(parent: &Path) -> Result<Self, String> {
        let parent = fs::canonicalize(parent).map_err(|e| error("OUTPUT", e.to_string()))?;
        for _ in 0..64 {
            let path = parent.join(format!(
                ".rill-shader-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(error("OUTPUT", format!("{}: {e}", path.display()))),
            }
        }
        Err(error(
            "OUTPUT",
            "could not allocate a private shader staging directory",
        ))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn prepare_parent(root: &Path, output: &Path) -> Result<(), String> {
    let relative = output
        .parent()
        .ok_or_else(|| error("OUTPUT", "output has no parent"))?
        .strip_prefix(root)
        .map_err(|_| error("OUTPUT", "output must be within the shader project"))?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(error(
                    "OUTPUT",
                    format!(
                        "output ancestor must be a real directory: {}",
                        path.display()
                    ),
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|e| error("OUTPUT", e.to_string()))?
            }
            Err(e) => return Err(error("OUTPUT", e.to_string())),
        }
    }
    validate_existing(output)
}

pub(super) fn validate_existing(output: &Path) -> Result<(), String> {
    match fs::symlink_metadata(output) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(error("OUTPUT", e.to_string())),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(error("OUTPUT", "shader output must be a real directory")),
    }
    if read_bounded(&output.join(".rill-shaders"), 128)
        .ok()
        .as_deref()
        != Some(MARKER)
    {
        return Err(error(
            "OUTPUT",
            format!(
                "refusing to replace unowned output {}; choose an empty output path",
                output.display()
            ),
        ));
    }
    let bytes = read_bounded(&output.join("manifest.json"), super::MAX_METADATA)?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|e| error("OUTPUT", format!("invalid existing shader metadata: {e}")))?;
    let mut allowed =
        BTreeSet::from([String::from(".rill-shaders"), String::from("manifest.json")]);
    let entries = manifest
        .get("artifacts")
        .and_then(Value::as_array)
        .ok_or_else(|| error("OUTPUT", "existing output has no artifact inventory"))?;
    if entries.len() > 64 {
        return Err(error("OUTPUT", "existing artifact inventory is oversized"));
    }
    for entry in entries {
        for key in ["file", "reflection"] {
            if let Some(file) = entry.get(key) {
                let file = file
                    .as_str()
                    .ok_or_else(|| error("OUTPUT", "invalid artifact filename"))?;
                let relative = super::manifest::relative(file)?;
                if relative.components().count() != 1
                    || file == "manifest.json"
                    || file == ".rill-shaders"
                {
                    return Err(error("OUTPUT", "invalid artifact filename"));
                }
                allowed.insert(file.to_owned());
            }
        }
    }
    for entry in fs::read_dir(output).map_err(|e| error("OUTPUT", e.to_string()))? {
        let entry = entry.map_err(|e| error("OUTPUT", e.to_string()))?;
        let metadata = entry
            .file_type()
            .map_err(|e| error("OUTPUT", e.to_string()))?;
        if !metadata.is_file()
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| allowed.contains(name))
        {
            return Err(error(
                "OUTPUT",
                format!(
                    "refusing to replace unknown output entry {}",
                    entry.path().display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
struct OutputLock(fs::File);
#[cfg(unix)]
impl OutputLock {
    fn acquire(output: &Path) -> Result<Self, String> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let key = super::digest(output.as_os_str().as_encoded_bytes());
        let path = output
            .parent()
            .unwrap()
            .join(format!(".rill-shader-lock-{key}"));
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| {
                error(
                    "OUTPUT",
                    format!("cannot open shader publication lock: {e}"),
                )
            })?;
        if !file
            .metadata()
            .map_err(|e| error("OUTPUT", e.to_string()))?
            .is_file()
        {
            return Err(error(
                "OUTPUT",
                "shader publication lock must be a regular file",
            ));
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(error(
                "BUSY",
                "another shader build is publishing this output; retry",
            ));
        }
        Ok(Self(file))
    }
}
#[cfg(unix)]
impl Drop for OutputLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn exchange(first: &Path, second: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let first = CString::new(first.as_os_str().as_bytes())
        .map_err(|_| error("OUTPUT", "output path contains NUL"))?;
    let second = CString::new(second.as_os_str().as_bytes())
        .map_err(|_| error("OUTPUT", "output path contains NUL"))?;
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(first.as_ptr(), second.as_ptr(), libc::RENAME_SWAP) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result != 0 {
        return Err(error("OUTPUT", format!("filesystem cannot atomically exchange shader artifacts; previous output retained: {}", std::io::Error::last_os_error())));
    }
    Ok(())
}

pub(super) fn publish(stage: Temporary, root: &Path, output: &Path) -> Result<(), String> {
    prepare_parent(root, output)?;
    #[cfg(unix)]
    let _lock = OutputLock::acquire(output)?;
    validate_existing(output)?;
    if !output.exists() {
        return fs::rename(&stage.0, output).map_err(|e| {
            error(
                "OUTPUT",
                format!("cannot publish {}: {e}", output.display()),
            )
        });
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        exchange(&stage.0, output)?;
        // The old owned directory is now private. Check again before removing it
        // in case a file was added after the ownership check before the swap.
        if let Err(reason) = validate_existing(&stage.0) {
            if let Err(restore) = exchange(&stage.0, output) {
                let path = stage.0.clone();
                std::mem::forget(stage);
                return Err(error(
                    "OUTPUT",
                    format!(
                        "{reason}; restore failed: {restore}; previous artifacts retained at {}",
                        path.display()
                    ),
                ));
            }
            return Err(reason);
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let backup = Temporary::new(output.parent().unwrap())?;
        // Windows directory rename requires a vacant destination. Remove only
        // the new private empty placeholder, never a previous artifact set.
        fs::remove_dir(&backup.0).map_err(|e| error("OUTPUT", e.to_string()))?;
        fs::rename(output, &backup.0).map_err(|e| {
            error(
                "OUTPUT",
                format!("cannot retain previous shader artifacts: {e}"),
            )
        })?;
        let result = validate_existing(&backup.0).and_then(|()| {
            fs::rename(&stage.0, output).map_err(|e| error("OUTPUT", e.to_string()))
        });
        if let Err(reason) = result {
            if let Err(restore) = fs::rename(&backup.0, output) {
                let path = backup.0.clone();
                std::mem::forget(backup);
                return Err(error(
                    "OUTPUT",
                    format!(
                        "{reason}; restore failed: {restore}; previous artifacts retained at {}",
                        path.display()
                    ),
                ));
            }
            return Err(reason);
        }
        Ok(())
    }
}
