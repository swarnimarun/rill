//! A small local-project driver over the ordinary compiler commands.
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

const MANIFEST: &str = "rill.json";
const MAX_MANIFEST: u64 = 65_536;

mod recipe;
pub(super) use recipe::PreparedRecipe;

pub(super) struct Project {
    pub(super) name: String,
    pub(super) root: PathBuf,
    pub(super) entry: PathBuf,
    output: PathBuf,
    recipe: recipe::Recipe,
}

pub fn create(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let directory = PathBuf::from(args.next().ok_or("usage: rill new <directory>")?);
    if args.next().is_some() {
        return Err("usage: rill new <directory>".into());
    }
    let leaf = directory
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("E-PROJECT-NAME: project directory needs a UTF-8 name")?;
    let mut name: String = leaf
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    name = name.trim_matches('-').to_owned();
    if name.is_empty() {
        name = "app".into();
    } else if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "app-");
    }
    validate_name(&name)?;
    fs::create_dir(&directory).map_err(|e| {
        format!(
            "E-PROJECT-CREATE: cannot create {}: {e}",
            directory.display()
        )
    })?;
    let mut created = Vec::new();
    let source_dir = directory.join("src");
    let mut source_created = false;
    let result = (|| {
        fs::create_dir(&source_dir).map_err(|e| format!("E-PROJECT-CREATE: {e}"))?;
        source_created = true;
        let manifest = serde_json::json!({
            "version": 1,
            "name": name,
            "entry": "src/main.rill"
        });
        for (path, contents) in [
            (
                directory.join(MANIFEST),
                format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
            ),
            (
                source_dir.join("main.rill"),
                "fn main() {\n    42\n}\n".into(),
            ),
            (directory.join(".gitignore"), "/build/\n".into()),
        ] {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("E-PROJECT-CREATE: cannot create {}: {e}", path.display()))?;
            created.push(path);
            file.write_all(contents.as_bytes())
                .map_err(|e| format!("E-PROJECT-CREATE: cannot write project: {e}"))?;
        }
        Ok(())
    })();
    if result.is_err() {
        for path in created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        if source_created {
            let _ = fs::remove_dir(source_dir);
        }
        let _ = fs::remove_dir(&directory);
    }
    result
}

pub fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let action = args
        .next()
        .ok_or("usage: rill project <check|build|run|watch|docs> [directory] [options]")?;
    if !matches!(
        action.to_str(),
        Some("check" | "build" | "run" | "watch" | "docs")
    ) {
        return Err(
            "usage: rill project <check|build|run|watch|docs> [directory] [options]".into(),
        );
    }
    let mut remaining = args.peekable();
    let directory = if remaining
        .peek()
        .is_some_and(|arg| !arg.to_string_lossy().starts_with('-'))
    {
        PathBuf::from(remaining.next().unwrap())
    } else {
        PathBuf::from(".")
    };
    let mut options: Vec<OsString> = remaining.collect();
    if action == "docs"
        && options.len() == 1
        && matches!(options[0].to_str(), Some("--help" | "-h"))
    {
        return crate::docgen::run(options.into_iter());
    }
    let project = Project::load(&directory)?;
    if action == "docs" {
        // Invoke the same docs driver with the already validated project root.
        // No compiler flags or executable output paths apply to documentation.
        let mut args = vec![project.root.into_os_string()];
        args.extend(options);
        return crate::docgen::run(args.into_iter());
    }
    if matches!(action.to_str(), Some("build" | "run" | "watch")) {
        project.configure_cache(&mut options)?;
    }
    if action == "watch" {
        options.push("--watch".into());
        options.push(project.root.join(MANIFEST).into_os_string());
    }
    options.extend([
        OsString::from("--project-root"),
        project.root.clone().into_os_string(),
    ]);
    if action == "build" {
        let explicit = explicit_output(&options)?;
        let output = explicit.clone().unwrap_or_else(|| project.output.clone());
        project.prepare_output(&output)?;
        if explicit.is_none() {
            options.push("-o".into());
            options.push(output.into_os_string());
        }
    }
    let executable = std::env::current_exe()
        .map_err(|e| format!("E-PROJECT-RUN: cannot locate compiler: {e}"))?;
    let mut command = Command::new(executable);
    command
        .arg(&action)
        .arg(&project.entry)
        .args(options)
        .current_dir(&project.root);
    rill::toolchain::configure_runtime(&mut command)?;
    #[cfg(unix)]
    if action == "watch" {
        use std::os::unix::process::CommandExt;
        // Give the long-lived watcher this PID so SIGTERM from a parent
        // supervisor reaches its cancellation handler directly.
        return Err(format!(
            "E-PROJECT-RUN: cannot start watcher: {}",
            command.exec()
        ));
    }
    let status = command
        .status()
        .map_err(|e| format!("E-PROJECT-RUN: cannot start compiler: {e}"))?;
    if !status.success() {
        return Err(format!(
            "E-PROJECT-RUN: compiler command failed with {status}"
        ));
    }
    Ok(())
}

