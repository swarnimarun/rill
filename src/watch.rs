//! Development rebuild/restart. Executables never share live state.
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

mod process;

const MAX_FILES: usize = 4096;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

struct Options {
    source: PathBuf,
    compiler_args: Vec<OsString>,
    paths: Vec<PathBuf>,
    interval: Duration,
    once: bool,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let source = PathBuf::from(args.next().ok_or("E-WATCH: expected source path")?);
        let mut compiler_args = Vec::new();
        let mut paths = Vec::new();
        let mut interval = None;
        let mut once = false;
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--interval-ms") if interval.is_none() => {
                    interval = Some(
                        args.next()
                            .and_then(|value| value.to_str().and_then(|s| s.parse::<u64>().ok()))
                            .filter(|value| (10..=60_000).contains(value))
                            .ok_or("E-WATCH: --interval-ms requires 10..60000")?,
                    );
                }
                Some("--watch") => paths.push(PathBuf::from(
                    args.next().ok_or("E-WATCH: --watch requires a path")?,
                )),
                Some("--once") if !once => once = true,
                // Consume compiler option values here so a path or flag value
                // named --watch is never reinterpreted as a watcher option.
                Some(
                    "--cc" | "--c-arg" | "--link-arg" | "--opt-level" | "--simd"
                    | "--native-config" | "--timeout-ms",
                ) => {
                    let value = args.next().ok_or_else(|| {
                        format!("E-WATCH: {} requires a value", arg.to_string_lossy())
                    })?;
                    compiler_args.extend([arg, value]);
                }
                Some("--zig-cc") => compiler_args.push(arg),
                _ => return Err(format!("E-WATCH: unsupported option {arg:?}")),
            }
        }
        // Reuse the real driver validation before starting any compiler.
        let mut check_args = compiler_args.clone();
        check_args.extend([OsString::from("-o"), OsString::from("unused")]);
        super::BuildOptions::parse("build", check_args.into_iter())?;
        Ok(Self {
            source,
            compiler_args,
            paths,
            interval: Duration::from_millis(interval.unwrap_or(200)),
            once,
        })
    }
}

/// Watch local sources and explicitly named inputs, rebuilding through the
/// ordinary compiler. Failed candidates leave the previous process running.
pub fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let options = Options::parse(args)?;
    let directory = super::RunDirectory::create()?;
    let compiler = std::env::current_exe().map_err(|e| format!("E-WATCH: {e}"))?;
    let mut inputs = Inputs::new(&options.source, &options.paths)?;
    let mut snapshot = inputs.initial.take().expect("initial watch snapshot");
    let mut supervisor = process::Supervisor::new()?;
    let mut generation = 0_u64;
    let build = |generation: u64| -> Result<Option<PathBuf>, String> {
        let binary = directory.0.join(format!("generation-{generation}"));
        let status = process::status(
            Command::new(&compiler)
                .arg("build")
                .arg(&options.source)
                .args(&options.compiler_args)
                .arg("-o")
                .arg(&binary),
        )?;
        if status.success() {
            Ok(Some(binary))
        } else {
            let _ = fs::remove_file(binary);
            eprintln!("E-WATCH-BUILD: candidate rejected; waiting for source changes");
            Ok(None)
        }
    };
    let mut previous_binary = None;
    loop {
        if process::cancelled() {
            return Err("E-WATCH: cancelled".into());
        }
        if let Some(binary) = build(generation)? {
            supervisor.spawn(&mut Command::new(&binary))?;
            if let Some(previous) = previous_binary.take() {
                let _ = fs::remove_file(previous);
            }
            previous_binary = Some(binary);
            eprintln!("rill watch: started generation {generation}");
        } else if options.once {
            return Err("E-WATCH-BUILD: initial build failed".into());
        }
        if options.once {
            let status = loop {
                if process::cancelled() {
                    return Err("E-WATCH: cancelled".into());
                }
                if let Some(status) = supervisor.poll()? {
                    break status;
                }
                thread::sleep(Duration::from_millis(20));
            };
            return if status.success() {
                Ok(())
            } else {
                Err(format!("E-WATCH: program failed with {status}"))
            };
        }
        loop {
            pause(options.interval);
            if process::cancelled() {
                return Err("E-WATCH: cancelled".into());
            }
            if let Some(status) = supervisor.poll()? {
                eprintln!("rill watch: program exited with {status}");
            }
            match inputs.read(&snapshot) {
                Ok(next) if next != snapshot => {
                    snapshot = next;
                    generation = generation
                        .checked_add(1)
                        .ok_or("E-WATCH: generation limit")?;
                    break;
                }
                Ok(_) => (),
                Err(error) => {
                    // An editor can briefly remove/replace a file while saving.
                    // Keep the last observed contents and retry next interval.
                    eprintln!("{error}");
                }
            }
        }
    }
}

