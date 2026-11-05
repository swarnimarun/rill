//! Best-effort reuse of checked, native-free generated C.
//!
//! Eligibility and checking belong to the driver. An entry records exact source
//! snapshots and the compiler build identity; it does not cache a native object,
//! executable, foreign declaration, or proof about the current C toolchain.
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

use rill::modules::ModuleGraph;

use crate::fingerprint::digest;

#[cfg(unix)]
const NAMESPACE: &str = "checked-c-v1";
const KEY_SCHEMA: &[u8] = b"rill-checked-c-key-v1\0";
const MAGIC: &[u8] = b"RILL-CHECKED-C-1\0";
const MAX_KEY_BYTES: usize = 64 * 1024 * 1024;
const MAX_C_BYTES: usize = 32 * 1024 * 1024;
const HEADER_BYTES: usize = MAGIC.len() + 8 + 8 + 32;
const MAX_ENTRY_BYTES: usize = HEADER_BYTES + MAX_KEY_BYTES + MAX_C_BYTES;
#[cfg(unix)]
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

pub(super) struct Cache {
    directory: PathBuf,
}

impl Cache {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub(super) fn lookup(&self, graph: &ModuleGraph) -> Option<String> {
        let key = source_key(graph, env!("RILL_COMPILER_ID"))?;
        let directory = cache_directory(&self.directory, false)?;
        let mut file = open_entry(&directory, &entry_name(&key))?;
        let metadata = file.metadata().ok()?;
        if metadata.len() > MAX_ENTRY_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_ENTRY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        decode(&bytes, &key)
    }

    pub(super) fn store(&self, graph: &ModuleGraph, c: &str) {
        if c.len() > MAX_C_BYTES {
            return;
        }
        let Some(key) = source_key(graph, env!("RILL_COMPILER_ID")) else {
            return;
        };
        let Some(directory) = cache_directory(&self.directory, true) else {
            return;
        };
        let name = entry_name(&key);
        if !replaceable_entry(&directory, &name) {
            return;
        }
        let Some((temporary, mut file)) = temporary_entry(&directory) else {
            return;
        };
        let written = (|| -> std::io::Result<()> {
            file.write_all(MAGIC)?;
            file.write_all(&(key.len() as u64).to_le_bytes())?;
            file.write_all(&(c.len() as u64).to_le_bytes())?;
            file.write_all(&digest(c.as_bytes()))?;
            file.write_all(&key)?;
            file.write_all(c.as_bytes())?;
            file.sync_all()
        })();
        if written.is_ok() {
            publish(&directory, &temporary, &name);
        }
        remove_temporary(&directory, &temporary);
    }
}

fn source_key(graph: &ModuleGraph, compiler: &str) -> Option<Vec<u8>> {
    let mut key = Key(Vec::new());
    key.bytes(KEY_SCHEMA)?;
    key.bytes(compiler.as_bytes())?;
    key.path(&graph.root)?;
    key.number(graph.modules.len())?;
    for (name, module) in &graph.modules {
        key.bytes(name.as_bytes())?;
        key.bytes(module.name.as_bytes())?;
        key.path(&module.path)?;
        key.bytes(module.source.as_bytes())?;
        key.number(module.imports.len())?;
        for import in &module.imports {
            key.bytes(import.module.as_bytes())?;
            key.bytes(import.alias.as_bytes())?;
            key.number(import.line)?;
            key.number(import.column)?;
        }
    }
    Some(key.0)
}

struct Key(Vec<u8>);

impl Key {
    fn number(&mut self, value: usize) -> Option<()> {
        self.append(&u64::try_from(value).ok()?.to_le_bytes())
    }

    fn bytes(&mut self, bytes: &[u8]) -> Option<()> {
        self.number(bytes.len())?;
        self.append(bytes)
    }

    fn path(&mut self, path: &Path) -> Option<()> {
        // The cache is local to this compiler build and platform. Preserve OS
        // path bytes, including non-UTF-8 paths, rather than a display string.
        self.bytes(path.as_os_str().as_encoded_bytes())
    }

    fn append(&mut self, bytes: &[u8]) -> Option<()> {
        if self.0.len().checked_add(bytes.len())? > MAX_KEY_BYTES {
            return None;
        }
        self.0.extend_from_slice(bytes);
        Some(())
    }
}

fn entry_name(key: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut name = String::with_capacity(70);
    for byte in digest(key) {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".cache");
    name
}

