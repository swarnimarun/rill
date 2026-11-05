//! Declarative project inputs. Native descriptors and shader layouts remain
//! explicit adapter responsibilities; no user build script is evaluated.
use serde_json::{Map, Value};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct Recipe {
    sources: Vec<PathBuf>,
    include_dirs: Vec<PathBuf>,
    config: rill::c_import::Config,
    pub(super) shaders: Option<crate::shader::ShaderProject>,
}

pub(crate) struct PreparedRecipe {
    // Objects remain valid until the caller finishes linking or running.
    _directory: Option<crate::RunDirectory>,
    pub objects: Vec<OsString>,
}

impl Recipe {
    pub fn load(root: &Path, fields: &Map<String, Value>) -> Result<Self, String> {
        let mut recipe = Self {
            sources: vec![],
            include_dirs: vec![],
            config: Default::default(),
            shaders: None,
        };
        if let Some(value) = fields.get("native") {
            let native = value
                .as_object()
                .ok_or("E-PROJECT-MANIFEST: native must be an object")?;
            for key in native.keys() {
                if !matches!(
                    key.as_str(),
                    "sources" | "include_dirs" | "packages" | "libraries"
                ) {
                    return Err(format!("E-PROJECT-MANIFEST: unknown native field {key:?}"));
                }
            }
            for source in strings(native, "sources")? {
                let path = confined(root, &source, false)?;
                if path.extension() != Some(OsStr::new("c")) {
                    return Err("E-PROJECT-MANIFEST: native sources must be .c files".into());
                }
                recipe.sources.push(path);
            }
            for dir in strings(native, "include_dirs")? {
                recipe.include_dirs.push(confined(root, &dir, true)?);
            }
            recipe.config.packages = strings(native, "packages")?;
            recipe.config.links = strings(native, "libraries")?;
            recipe.config.validate()?;
        }
        if let Some(value) = fields.get("shaders") {
            let relative = value
                .as_str()
                .ok_or("E-PROJECT-MANIFEST: shaders must name a manifest file")?;
            recipe.shaders = Some(crate::shader::ShaderProject::load(&confined(
                root, relative, false,
            )?)?);
        }
        Ok(recipe)
    }

    pub fn has_native(&self) -> bool {
        !self.sources.is_empty()
            || !self.include_dirs.is_empty()
            || !self.config.packages.is_empty()
            || !self.config.links.is_empty()
    }

    pub fn resolve(&self) -> Result<rill::c_import::Resolved, String> {
        let mut resolved = self.config.resolve()?;
        for dir in &self.include_dirs {
            resolved
                .cflags
                .extend([OsString::from("-I"), dir.as_os_str().to_owned()]);
        }
        Ok(resolved)
    }

    pub fn prepare_shaders(&self, command: &str) -> Result<(), String> {
        if let Some(shaders) = &self.shaders {
            if command == "check" {
                shaders.check()?;
            } else {
                shaders.build()?;
            }
        }
        Ok(())
    }

    pub fn prepare(
        &self,
        action: &str,
        compiler: &OsStr,
        zig_cc: bool,
        flags: &[OsString],
    ) -> Result<PreparedRecipe, String> {
        rill::c_import::validate_compile_args(flags)?;
        let directory = if !self.sources.is_empty() && action != "check" {
            Some(crate::RunDirectory::create()?)
        } else {
            None
        };
        let mut objects = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            let mut command = Command::new(compiler);
            if zig_cc {
                command.arg("cc");
            }
            command.args(flags).args(["-std=c11", "-x", "c"]);
            if action == "check" {
                command.arg("-fsyntax-only").arg(source);
            } else {
                let object = directory
                    .as_ref()
                    .unwrap()
                    .0
                    .join(format!("native-{index}.o"));
                command.arg("-c").arg(source).arg("-o").arg(&object);
                objects.push(object.into_os_string());
            }
            let status = command.status().map_err(|e| {
                format!(
                    "E-PROJECT-NATIVE: cannot start {}: {e}",
                    compiler.to_string_lossy()
                )
            })?;
            if !status.success() {
                return Err(format!(
                    "E-PROJECT-NATIVE: {} failed with {status}",
                    source.display()
                ));
            }
        }
        Ok(PreparedRecipe {
            _directory: directory,
            objects,
        })
    }

    pub fn inputs(&self) -> Result<Vec<PathBuf>, String> {
        let mut paths = self.sources.clone();
        for source in &self.sources {
            if let Some(parent) = source.parent() {
                paths.push(parent.to_path_buf());
            }
        }
        paths.extend(self.include_dirs.iter().cloned());
        if let Some(shaders) = &self.shaders {
            paths.extend(shaders.inputs()?);
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn validate_output(&self, output: &Path) -> Result<(), String> {
        let output = crate::destination_path(output)?;
        if self.sources.contains(&output)
            || self
                .include_dirs
                .iter()
                .any(|path| path.starts_with(&output) || output.starts_with(path))
        {
            return Err("E-PROJECT-MANIFEST: output must not replace native inputs".into());
        }
        if let Some(shaders) = &self.shaders {
            let shader_output = crate::destination_path(shaders.output())?;
            if output.starts_with(&shader_output)
                || shader_output.starts_with(&output)
                || shaders.inputs()?.contains(&output)
            {
                return Err(
                    "E-PROJECT-MANIFEST: executable and shader inputs/output must not overlap"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

fn strings(fields: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = fields.get(key) else {
        return Ok(vec![]);
    };
    let values = value
        .as_array()
        .filter(|values| values.len() <= 64)
        .ok_or_else(|| {
            format!("E-PROJECT-MANIFEST: native.{key} must be an array of at most 64 strings")
        })?;
    let mut result = Vec::new();
    for value in values {
        let value = value
            .as_str()
            .ok_or_else(|| format!("E-PROJECT-MANIFEST: native.{key} entries must be strings"))?;
        if result.iter().any(|existing| existing == value) {
            return Err(format!(
                "E-PROJECT-MANIFEST: duplicate native.{key} entry {value:?}"
            ));
        }
        result.push(value.to_owned());
    }
    Ok(result)
}

fn confined(root: &Path, value: &str, directory: bool) -> Result<PathBuf, String> {
    let path = super::relative_path(value)?;
    let path = fs::canonicalize(root.join(path))
        .map_err(|e| format!("E-PROJECT-IO: cannot open {value:?}: {e}"))?;
    if !path.starts_with(root)
        || if directory {
            !path.is_dir()
        } else {
            !path.is_file()
        }
    {
        return Err("E-PROJECT-MANIFEST: native/shader inputs must stay inside the project".into());
    }
    Ok(path)
}