fn pause(duration: Duration) {
    let deadline = Instant::now() + duration;
    while !process::cancelled() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

fn snapshot(paths: &[PathBuf]) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut bytes = 0;
    for path in paths {
        visit(path, true, 0, &mut files, &mut bytes)?;
    }
    Ok(files)
}

struct Inputs {
    entry: PathBuf,
    extras: Vec<PathBuf>,
    sources: Vec<PathBuf>,
    aliases: Vec<(PathBuf, PathBuf)>,
    broken_imports: bool,
    initial: Option<BTreeMap<PathBuf, Vec<u8>>>,
}

impl Inputs {
    fn new(entry: &Path, extras: &[PathBuf]) -> Result<Self, String> {
        let (initial, broken_imports, aliases) = source_snapshot(entry, extras)?;
        Ok(Self {
            entry: entry.into(),
            extras: extras.into(),
            sources: initial.keys().cloned().collect(),
            aliases,
            broken_imports,
            initial: Some(initial),
        })
    }

    fn read(
        &mut self,
        previous: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        let identities_unchanged = self.aliases.iter().all(|(lexical, resolved)| {
            fs::canonicalize(lexical).is_ok_and(|path| path == *resolved)
        });
        if self.broken_imports || !identities_unchanged {
            return self.discover();
        }
        let mut paths = self.extras.clone();
        paths.extend(self.sources.iter().cloned());
        let next = snapshot(&paths)?;
        if &next == previous {
            return Ok(next);
        }
        // Re-discover imports only when an observed input changes. Unchanged
        // polling does no lexing, parsing, name resolution or type checking.
        self.discover()
    }

    fn discover(&mut self) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        let (next, broken, aliases) = source_snapshot(&self.entry, &self.extras)?;
        self.sources = next.keys().cloned().collect();
        self.broken_imports = broken;
        self.aliases = aliases;
        Ok(next)
    }
}

fn source_snapshot(entry: &Path, extra_paths: &[PathBuf]) -> Result<Discovery, String> {
    let mut files = snapshot(extra_paths)?;
    let mut bytes = files.values().map(Vec::len).sum::<usize>();
    let mut broken = false;
    let mut aliases = Vec::new();
    match rill::modules::load(entry) {
        Ok(graph) => {
            // Discovery retains the exact source bytes and only follows imports.
            // Bundled sources are fixed in this compiler executable.
            for module in graph.modules.values() {
                if module.name.starts_with("std.") {
                    continue;
                }
                let lexical = if module.name == "@entry" {
                    entry.to_owned()
                } else {
                    graph
                        .root
                        .join(module.name.replace('.', "/"))
                        .with_extension("rill")
                };
                aliases.push((lexical, module.path.clone()));
                if files.contains_key(&module.path) {
                    continue;
                }
                bytes += module.source.len();
                if files.len() >= MAX_FILES || bytes > MAX_TOTAL_BYTES {
                    return Err("E-WATCH-LIMIT: source graph exceeds watch budget".into());
                }
                files.insert(module.path.clone(), module.source.as_bytes().to_vec());
            }
        }
        Err(_) => {
            broken = true;
            // A new import may name a file that the editor has not created yet.
            // Until discovery succeeds again, observe the local tree so adding
            // that missing file repairs the build without restarting watch.
            let root = entry
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            visit(root, true, 0, &mut files, &mut bytes)?;
        }
    }
    Ok((files, broken, aliases))
}