fn decode(bytes: &[u8], expected_key: &[u8]) -> Option<String> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_ENTRY_BYTES || !bytes.starts_with(MAGIC) {
        return None;
    }
    let mut offset = MAGIC.len();
    let key_bytes = read_length(bytes, &mut offset)?;
    let c_bytes = read_length(bytes, &mut offset)?;
    if key_bytes > MAX_KEY_BYTES || c_bytes > MAX_C_BYTES || key_bytes != expected_key.len() {
        return None;
    }
    let checksum = bytes.get(offset..offset.checked_add(32)?)?;
    offset += 32;
    let key_end = offset.checked_add(key_bytes)?;
    let c_end = key_end.checked_add(c_bytes)?;
    if c_end != bytes.len() || bytes.get(offset..key_end)? != expected_key {
        return None;
    }
    let c = bytes.get(key_end..c_end)?;
    if digest(c) != checksum {
        return None;
    }
    String::from_utf8(c.to_vec()).ok()
}

fn read_length(bytes: &[u8], offset: &mut usize) -> Option<usize> {
    let end = offset.checked_add(8)?;
    let value = u64::from_le_bytes(bytes.get(*offset..end)?.try_into().ok()?);
    *offset = end;
    usize::try_from(value).ok()
}

// Directory handles keep temporary writes and publication inside the opened
// cache even if a parent is renamed. Each component and entry is opened without
// following symlinks. Other platforms conservatively bypass this optional cache
// until they have equivalent directory-relative, no-follow filesystem support.
#[cfg(unix)]
fn cache_directory(path: &Path, create: bool) -> Option<File> {
    use std::os::fd::FromRawFd;
    use std::path::Component;

    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => names.push(name),
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    if names.is_empty() {
        return None;
    }
    let root = std::ffi::CString::new("/").ok()?;
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return None;
    }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    for name in names {
        directory = child_directory(&directory, name, create)?;
    }
    if !private_owner(&directory) {
        return None;
    }
    let directory = child_directory(&directory, std::ffi::OsStr::new(NAMESPACE), create)?;
    if !private_owner(&directory) {
        return None;
    }
    Some(directory)
}

#[cfg(unix)]
fn child_directory(parent: &File, name: &std::ffi::OsStr, create: bool) -> Option<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let name = std::ffi::CString::new(name.as_bytes()).ok()?;
    let open = || unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let mut fd = open();
    if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if created < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return None;
        }
        fd = open();
    }
    if fd < 0 {
        return None;
    }
    Some(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn private_owner(file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;

    file.metadata().is_ok_and(|metadata| {
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0
    })
}

#[cfg(unix)]
fn open_entry(directory: &File, name: &str) -> Option<File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = std::ffi::CString::new(name).ok()?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return None;
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata().ok()?.is_file() || !private_owner(&file) {
        return None;
    }
    Some(file)
}

#[cfg(unix)]
fn replaceable_entry(directory: &File, name: &str) -> bool {
    use std::os::fd::AsRawFd;

    let Ok(c_name) = std::ffi::CString::new(name) else {
        return false;
    };
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    let found = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            c_name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found < 0 {
        return std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound;
    }
    let Some(mut existing) = open_entry(directory, name) else {
        return false;
    };
    let mut magic = vec![0; MAGIC.len()];
    existing.read_exact(&mut magic).is_ok() && magic == MAGIC
}

#[cfg(unix)]
fn temporary_entry(directory: &File) -> Option<(String, File)> {
    use std::os::fd::{AsRawFd, FromRawFd};

    for _ in 0..32 {
        let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let name = format!(".tmp-{}-{sequence}", std::process::id());
        let c_name = std::ffi::CString::new(name.as_str()).ok()?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd >= 0 {
            return Some((name, unsafe { File::from_raw_fd(fd) }));
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
            return None;
        }
    }
    None
}

#[cfg(unix)]
fn publish(directory: &File, temporary: &str, name: &str) {
    use std::os::fd::AsRawFd;

    let (Ok(temporary), Ok(name)) = (
        std::ffi::CString::new(temporary),
        std::ffi::CString::new(name),
    ) else {
        return;
    };
    let published = unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
        )
    };
    if published == 0 {
        // A crash may lose this optional entry, but must never publish a partial
        // entry. Sync the directory where supported without making it required.
        let _ = directory.sync_all();
    }
}

#[cfg(unix)]
fn remove_temporary(directory: &File, name: &str) {
    use std::os::fd::AsRawFd;

    if let Ok(name) = std::ffi::CString::new(name) {
        unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    }
}

#[cfg(not(unix))]
fn cache_directory(_path: &Path, _create: bool) -> Option<File> {
    None
}

#[cfg(not(unix))]
fn open_entry(_directory: &File, _name: &str) -> Option<File> {
    None
}

