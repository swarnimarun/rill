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
    project_root: Option<PathBuf>,
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
                    | "--native-config" | "--timeout-ms" | "--cache-dir" | "--project-root",
                ) => {
                    let value = args.next().ok_or_else(|| {
                        format!("E-WATCH: {} requires a value", arg.to_string_lossy())
                    })?;
                    compiler_args.extend([arg, value]);
                }
                Some("--zig-cc" | "--incremental" | "--no-incremental" | "--cache-status") => {
                    compiler_args.push(arg)
                }
                _ => return Err(format!("E-WATCH: unsupported option {arg:?}")),
            }
        }
        // Reuse the real driver validation before starting any compiler.
        let mut check_args = compiler_args.clone();
        check_args.extend([OsString::from("-o"), OsString::from("unused")]);
        let checked = super::BuildOptions::parse("build", check_args.into_iter())?;
        Ok(Self {
            source,
            compiler_args,
            paths,
            interval: Duration::from_millis(interval.unwrap_or(200)),
            once,
            project_root: checked.project_root,
        })
    }
}

/// Watch local sources and explicitly named inputs, rebuilding through the
/// ordinary compiler. Failed candidates leave the previous process running.
pub fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let options = Options::parse(args)?;
    let directory = super::RunDirectory::create()?;
    let compiler = std::env::current_exe().map_err(|e| format!("E-WATCH: {e}"))?;
    let mut inputs = Inputs::new_project(
        &options.source,
        &options.paths,
        options.project_root.as_deref(),
    )?;
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
            let mut native = Command::new(&binary);
            rill::toolchain::configure_runtime(&mut native)?;
            if let Some(root) = &options.project_root {
                crate::project::Project::load(root)?.configure_runtime(&mut native);
            }
            supervisor.spawn(&mut native)?;
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

#[cfg(test)]
fn snapshot(paths: &[PathBuf]) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    snapshot_excluding(paths, &[])
}

fn snapshot_excluding(
    paths: &[PathBuf],
    excluded: &[PathBuf],
) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut bytes = 0;
    for path in paths {
        visit(path, true, 0, &mut files, &mut bytes, excluded)?;
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
    project_root: Option<PathBuf>,
    requested: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
}

impl Inputs {
    #[cfg(test)]
    fn new(entry: &Path, extras: &[PathBuf]) -> Result<Self, String> {
        Self::new_project(entry, extras, None)
    }

    fn new_project(
        entry: &Path,
        extras: &[PathBuf],
        project_root: Option<&Path>,
    ) -> Result<Self, String> {
        let requested = extras.to_vec();
        let mut extras = extras.to_vec();
        let mut excluded = Vec::new();
        if let Some(root) = project_root {
            let project = crate::project::Project::load(root)?;
            extras.extend(project.watch_inputs()?);
            excluded = project.watch_exclusions();
        }
        let (initial, broken_imports, aliases) =
            source_snapshot_excluding(entry, &extras, &excluded)?;
        Ok(Self {
            entry: entry.into(),
            extras,
            sources: initial.keys().cloned().collect(),
            aliases,
            broken_imports,
            initial: Some(initial),
            project_root: project_root.map(Path::to_path_buf),
            requested,
            excluded,
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
        let next = snapshot_excluding(&paths, &self.excluded)?;
        if &next == previous {
            return Ok(next);
        }
        // Re-discover imports only when an observed input changes. Unchanged
        // polling does no lexing, parsing, name resolution or type checking.
        self.discover()
    }

    fn discover(&mut self) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        if let Some(root) = &self.project_root {
            // A malformed edit must leave the old process alive and retain
            // the manifest in the watch set so a subsequent repair retries.
            if let Ok(project) = crate::project::Project::load(root) {
                if let Ok(paths) = project.watch_inputs() {
                    self.extras = self.requested.clone();
                    self.extras.extend(paths);
                    self.excluded = project.watch_exclusions();
                }
            }
        }
        let (next, broken, aliases) =
            source_snapshot_excluding(&self.entry, &self.extras, &self.excluded)?;
        self.sources = next.keys().cloned().collect();
        self.broken_imports = broken;
        self.aliases = aliases;
        Ok(next)
    }
}

#[cfg(test)]
fn source_snapshot(entry: &Path, extra_paths: &[PathBuf]) -> Result<Discovery, String> {
    source_snapshot_excluding(entry, extra_paths, &[])
}

fn source_snapshot_excluding(
    entry: &Path,
    extra_paths: &[PathBuf],
    excluded: &[PathBuf],
) -> Result<Discovery, String> {
    let mut files = snapshot_excluding(extra_paths, excluded)?;
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
            visit(root, true, 0, &mut files, &mut bytes, excluded)?;
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
    excluded: &[PathBuf],
) -> Result<(), String> {
    if excluded.iter().any(|root| path.starts_with(root)) {
        return Ok(());
    }
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
            visit(&entry.path(), false, depth + 1, files, bytes, excluded)?;
        }
    } else if metadata.is_file()
        && (explicit
            || path.file_name() == Some(OsStr::new("rill.json"))
            || crate::shader::supported_input(path)
            || matches!(
                path.extension().and_then(OsStr::to_str),
                Some("rill" | "rillcfg" | "h" | "c" | "hpp" | "inl" | "inc" | "json")
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
    fn project_inputs_reload_recipes_and_exclude_nested_shader_output() {
        let directory = super::super::RunDirectory::create().unwrap();
        let root = directory.0.canonicalize().unwrap();
        let entry = root.join("main.rill");
        fs::write(&entry, "fn main() { 42 }").unwrap();
        fs::create_dir(root.join("shaders")).unwrap();
        fs::write(root.join("shaders/main.metal"), "kernel void clear() {}").unwrap();
        fs::write(root.join("rill.shaders.json"), r#"{"version":1,"output":"shaders/compiled","shaders":[{"name":"clear","language":"metal","stage":"compute","entry":"clear","source":"shaders/main.metal","target":"msl-source"}]}"#).unwrap();
        let manifest = root.join("rill.json");
        fs::write(
            &manifest,
            r#"{"version":2,"name":"watch","entry":"main.rill","shaders":"rill.shaders.json"}"#,
        )
        .unwrap();
        let mut inputs = Inputs::new_project(&entry, &[], Some(&root)).unwrap();
        let first = inputs.initial.take().unwrap();
        fs::create_dir(root.join("shaders/compiled")).unwrap();
        fs::write(
            root.join("shaders/compiled/manifest.json"),
            vec![b'x'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(inputs.read(&first).unwrap(), first);
        let mut second = first;
        for extension in ["slangh", "hlsli", "glsli"] {
            fs::write(
                root.join(format!("shaders/lighting.{extension}")),
                "// new include",
            )
            .unwrap();
            let next = inputs.read(&second).unwrap();
            assert_ne!(
                next, second,
                "new {extension} include must trigger a rebuild"
            );
            second = next;
        }
        fs::create_dir(root.join("native")).unwrap();
        fs::write(root.join("native/new.c"), "int answer(void) { return 1; }").unwrap();
        fs::write(&manifest, r#"{"version":2,"name":"watch","entry":"main.rill","native":{"sources":["native/new.c"]},"shaders":"rill.shaders.json"}"#).unwrap();
        let third = inputs.read(&second).unwrap();
        assert!(third.contains_key(&root.join("native/new.c")));
        fs::write(root.join("native/new.c"), "int answer(void) { return 2; }").unwrap();
        assert_ne!(inputs.read(&third).unwrap(), third);
    }

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