impl Project {
    pub(super) fn load(directory: &Path) -> Result<Self, String> {
        let root = directory
            .canonicalize()
            .map_err(|e| format!("E-PROJECT-IO: cannot open {}: {e}", directory.display()))?;
        let path = root.join(MANIFEST);
        let mut text = String::new();
        fs::File::open(&path)
            .and_then(|file| file.take(MAX_MANIFEST + 1).read_to_string(&mut text))
            .map_err(|e| format!("E-PROJECT-IO: cannot read {}: {e}", path.display()))?;
        if text.len() as u64 > MAX_MANIFEST {
            return Err("E-PROJECT-MANIFEST: rill.json exceeds 65536 bytes".into());
        }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("E-PROJECT-MANIFEST: invalid JSON: {e}"))?;
        let fields = value
            .as_object()
            .ok_or("E-PROJECT-MANIFEST: expected a JSON object")?;
        reject_duplicate_fields(&text)?;
        let version = fields
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .filter(|version| matches!(version, 1 | 2))
            .ok_or("E-PROJECT-MANIFEST: version must be the integer 1 or 2")?;
        for field in fields.keys() {
            if !matches!(field.as_str(), "version" | "name" | "entry" | "output")
                && !(version == 2 && matches!(field.as_str(), "native" | "shaders"))
            {
                return Err(format!("E-PROJECT-MANIFEST: unknown field {field:?}"));
            }
        }
        let string = |field: &str| {
            fields
                .get(field)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("E-PROJECT-MANIFEST: {field} must be a string"))
        };
        let name = string("name")?;
        validate_name(name)?;
        let entry = relative_path(string("entry")?)?;
        if entry.extension() != Some(OsStr::new("rill")) {
            return Err("E-PROJECT-MANIFEST: entry must name a .rill source file".into());
        }
        let resolved = root
            .join(&entry)
            .canonicalize()
            .map_err(|e| format!("E-PROJECT-IO: cannot open entry {}: {e}", entry.display()))?;
        if !resolved.starts_with(&root) || !resolved.is_file() {
            return Err("E-PROJECT-MANIFEST: entry must be a file inside the project".into());
        }
        let output = if fields.contains_key("output") {
            relative_path(string("output")?)?
        } else {
            PathBuf::from("build").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        };
        let recipe = recipe::Recipe::load(&root, fields)?;
        let project = Self {
            name: name.to_owned(),
            root,
            entry,
            output,
            recipe,
        };
        project.validate_output(&project.output)?;
        project
            .recipe
            .validate_output(&project.root.join(&project.output))?;
        Ok(project)
    }

    pub(super) fn has_recipes(&self) -> bool {
        self.recipe.has_native() || self.recipe.shaders.is_some()
    }

    pub(super) fn has_native(&self) -> bool {
        self.recipe.has_native()
    }

    pub(super) fn resolve_recipe(&self) -> Result<rill::c_import::Resolved, String> {
        self.recipe.resolve()
    }

    pub(super) fn prepare_recipe(
        &self,
        command: &str,
        compiler: &OsStr,
        zig_cc: bool,
        cflags: &[OsString],
    ) -> Result<PreparedRecipe, String> {
        self.recipe.prepare(command, compiler, zig_cc, cflags)
    }

    pub(super) fn prepare_shaders(&self, command: &str) -> Result<(), String> {
        self.recipe.prepare_shaders(command)
    }

    pub(super) fn configure_runtime(&self, command: &mut Command) {
        command.env("RILL_PROJECT_ROOT", &self.root);
        if let Some(shaders) = &self.recipe.shaders {
            command.env("RILL_PROJECT_SHADER_DIR", shaders.output());
        }
    }

    pub(super) fn watch_inputs(&self) -> Result<Vec<PathBuf>, String> {
        let mut paths = vec![self.root.join(MANIFEST)];
        paths.extend(self.recipe.inputs()?);
        Ok(paths)
    }

    pub(super) fn watch_exclusions(&self) -> Vec<PathBuf> {
        self.recipe
            .shaders
            .as_ref()
            .map(|shaders| vec![shaders.output().to_path_buf()])
            .unwrap_or_default()
    }

    fn validate_output(&self, output: &Path) -> Result<(), String> {
        if output == self.entry
            || output == Path::new(MANIFEST)
            || output.extension() == Some(OsStr::new("rill"))
        {
            return Err(
                "E-PROJECT-MANIFEST: output must not overwrite project source or rill.json".into(),
            );
        }
        let mut ancestor = self.root.join(output);
        loop {
            match fs::symlink_metadata(&ancestor) {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !ancestor.pop() {
                        return Err("E-PROJECT-MANIFEST: cannot resolve output directory".into());
                    }
                }
                Err(e) => return Err(format!("E-PROJECT-IO: cannot inspect output: {e}")),
            }
        }
        let resolved = ancestor
            .canonicalize()
            .map_err(|e| format!("E-PROJECT-IO: cannot resolve output: {e}"))?;
        if !resolved.starts_with(&self.root) {
            return Err("E-PROJECT-MANIFEST: output must stay inside the project".into());
        }
        let entry = self
            .root
            .join(&self.entry)
            .canonicalize()
            .map_err(|e| format!("E-PROJECT-IO: cannot resolve entry: {e}"))?;
        let manifest = self
            .root
            .join(MANIFEST)
            .canonicalize()
            .map_err(|e| format!("E-PROJECT-IO: cannot resolve manifest: {e}"))?;
        if ancestor == self.root.join(output)
            && (resolved.is_dir()
                || resolved == entry
                || resolved == manifest
                || resolved.extension() == Some(OsStr::new("rill")))
        {
            return Err(
                "E-PROJECT-MANIFEST: output must not overwrite project source or a directory"
                    .into(),
            );
        }
        Ok(())
    }

    fn prepare_output(&self, output: &Path) -> Result<(), String> {
        self.validate_output(output)?;
        self.recipe.validate_output(&self.root.join(output))?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(self.root.join(parent))
                .map_err(|e| format!("E-PROJECT-IO: cannot create output directory: {e}"))?;
        }
        Ok(())
    }

    fn configure_cache(&self, options: &mut Vec<OsString>) -> Result<(), String> {
        let mut args = options.iter();
        let mut mode = None;
        let mut cache_dir = None;
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--incremental") => mode = Some(true),
                Some("--no-incremental") => mode = Some(false),
                Some("--cache-dir") => {
                    if cache_dir.is_some() {
                        return Err(
                            "E-PROJECT-CACHE: --cache-dir may be specified only once".into()
                        );
                    }
                    let value = args
                        .next()
                        .ok_or("E-PROJECT-CACHE: --cache-dir requires a directory")?;
                    cache_dir =
                        Some(relative_path(value.to_str().ok_or(
                            "E-PROJECT-CACHE: cache directory needs a UTF-8 path",
                        )?)?);
                }
                Some(
                    "-o" | "--cc" | "--c-arg" | "--link-arg" | "--opt-level" | "--simd"
                    | "--native-config" | "--timeout-ms" | "--watch" | "--interval-ms",
                ) => {
                    args.next();
                }
                _ => {}
            }
        }
        if mode == Some(false) && cache_dir.is_none() {
            return Ok(());
        }
        let explicit = cache_dir.is_some();
        let cache_dir = cache_dir.unwrap_or_else(|| PathBuf::from("build/.rill-cache"));
        let output = explicit_output(options)?.unwrap_or_else(|| self.output.clone());
        if cache_dir.starts_with(&output) || output.starts_with(&cache_dir) {
            if !explicit && mode.is_none() {
                options.push("--no-incremental".into());
                return Ok(());
            }
            return Err(
                "E-PROJECT-CACHE: cache directory and executable output must not contain each other; choose another --cache-dir or -o path".into(),
            );
        }
        let mut path = self.root.clone();
        for component in cache_dir.components() {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    if !explicit && mode.is_none() {
                        options.push("--no-incremental".into());
                        return Ok(());
                    }
                    return Err("E-PROJECT-CACHE: cache path must use directories inside the project, without symlinks".into());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "E-PROJECT-CACHE: cannot inspect cache directory: {error}"
                    ))
                }
            }
        }
        if !explicit {
            options.extend([OsString::from("--cache-dir"), cache_dir.into_os_string()]);
        }
        Ok(())
    }
}