#[cfg(not(unix))]
fn replaceable_entry(_directory: &File, _name: &str) -> bool {
    false
}

#[cfg(not(unix))]
fn temporary_entry(_directory: &File) -> Option<(String, File)> {
    None
}

#[cfg(not(unix))]
fn publish(_directory: &File, _temporary: &str, _name: &str) {}

#[cfg(not(unix))]
fn remove_temporary(_directory: &File, _name: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    use rill::modules::{Import, Module};
    use std::collections::BTreeMap;

    fn graph() -> ModuleGraph {
        ModuleGraph {
            root: PathBuf::from("/project"),
            modules: BTreeMap::from([(
                "@entry".into(),
                Module {
                    name: "@entry".into(),
                    path: PathBuf::from("/project/main.rill"),
                    source: "fn main() { 42 }".into(),
                    imports: vec![Import {
                        module: "helper".into(),
                        alias: "h".into(),
                        line: 1,
                        column: 1,
                    }],
                },
            )]),
        }
    }

    fn encoded(key: &[u8], c: &[u8]) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&(c.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&digest(c));
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(c);
        bytes
    }

    #[test]
    fn key_records_exact_sources_paths_imports_and_compiler() {
        let original = graph();
        let key = source_key(&original, "compiler-a").unwrap();
        assert_ne!(key, source_key(&original, "compiler-b").unwrap());
        let mut changed = original.clone();
        changed.root.push("different");
        assert_ne!(key, source_key(&changed, "compiler-a").unwrap());
        for field in 0..8 {
            let mut changed = original.clone();
            let module = changed.modules.get_mut("@entry").unwrap();
            match field {
                0 => module.name.push('x'),
                1 => module.path.push("x"),
                2 => module.source = "fn main() { 43 }".into(),
                3 => module.imports[0].module.push('x'),
                4 => module.imports[0].alias.push('x'),
                5 => module.imports[0].line += 1,
                6 => module.imports[0].column += 1,
                7 => module.imports.clear(),
                _ => unreachable!(),
            }
            assert_ne!(key, source_key(&changed, "compiler-a").unwrap());
        }
        let mut changed = original;
        let module = changed.modules.remove("@entry").unwrap();
        changed.modules.insert("other".into(), module);
        assert_ne!(key, source_key(&changed, "compiler-a").unwrap());
    }

    #[test]
    fn decoder_rejects_corruption_truncation_trailing_data_and_wrong_keys() {
        let key = source_key(&graph(), "compiler").unwrap();
        let bytes = encoded(&key, b"int main(void) { return 42; }\n");
        assert!(decode(&bytes, &key).is_some());
        assert!(decode(&bytes, b"different key").is_none());
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length], &key).is_none());
        }
        for offset in [
            0,
            MAGIC.len(),
            MAGIC.len() + 8,
            HEADER_BYTES - 1,
            HEADER_BYTES,
            bytes.len() - 1,
        ] {
            let mut changed = bytes.clone();
            changed[offset] ^= 1;
            assert!(decode(&changed, &key).is_none());
        }
        let mut changed = bytes.clone();
        changed.push(0);
        assert!(decode(&changed, &key).is_none());
        let mut changed = bytes;
        changed[MAGIC.len()..MAGIC.len() + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&changed, &key).is_none());
        assert!(decode(&encoded(&key, &[0xff]), &key).is_none());
    }

    #[test]
    fn key_encoding_is_bounded_and_unambiguous() {
        let mut key = Key(Vec::new());
        key.bytes(b"ab").unwrap();
        key.bytes(b"c").unwrap();
        let mut other = Key(Vec::new());
        other.bytes(b"a").unwrap();
        other.bytes(b"bc").unwrap();
        assert_ne!(key.0, other.0);
        let mut full = Key(vec![0; MAX_KEY_BYTES - 1]);
        assert!(full.append(&[1]).is_some());
        assert!(full.append(&[1]).is_none());
    }

    #[cfg(unix)]
    mod filesystem {
        use super::*;
        use std::fs;
        use std::os::unix::fs::{symlink, PermissionsExt};

        struct Workspace(PathBuf);

        impl Workspace {
            fn new() -> Self {
                let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "rill-build-cache-{}-{sequence}",
                    std::process::id()
                ));
                fs::create_dir(&path).unwrap();
                Self(fs::canonicalize(path).unwrap())
            }

            fn cache(&self) -> Cache {
                Cache::new(self.0.join("build/cache"))
            }

            fn entry(&self) -> PathBuf {
                let key = source_key(&graph(), env!("RILL_COMPILER_ID")).unwrap();
                self.0
                    .join("build/cache")
                    .join(NAMESPACE)
                    .join(entry_name(&key))
            }
        }

        impl Drop for Workspace {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }

        #[test]
        fn round_trip_misses_for_changed_graph_and_recovers_corrupt_entry() {
            let workspace = Workspace::new();
            let cache = workspace.cache();
            assert!(cache.lookup(&graph()).is_none());
            cache.store(&graph(), "checked C");
            assert_eq!(cache.lookup(&graph()).as_deref(), Some("checked C"));
            let mut changed = graph();
            changed.modules.get_mut("@entry").unwrap().source.push(' ');
            assert!(cache.lookup(&changed).is_none());
            fs::write(workspace.entry(), MAGIC).unwrap();
            assert!(cache.lookup(&graph()).is_none());
            cache.store(&graph(), "repaired C");
            assert_eq!(cache.lookup(&graph()).as_deref(), Some("repaired C"));
        }

        #[test]
        fn symlink_ancestry_and_entry_are_neither_read_nor_written_through() {
            let workspace = Workspace::new();
            let outside = workspace.0.join("outside");
            fs::create_dir(&outside).unwrap();
            symlink(&outside, workspace.0.join("alias")).unwrap();
            let cache = Cache::new(workspace.0.join("alias/cache"));
            cache.store(&graph(), "C");
            assert!(cache.lookup(&graph()).is_none());
            assert!(!outside.join("cache").exists());

            let cache = workspace.cache();
            cache.store(&graph(), "original");
            let entry = workspace.entry();
            let original = fs::read(&entry).unwrap();
            let target = outside.join("entry");
            fs::write(&target, &original).unwrap();
            fs::remove_file(&entry).unwrap();
            symlink(&target, &entry).unwrap();
            assert!(cache.lookup(&graph()).is_none());
            cache.store(&graph(), "replacement");
            assert!(fs::symlink_metadata(&entry)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::read(target).unwrap(), original);
        }

        #[test]
        fn unknown_files_and_writable_entries_are_preserved() {
            let workspace = Workspace::new();
            let cache = workspace.cache();
            cache.store(&graph(), "original");
            let entry = workspace.entry();
            fs::write(&entry, b"unrelated file").unwrap();
            cache.store(&graph(), "replacement");
            assert_eq!(fs::read(&entry).unwrap(), b"unrelated file");
            assert!(cache.lookup(&graph()).is_none());

            let key = source_key(&graph(), env!("RILL_COMPILER_ID")).unwrap();
            let original = encoded(&key, b"original");
            fs::write(&entry, &original).unwrap();
            fs::set_permissions(&entry, fs::Permissions::from_mode(0o666)).unwrap();
            cache.store(&graph(), "replacement");
            assert!(cache.lookup(&graph()).is_none());
            assert_eq!(fs::read(entry).unwrap(), original);
        }

        #[test]
        fn writable_cache_directories_are_bypassed() {
            let workspace = Workspace::new();
            let cache = workspace.cache();
            cache.store(&graph(), "original");
            let path = workspace.0.join("build/cache");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o777)).unwrap();
            assert!(cache.lookup(&graph()).is_none());
            cache.store(&graph(), "replacement");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(cache.lookup(&graph()).as_deref(), Some("original"));
        }

        #[test]
        fn concurrent_publication_leaves_one_complete_entry() {
            let workspace = Workspace::new();
            let directory = workspace.0.join("build/cache");
            let workers: Vec<_> = (0..8)
                .map(|index| {
                    let directory = directory.clone();
                    std::thread::spawn(move || {
                        Cache::new(directory).store(&graph(), &format!("checked C {index}"));
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
            let c = workspace.cache().lookup(&graph()).unwrap();
            assert!((0..8).any(|index| c == format!("checked C {index}")));
            let entries: Vec<_> = fs::read_dir(directory.join(NAMESPACE)).unwrap().collect();
            assert_eq!(entries.len(), 1);
        }

        #[test]
        fn source_paths_keep_non_utf8_identity() {
            use std::os::unix::ffi::OsStringExt;

            let mut first = graph();
            let mut second = graph();
            first.modules.get_mut("@entry").unwrap().path =
                std::ffi::OsString::from_vec(vec![b'/', 0xfe]).into();
            second.modules.get_mut("@entry").unwrap().path =
                std::ffi::OsString::from_vec(vec![b'/', 0xff]).into();
            assert_ne!(
                source_key(&first, "compiler"),
                source_key(&second, "compiler")
            );
        }
    }
}