type Discovery = (BTreeMap<PathBuf, Vec<u8>>, bool, Vec<(PathBuf, PathBuf)>);

fn visit(
    path: &Path,
    explicit: bool,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    bytes: &mut usize,
) -> Result<(), String> {
    let failure = |error| format!("E-WATCH-IO: {}: {error}", path.display());
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failure(error)),
    };
    if metadata.file_type().is_symlink() {
        return if explicit {
            Err(format!(
                "E-WATCH-PATH: symlink watch input {}",
                path.display()
            ))
        } else {
            Ok(())
        };
    }
    if metadata.is_dir() {
        if depth >= 64 {
            return Err("E-WATCH-LIMIT: directory depth exceeds 64".into());
        }
        for entry in fs::read_dir(path).map_err(failure)? {
            let entry = entry.map_err(failure)?;
            let name = entry.file_name();
            if name.to_str().is_some_and(|name| {
                name.starts_with('.') || matches!(name, "target" | "build" | "node_modules")
            }) {
                continue;
            }
            visit(&entry.path(), false, depth + 1, files, bytes)?;
        }
    } else if metadata.is_file()
        && (explicit
            || path.file_name() == Some(OsStr::new("rill.json"))
            || matches!(
                path.extension().and_then(OsStr::to_str),
                Some("rill" | "rillcfg" | "h" | "c")
            ))
    {
        let path = fs::canonicalize(path).map_err(failure)?;
        if files.contains_key(&path) {
            return Ok(());
        }
        if files.len() >= MAX_FILES {
            return Err("E-WATCH-LIMIT: more than 4096 input files".into());
        }
        let mut contents = Vec::new();
        fs::File::open(&path)
            .map_err(failure)?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut contents)
            .map_err(failure)?;
        *bytes += contents.len();
        if contents.len() as u64 > MAX_FILE_BYTES || *bytes > MAX_TOTAL_BYTES {
            return Err("E-WATCH-LIMIT: input exceeds 1 MiB/file or 64 MiB total".into());
        }
        files.insert(path, contents);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_changes_additions_deletions_and_explicit_assets_are_detected() {
        let directory = super::super::RunDirectory::create().unwrap();
        let source = directory.0.join("main.rill");
        let asset = directory.0.join("data.txt");
        fs::write(&source, "fn main() { 1 }").unwrap();
        fs::write(&asset, "a").unwrap();
        let paths = [directory.0.clone(), asset.clone()];
        let first = snapshot(&paths).unwrap();
        fs::write(&source, "fn main() { 2 }").unwrap();
        assert_ne!(first, snapshot(&paths).unwrap());
        let second = snapshot(&paths).unwrap();
        fs::write(directory.0.join("module.rill"), "").unwrap();
        assert_ne!(second, snapshot(&paths).unwrap());
        let third = snapshot(&paths).unwrap();
        fs::remove_file(&source).unwrap();
        assert_ne!(third, snapshot(&paths).unwrap());
        let fourth = snapshot(&paths).unwrap();
        fs::write(asset, "b").unwrap();
        assert_ne!(fourth, snapshot(&paths).unwrap());
        fs::create_dir(directory.0.join("build")).unwrap();
        let before = snapshot(&paths).unwrap();
        fs::write(directory.0.join("build/generated.c"), "ignored").unwrap();
        assert_eq!(before, snapshot(&paths).unwrap());
    }

    #[test]
    fn compiler_values_are_not_watcher_options() {
        let options = Options::parse(
            ["main.rill", "--link-arg", "--watch", "--once"]
                .map(OsString::from)
                .into_iter(),
        )
        .unwrap();
        assert_eq!(options.compiler_args, ["--link-arg", "--watch"]);
        assert!(options.once);
    }

    #[test]
    fn source_graph_ignores_unused_libraries_and_repairs_missing_imports() {
        let directory = super::super::RunDirectory::create().unwrap();
        let entry = directory.0.join("main.rill");
        let imported = directory.0.join("used.rill");
        let unused = directory.0.join("unused.rill");
        fs::write(&entry, "import used; fn main() { used.answer() }").unwrap();
        fs::write(&imported, "export fn answer() { 1 }").unwrap();
        fs::write(&unused, "invalid unused library").unwrap();
        let first = source_snapshot(&entry, &[]).unwrap().0;
        assert_eq!(first.len(), 2);
        fs::write(&unused, "another invalid unused library").unwrap();
        assert_eq!(first, source_snapshot(&entry, &[]).unwrap().0);
        fs::write(&imported, "export fn answer() { 2 }").unwrap();
        assert_ne!(first, source_snapshot(&entry, &[]).unwrap().0);
        fs::write(&entry, "import missing; fn main() { missing.answer() }").unwrap();
        let broken = source_snapshot(&entry, &[]).unwrap().0;
        fs::write(directory.0.join("missing.rill"), "export fn answer() { 3 }").unwrap();
        let repaired = source_snapshot(&entry, &[]).unwrap().0;
        assert_ne!(broken, repaired);
        assert_eq!(repaired.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_entry_and_import_symlinks_invalidate_known_inputs() {
        use std::os::unix::fs::symlink;
        let directory = super::super::RunDirectory::create().unwrap();
        let entry = directory.0.join("main.rill");
        let first = directory.0.join("first.txt");
        let second = directory.0.join("second.txt");
        fs::write(&first, "fn main() { 1 }").unwrap();
        fs::write(&second, "fn main() { 2 }").unwrap();
        symlink(&first, &entry).unwrap();
        let mut inputs = Inputs::new(&entry, &[]).unwrap();
        let previous = inputs.initial.take().unwrap();
        fs::remove_file(&entry).unwrap();
        symlink(&second, &entry).unwrap();
        assert_ne!(inputs.read(&previous).unwrap(), previous);

        fs::remove_file(&entry).unwrap();
        fs::write(&entry, "fn main() { 1 }").unwrap();
        let mut inputs = Inputs::new(&entry, &[]).unwrap();
        let previous = inputs.initial.take().unwrap();
        fs::remove_file(&entry).unwrap();
        symlink(&second, &entry).unwrap();
        assert_ne!(inputs.read(&previous).unwrap(), previous);

        fs::remove_file(&entry).unwrap();
        fs::write(&entry, "import value; fn main() { value.answer() }").unwrap();
        fs::write(&first, "export fn answer() { 1 }").unwrap();
        fs::write(&second, "export fn answer() { 2 }").unwrap();
        let imported = directory.0.join("value.rill");
        symlink(&first, &imported).unwrap();
        let mut inputs = Inputs::new(&entry, &[]).unwrap();
        let previous = inputs.initial.take().unwrap();
        fs::remove_file(&imported).unwrap();
        symlink(&second, &imported).unwrap();
        assert_ne!(inputs.read(&previous).unwrap(), previous);

        fs::remove_file(&imported).unwrap();
        fs::write(&imported, "export fn answer() { 1 }").unwrap();
        let mut inputs = Inputs::new(&entry, &[]).unwrap();
        let previous = inputs.initial.take().unwrap();
        fs::remove_file(&imported).unwrap();
        symlink(&second, &imported).unwrap();
        assert_ne!(inputs.read(&previous).unwrap(), previous);
    }
}