fn relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains(['\\', ':'])
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.components().next().is_none()
    {
        return Err(
            "E-PROJECT-MANIFEST: paths must be relative, without '.' or '..' segments".into(),
        );
    }
    Ok(path.into())
}

fn validate_name(name: &str) -> Result<(), String> {
    if !(1..=64).contains(&name.len())
        || !name.starts_with(|c: char| c.is_ascii_alphabetic())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("E-PROJECT-NAME: name must have 1..64 ASCII letters, digits, '-' or '_', starting with a letter".into());
    }
    Ok(())
}

fn explicit_output(options: &[OsString]) -> Result<Option<PathBuf>, String> {
    let mut args = options.iter();
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--") => break,
            Some("-o") => {
                if output.is_some() {
                    return Err("E-PROJECT-MANIFEST: -o may be specified only once".into());
                }
                let path = args
                    .next()
                    .ok_or("E-PROJECT-MANIFEST: -o requires a path")?;
                output = Some(relative_path(
                    path.to_str()
                        .ok_or("E-PROJECT-MANIFEST: output needs a UTF-8 path")?,
                )?);
            }
            Some(
                "--cc" | "--c-arg" | "--link-arg" | "--opt-level" | "--simd" | "--native-config"
                | "--timeout-ms" | "--cache-dir",
            ) => {
                args.next();
            }
            _ => {}
        }
    }
    Ok(output)
}

// The JSON parser has already validated syntax. Track only top-level keys so
// serde_json's last-value-wins map cannot hide duplicate configuration fields.
fn reject_duplicate_fields(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0;
    let mut seen = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => {
                depth += 1;
                seen.push(BTreeSet::new());
            }
            b'}' | b']' => {
                depth -= 1;
                seen.pop();
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
                let after = text[index + 1..].trim_start();
                if depth > 0 && after.starts_with(':') {
                    let field: String = serde_json::from_str(&text[start..=index]).unwrap();
                    if !seen.last_mut().unwrap().insert(field.clone()) {
                        return Err(format!("E-PROJECT-MANIFEST: duplicate field {field:?}"));
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}
