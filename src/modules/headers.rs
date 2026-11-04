//! Pure source preparation and build-published native interfaces for editors.
//! Cache entries are never used by the ordinary executable/codegen loader.
use super::{ModuleError, ModuleGraph};
pub use crate::c_import::inputs::{capture_inputs, search_dependencies, InputState};
use crate::c_import::inputs::{
    fingerprint, path_value, value_path, MAX_CACHE_BYTES, MAX_DEPENDENCIES,
};
use crate::{
    ast::{Expr, ExternType, Pattern},
    c_import::Config,
    parser::{self, ParsedModule},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    hash::{DefaultHasher, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BINDINGS: usize = 1024;
fn error(message: impl Into<String>) -> ModuleError {
    ModuleError {
        code: "E-C-IMPORT",
        message: message.into(),
    }
}

#[derive(Clone, Debug)]
pub struct HeaderRequest {
    pub module: String,
    pub path: PathBuf,
    pub native: Config,
    pub names: Vec<String>,
}

fn parse(graph: &ModuleGraph) -> Result<BTreeMap<String, ParsedModule>, ModuleError> {
    graph
        .modules
        .iter()
        .map(|(name, module)| {
            parser::parse_module(&module.source)
                .map(|parsed| (name.clone(), parsed))
                .map_err(|message| ModuleError {
                    code: "E-MODULE-PARSE",
                    message: format!("{}:{message}", module.path.display()),
                })
        })
        .collect()
}

/// Match the ordinary lowerer's sorted-module native dependency sequence.
pub fn native_config(graph: &ModuleGraph) -> Result<Config, ModuleError> {
    let mut config = Config::default();
    for module in parse(graph)?.values() {
        config
            .includes
            .extend(module.program.native.includes.iter().cloned());
        config
            .links
            .extend(module.program.native.links.iter().cloned());
        config
            .packages
            .extend(module.program.native.packages.iter().cloned());
    }
    config.validate().map_err(error)?;
    Ok(config)
}

fn declarations(module: &ParsedModule) -> BTreeSet<&str> {
    module
        .program
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .chain(module.program.externs.iter().map(|f| f.name.as_str()))
        .chain(module.program.constants.iter().map(|c| c.name.as_str()))
        .chain(
            module
                .program
                .types
                .iter()
                .flat_map(|t| t.variants.iter().map(|v| v.name.as_str())),
        )
        .collect()
}

/// Find only unresolved value references, respecting Rill's lexical and alias
/// rules. Qualified consumer references request bindings in the header module.
pub fn requests(graph: &ModuleGraph) -> Result<Vec<HeaderRequest>, ModuleError> {
    let parsed = parse(graph)?;
    let declared: BTreeMap<_, _> = parsed
        .iter()
        .map(|(name, unit)| (name.as_str(), declarations(unit)))
        .collect();
    let mut needed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (module, unit) in &parsed {
        let aliases = graph.modules[module]
            .imports
            .iter()
            .map(|i| (i.alias.as_str(), i.module.as_str()))
            .collect();
        let mut uses = Uses {
            module,
            aliases,
            parsed: &parsed,
            declared: &declared,
            needed: &mut needed,
        };
        for function in &unit.program.functions {
            uses.function(
                &function.body,
                function.params.iter().map(|p| p.name.as_str()),
            );
        }
    }
    needed
        .into_iter()
        .map(|(module, names)| {
            if names.len() > 256 {
                return Err(error(format!(
                    "{module}: automatic header import exceeds 256 referenced symbols"
                )));
            }
            Ok(HeaderRequest {
                path: graph.modules[&module].path.clone(),
                native: parsed[&module].program.native.clone(),
                module,
                names: names.into_iter().collect(),
            })
        })
        .collect()
}

struct Uses<'a, 'b> {
    module: &'a str,
    aliases: HashMap<&'a str, &'a str>,
    parsed: &'a BTreeMap<String, ParsedModule>,
    declared: &'a BTreeMap<&'a str, BTreeSet<&'a str>>,
    needed: &'b mut BTreeMap<String, BTreeSet<String>>,
}

enum Work<'a> {
    Expression(&'a Expr),
    Push(&'a str),
    Pop(&'a str),
}

impl Uses<'_, '_> {
    fn name(&mut self, name: &str, locals: &HashMap<&str, usize>) {
        let (module, name) = if let Some((alias, name)) = name.split_once('.') {
            let Some(module) = self.aliases.get(alias) else {
                return;
            };
            (*module, name)
        } else {
            if locals.contains_key(name) || super::compile::builtin(name) {
                return;
            }
            (self.module, name)
        };
        let Some(unit) = self.parsed.get(module) else {
            return;
        };
        if unit.program.native.includes.is_empty()
            || self
                .declared
                .get(module)
                .is_some_and(|names| names.contains(name))
        {
            return;
        }
        self.needed
            .entry(module.to_owned())
            .or_default()
            .insert(name.to_owned());
    }

    fn function<'e>(&mut self, body: &'e Expr, parameters: impl Iterator<Item = &'e str>) {
        let mut locals = HashMap::<&str, usize>::new();
        for name in parameters {
            *locals.entry(name).or_default() += 1;
        }
        let mut work = vec![Work::Expression(body)];
        while let Some(next) = work.pop() {
            let expression = match next {
                Work::Push(name) => {
                    *locals.entry(name).or_default() += 1;
                    continue;
                }
                Work::Pop(name) => {
                    let count = locals.get_mut(name).expect("balanced header scope");
                    *count -= 1;
                    if *count == 0 {
                        locals.remove(name);
                    }
                    continue;
                }
                Work::Expression(expression) => expression,
            };
            match expression {
                Expr::Var(name) => self.name(name, &locals),
                Expr::Call(name, args) => {
                    self.name(name, &locals);
                    work.extend(args.iter().rev().map(Work::Expression));
                }
                Expr::Field(root, fields) => {
                    if !locals.contains_key(root.as_str()) {
                        if fields.len() == 1 && self.aliases.contains_key(root.as_str()) {
                            self.name(&format!("{root}.{}", fields[0]), &locals);
                        } else if !self.aliases.contains_key(root.as_str()) {
                            self.name(root, &locals);
                        }
                    }
                }
                Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                    work.extend([
                        Work::Pop(name),
                        Work::Expression(body),
                        Work::Push(name),
                        Work::Expression(value),
                    ]);
                }
                Expr::Match(value, arms) => {
                    for arm in arms.iter().rev() {
                        let mut patterns = vec![&arm.pattern];
                        let mut bindings = vec![];
                        while let Some(pattern) = patterns.pop() {
                            match pattern {
                                Pattern::Binding(name) => bindings.push(name.as_str()),
                                Pattern::Constructor(name, fields) => {
                                    self.name(name, &locals);
                                    patterns.extend(fields);
                                }
                                _ => {}
                            }
                        }
                        work.extend(bindings.iter().map(|name| Work::Pop(name)));
                        work.push(Work::Expression(&arm.body));
                        work.extend(bindings.iter().rev().map(|name| Work::Push(name)));
                    }
                    work.push(Work::Expression(value));
                }
                Expr::Record(_, fields) => {
                    work.extend(fields.iter().rev().map(|(_, e)| Work::Expression(e)))
                }
                Expr::Binary(_, left, right) | Expr::While(left, right) => {
                    work.extend([Work::Expression(right), Work::Expression(left)])
                }
                Expr::If(condition, yes, no) => work.extend([
                    Work::Expression(no),
                    Work::Expression(yes),
                    Work::Expression(condition),
                ]),
                Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => {
                    work.push(Work::Expression(value))
                }
                _ => {}
            }
        }
    }
}

fn foreign(ty: &ExternType) -> String {
    match ty {
        ExternType::I32 => "i32".into(),
        ExternType::U8 => "u8".into(),
        ExternType::U32 => "u32".into(),
        ExternType::U64 => "u64".into(),
        ExternType::F32 => "f32".into(),
        ExternType::Bool => "bool".into(),
        ExternType::Unit => "unit".into(),
        ExternType::CStr => "cstr".into(),
        ExternType::Pointer { pointee, is_const } => {
            format!("{}<{pointee}>", if *is_const { "const_ptr" } else { "ptr" })
        }
    }
}

fn binding_text(generated: &str, existing: &BTreeSet<&str>) -> Result<String, ModuleError> {
    if generated.len() > 1024 * 1024 {
        return Err(error("generated header bindings exceed 1 MiB"));
    }
    let unit = parser::parse_module(generated).map_err(error)?;
    if !unit.program.functions.is_empty()
        || !unit.program.types.is_empty()
        || unit.program.externs.len() + unit.program.constants.len() > MAX_BINDINGS
    {
        return Err(error(
            "generated header interface must contain only bounded foreign declarations",
        ));
    }
    let mut text = String::new();
    let mut seen = BTreeSet::new();
    for f in &unit.program.externs {
        crate::check::validate_extern_function(f).map_err(error)?;
        if !seen.insert(f.name.as_str()) {
            return Err(error("duplicate generated header declaration"));
        }
        if existing.contains(f.name.as_str()) {
            continue;
        }
        let params = f
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, foreign(&p.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(
            "\nexport extern fn {}({params}) -> {};",
            f.name,
            foreign(&f.result)
        ));
    }
    for c in &unit.program.constants {
        crate::check::validate_extern_constant(c).map_err(error)?;
        if !seen.insert(c.name.as_str()) {
            return Err(error("duplicate generated header declaration"));
        }
        if !existing.contains(c.name.as_str()) {
            text.push_str(&format!(
                "\nexport extern const {}: {};",
                c.name,
                foreign(&c.ty)
            ));
        }
    }
    Ok(text)
}

/// Add validated declarations in memory. Native preambles and other generated
/// text never become additional build inputs or overwrite explicit visibility.
pub fn apply(
    graph: &mut ModuleGraph,
    module: &str,
    generated_module: &str,
) -> Result<(), ModuleError> {
    let target = graph
        .modules
        .get_mut(module)
        .ok_or_else(|| error("unknown header module"))?;
    let parsed = parser::parse_module(&target.source).map_err(error)?;
    let text = binding_text(generated_module, &declarations(&parsed))?;
    target.source.push_str(&text);
    Ok(())
}

fn native_value(config: &Config) -> Value {
    json!({"includes":config.includes,"links":config.links,"packages":config.packages})
}

fn graph_native_value(graph: &ModuleGraph) -> Result<Value, ModuleError> {
    let units = parse(graph)?;
    Ok(Value::Array(graph.modules.iter().map(|(name, module)| {
        json!({"module":name,"path":path_value(&module.path),"native":native_value(&units[name].program.native)})
    }).collect()))
}

fn cache_file(path: &Path) -> PathBuf {
    let mut hash = DefaultHasher::new();
    hash.write(path_value(path).to_string().as_bytes());
    std::env::temp_dir()
        .join("rill-header-cache-v1")
        .join(format!("{:016x}.json", hash.finish()))
}

/// A captured editor interface. Publishing checks its original dependencies
/// again and never associates old declarations with newer input fingerprints.
#[derive(Debug)]
pub struct Snapshot {
    target: PathBuf,
    bytes: Vec<u8>,
    inputs: InputState,
}

pub fn prepare(
    graph: &ModuleGraph,
    module: &str,
    generated_module: &str,
    dependencies: &[PathBuf],
) -> Result<Snapshot, ModuleError> {
    prepare_with_state(
        graph,
        module,
        generated_module,
        &InputState::capture(dependencies)?,
    )
}

pub fn prepare_with_state(
    graph: &ModuleGraph,
    module: &str,
    generated_module: &str,
    inputs: &InputState,
) -> Result<Snapshot, ModuleError> {
    if !inputs.current() {
        return Err(error(
            "header inputs changed during declaration import; retry the build",
        ));
    }
    let source = graph
        .modules
        .get(module)
        .ok_or_else(|| error("unknown header module"))?;
    let parsed = parser::parse_module(&source.source).map_err(error)?;
    if parsed.program.native.includes.is_empty() {
        return Err(error("cannot publish a header interface without @include"));
    }
    let native = graph_native_value(graph)?;
    let generated = binding_text(generated_module, &declarations(&parsed))?;
    let value = json!({"version":2,"path":path_value(&source.path),"native":native,"bindings":generated,"dependencies":inputs.dependencies});
    let bytes = serde_json::to_vec(&value).map_err(|e| error(e.to_string()))?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return Err(error("header interface cache exceeds 2 MiB"));
    }
    Ok(Snapshot {
        target: cache_file(&source.path),
        bytes,
        inputs: inputs.clone(),
    })
}

impl Snapshot {
    /// False means inputs changed after preparation. A native build can still
    /// succeed while its editor interface conservatively waits for a refresh.
    pub fn publish(self) -> Result<bool, ModuleError> {
        if !self.inputs.current() {
            return Ok(false);
        }
        write_cache(&self.target, &self.bytes)?;
        Ok(true)
    }
}

/// Publish a last-successful-build editor snapshot. Content hashes detect
/// changes, not adversarial substitution; this is not a security boundary.
pub fn publish(
    graph: &ModuleGraph,
    module: &str,
    generated_module: &str,
    dependencies: &[PathBuf],
) -> Result<(), ModuleError> {
    prepare(graph, module, generated_module, dependencies)?
        .publish()
        .map(|_| ())
}

fn write_cache(target: &Path, bytes: &[u8]) -> Result<(), ModuleError> {
    let directory = target.parent().unwrap();
    fs::create_dir_all(directory).map_err(|e| error(e.to_string()))?;
    if fs::symlink_metadata(directory)
        .map_err(|e| error(e.to_string()))?
        .file_type()
        .is_symlink()
    {
        return Err(error(
            "header interface cache directory must not be a symlink",
        ));
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let temporary = directory.join(format!(
            ".{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(error(e.to_string())),
        }
    };
    let result = (|| {
        file.write_all(bytes).map_err(|e| error(e.to_string()))?;
        file.sync_all().map_err(|e| error(e.to_string()))?;
        drop(file);
        fs::rename(&temporary, target).map_err(|e| error(e.to_string()))
    })();
    let _ = fs::remove_file(temporary);
    result
}

/// Read validated build snapshots without executing native tools. Missing,
/// malformed or stale snapshots are ignored and normal name errors remain.
/// Returned lengths separate real source from appended generated declarations.
pub fn apply_cached(graph: &mut ModuleGraph) -> Result<BTreeMap<String, usize>, ModuleError> {
    if native_config(graph)?.includes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let native = graph_native_value(graph)?;
    let mut lengths = BTreeMap::new();
    for module in graph.modules.keys().cloned().collect::<Vec<_>>() {
        let source = &graph.modules[&module];
        let path = cache_file(&source.path);
        let read = (|| -> Option<String> {
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() > MAX_CACHE_BYTES {
                return None;
            }
            let mut bytes = vec![];
            File::open(path)
                .ok()?
                .take(MAX_CACHE_BYTES + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > MAX_CACHE_BYTES {
                return None;
            }
            let value: Value = serde_json::from_slice(&bytes).ok()?;
            if value.get("version")?.as_u64()? != 2
                || value.get("path")? != &path_value(&source.path)
                || value.get("native")? != &native
            {
                return None;
            }
            let dependencies = value.get("dependencies")?.as_array()?;
            if dependencies.is_empty() || dependencies.len() > MAX_DEPENDENCIES {
                return None;
            }
            let mut total = 0;
            let mut work = 0;
            for old in dependencies {
                let path = value_path(old.get("path")?)?;
                if fingerprint(&path, &mut total, &mut work).ok()?.ne(old) {
                    return None;
                }
            }
            Some(value.get("bindings")?.as_str()?.to_owned())
        })();
        if let Some(bindings) = read {
            let original = source.source.len();
            // Cache bytes remain untrusted even after their metadata matches.
            if apply(graph, &module, &bindings).is_ok() {
                lengths.insert(module, original);
            }
        }
    }
    Ok(lengths)
}
