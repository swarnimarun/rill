//! Portable documentation generation. Project source is parsed, never executed.
mod content;
mod render;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const MARKER: &str = ".rill-docs.json";
const MAX_FILES: usize = 2048;
const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const USAGE: &str = "usage: rill docs [project-directory] [-o <directory>] [--language] [--open]";

pub(super) struct Page {
    pub path: String,
    pub title: String,
    pub section: String,
    pub source: String,
    pub markdown: String,
    pub items: Vec<ApiItem>,
}

pub(super) struct ApiItem {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub markdown: String,
}

pub(super) struct Site {
    pub title: String,
    pub pages: Vec<Page>,
}

struct Options {
    project: Option<PathBuf>,
    output: Option<PathBuf>,
    language: bool,
    open: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self {
            project: None,
            output: None,
            language: false,
            open: false,
        };
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("-o" | "--output") if options.output.is_none() => {
                    let value = args.next().ok_or("E-DOCS-ARGS: -o requires a directory")?;
                    if value.is_empty() {
                        return Err("E-DOCS-ARGS: output directory must not be empty".into());
                    }
                    options.output = Some(value.into());
                }
                Some("--language") if !options.language => options.language = true,
                Some("--open") if !options.open => options.open = true,
                _ if !arg.to_string_lossy().starts_with('-') && options.project.is_none() => {
                    options.project = Some(arg.into());
                }
                _ => {
                    return Err(format!(
                        "E-DOCS-ARGS: invalid or repeated option {arg:?}\n{USAGE}"
                    ))
                }
            }
        }
        if options.language && options.project.is_some() {
            return Err(
                "E-DOCS-ARGS: --language cannot be combined with a project directory".into(),
            );
        }
        Ok(options)
    }
}

pub(super) fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args: Vec<_> = args.collect();
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h")) {
        println!("{USAGE}\nGenerate offline language, project Markdown and source API documentation.\nDefault output: build/docs. Use --language to omit the current project.");
        return Ok(());
    }
    let options = Options::parse(args.into_iter())?;
    let current = std::env::current_dir().map_err(|e| format!("E-DOCS-IO: {e}"))?;
    let project = if let Some(path) = options.project {
        Some(crate::project::Project::load(&path)?)
    } else if !options.language && current.join("rill.json").try_exists().map_err(io_error)? {
        Some(crate::project::Project::load(&current)?)
    } else {
        None
    };
    let base = project
        .as_ref()
        .map_or(current.as_path(), |p| p.root.as_path());
    let output = options
        .output
        .unwrap_or_else(|| PathBuf::from("build/docs"));
    let site = content::collect(project.as_ref())?;
    let files = render::render(&site)?;
    let destination = publish(base, &output, files)?;
    let index = destination.join("index.html");
    println!("Documentation: {}", index.display());
    if options.open {
        open(&index)?;
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> String {
    format!("E-DOCS-IO: {error}")
}

fn relative_file(name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.len() > 1024
        || name.contains(['\\', ':'])
        || name.split('/').any(|part| matches!(part, "" | "." | ".."))
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("E-DOCS-PATH: invalid generated path {name:?}"));
    }
    Ok(PathBuf::from(name))
}

fn destination(base: &Path, output: &Path) -> Result<PathBuf, String> {
    let joined = if output.is_absolute() {
        output.to_owned()
    } else {
        base.join(output)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => (),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err("E-DOCS-PATH: output escapes its filesystem root".into());
                }
            }
            component => normalized.push(component),
        }
    }
    let name = normalized
        .file_name()
        .ok_or("E-DOCS-PATH: output must name a directory")?;
    let parent = normalized
        .parent()
        .ok_or("E-DOCS-PATH: output needs a parent")?;
    // Resolve parent aliases once, including ordinary host temporary paths.
    // Publication below then uses that canonical parent, not a mutable alias.
    fs::create_dir_all(parent).map_err(io_error)?;
    let output = fs::canonicalize(parent).map_err(io_error)?.join(name);
    if base.starts_with(&output) {
        return Err("E-DOCS-PATH: output cannot replace the project or its ancestors".into());
    }
    Ok(output)
}

fn publish(
    base: &Path,
    output: &Path,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<PathBuf, String> {
    if !files.contains_key("index.html") || files.contains_key(MARKER) || files.len() > MAX_FILES {
        return Err("E-DOCS-OUTPUT: invalid generated file catalogue".into());
    }
    let mut total = 0_usize;
    for (path, contents) in &files {
        relative_file(path)?;
        total = total
            .checked_add(contents.len())
            .ok_or("E-DOCS-LIMIT: generated output size")?;
        if total > MAX_OUTPUT_BYTES {
            return Err("E-DOCS-LIMIT: generated output exceeds 128 MiB".into());
        }
    }
    let output = destination(base, output)?;
    let exists = match fs::symlink_metadata(&output) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("E-DOCS-OUTPUT: output must be a real generated directory".into());
            }
            owned_tree(&output)?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error(error)),
    };
    let stage = Staging::create(output.parent().expect("canonical parent"))?;
    for (name, contents) in &files {
        let path = stage.0.join(name);
        fs::create_dir_all(path.parent().expect("generated relative file")).map_err(io_error)?;
        write_new(&path, contents)?;
    }
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1,
        "files": files.keys().collect::<Vec<_>>()
    }))
    .map_err(|e| format!("E-DOCS-OUTPUT: {e}"))?;
    if manifest.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("E-DOCS-LIMIT: generated manifest exceeds 4 MiB".into());
    }
    write_new(&stage.0.join(MARKER), &manifest)?;
    if exists {
        // Stage and backup are siblings, so replacement never crosses devices.
        let backup = Staging::create(output.parent().unwrap())?;
        fs::remove_dir(&backup.0).map_err(io_error)?;
        fs::rename(&output, &backup.0).map_err(io_error)?;
        if let Err(error) = fs::rename(&stage.0, &output) {
            if let Err(restore) = fs::rename(&backup.0, &output) {
                // Keep the only old copy if a filesystem failure prevents rollback.
                let saved = backup.0.clone();
                std::mem::forget(backup);
                return Err(format!("E-DOCS-OUTPUT: publish failed: {error}; restore failed: {restore}; previous docs retained at {}", saved.display()));
            }
            return Err(io_error(error));
        }
    } else {
        fs::rename(&stage.0, &output).map_err(io_error)?;
    }
    Ok(output)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(io_error)
}

fn owned_tree(root: &Path) -> Result<(), String> {
    let marker = root.join(MARKER);
    let metadata = fs::symlink_metadata(&marker).map_err(|_| {
        "E-DOCS-OUTPUT: existing directory is not owned by rill docs; choose another -o".to_owned()
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("E-DOCS-OUTPUT: invalid documentation ownership manifest".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(&marker)
        .map_err(io_error)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("E-DOCS-LIMIT: ownership manifest exceeds 4 MiB".into());
    }
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("E-DOCS-OUTPUT: invalid ownership manifest: {e}"))?;
    if manifest.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return Err("E-DOCS-OUTPUT: unsupported ownership manifest version".into());
    }
    let names = manifest
        .get("files")
        .and_then(|v| v.as_array())
        .filter(|v| !v.is_empty() && v.len() <= MAX_FILES)
        .ok_or("E-DOCS-OUTPUT: invalid ownership file list")?;
    let mut expected = BTreeSet::from([PathBuf::from(MARKER)]);
    for name in names {
        let name = name
            .as_str()
            .ok_or("E-DOCS-OUTPUT: invalid ownership file name")?;
        let path = relative_file(name)?;
        if !expected.insert(path) {
            return Err("E-DOCS-OUTPUT: duplicate ownership file name".into());
        }
    }
    if !expected.contains(Path::new("index.html")) {
        return Err("E-DOCS-OUTPUT: ownership manifest lacks an index page".into());
    }
    // Missing generated files can be recreated. Every file that is present
    // still has to be listed; user additions and symlinks always reject.
    inspect_tree(root, Path::new(""), &expected, 0)
}

fn inspect_tree(
    root: &Path,
    relative: &Path,
    expected: &BTreeSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("E-DOCS-LIMIT: output directory depth exceeds 64".into());
    }
    for entry in fs::read_dir(root.join(relative)).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = relative.join(entry.file_name());
        let ty = entry.file_type().map_err(io_error)?;
        if ty.is_symlink() {
            return Err("E-DOCS-OUTPUT: generated directory contains a symlink".into());
        }
        if ty.is_dir() && expected.iter().any(|name| name.starts_with(&path)) {
            inspect_tree(root, &path, expected, depth + 1)?;
        } else if ty.is_file() && expected.contains(&path) {
            continue;
        } else {
            return Err(format!(
                "E-DOCS-OUTPUT: refusing to replace untracked output {}",
                path.display()
            ));
        }
    }
    Ok(())
}

struct Staging(PathBuf);
impl Staging {
    fn create(parent: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = parent.join(format!(
                ".rill-docs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(e)),
            }
        }
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn open(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    let status = command
        .arg(path)
        .status()
        .map_err(|e| format!("E-DOCS-OPEN: {e}"))?;
    if !status.success() {
        return Err(format!(
            "E-DOCS-OPEN: opener failed with {status}; open {} manually",
            path.display()
        ));
    }
    Ok(())
}
