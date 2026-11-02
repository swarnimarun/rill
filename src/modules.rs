//! Bounded discovery of local and explicitly imported bundled source modules.
//! A successful load retains source snapshots but does not check the program.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::lexer::{lex, Kind, Token};

pub mod compile;
pub mod resolve;
mod stdlib;

const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_MODULES: usize = 256;
const MAX_DEPTH: usize = 64;
const ENTRY: &str = "@entry";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub alias: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    /// Canonical local path, or a synthetic `<std.name>` diagnostic path.
    pub path: PathBuf,
    pub source: String,
    pub imports: Vec<Import>,
}

#[derive(Clone, Debug)]
pub struct ModuleGraph {
    pub root: PathBuf,
    /// Modules are sorted by logical name; `@entry` identifies the entry file.
    pub modules: BTreeMap<String, Module>,
}

#[derive(Debug)]
pub struct ModuleError {
    pub code: &'static str,
    pub message: String,
}
impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ModuleError {}
fn error(code: &'static str, message: impl Into<String>) -> ModuleError {
    ModuleError {
        code,
        message: message.into(),
    }
}

/// Discover a bounded, acyclic graph rooted at `entry` without checking it.
///
/// Paths are canonicalized and must stay under the entry's parent directory.
/// This is not race-safe against an adversary changing filesystem paths while
/// loading; use only a trusted, stable source tree until a directory-handle
/// based loader is available.
pub fn load(entry: &Path) -> Result<ModuleGraph, ModuleError> {
    let parent = entry
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(parent)
        .map_err(|e| error("E-MODULE-IO", format!("{}: {e}", parent.display())))?;
    let path = fs::canonicalize(entry)
        .map_err(|e| error("E-MODULE-IO", format!("{}: {e}", entry.display())))?;
    if !path.starts_with(&root) {
        return Err(error(
            "E-MODULE-PATH",
            format!("entry escapes project root: {}", entry.display()),
        ));
    }
    let mut loader = Loader {
        root: root.clone(),
        modules: BTreeMap::new(),
        paths: HashMap::new(),
        #[cfg(unix)]
        identities: HashMap::new(),
        stack: Vec::new(),
    };
    loader.visit(ENTRY.to_string(), path)?;
    Ok(ModuleGraph {
        root,
        modules: loader.modules,
    })
}

struct Loader {
    root: PathBuf,
    modules: BTreeMap<String, Module>,
    paths: HashMap<PathBuf, String>,
    #[cfg(unix)]
    identities: HashMap<(u64, u64), String>,
    stack: Vec<(String, PathBuf)>,
}
impl Loader {
    fn visit(&mut self, name: String, path: PathBuf) -> Result<(), ModuleError> {
        if let Some(start) = self.stack.iter().position(|(_, p)| *p == path) {
            let mut names: Vec<_> = self.stack[start..]
                .iter()
                .map(|(n, _)| n.as_str())
                .collect();
            names.push(&name);
            return Err(error("E-MODULE-CYCLE", names.join(" -> ")));
        }
        if let Some(old) = self.paths.get(&path) {
            if old != &name {
                return Err(error(
                    "E-MODULE-DUPLICATE",
                    format!("{name} and {old} resolve to the same file"),
                ));
            }
            return Ok(());
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(error("E-MODULE-LIMIT", "import depth exceeds 64"));
        }
        if self.paths.len() >= MAX_MODULES {
            return Err(error("E-MODULE-LIMIT", "module count exceeds 256"));
        }
        let source = if let Some(source) = stdlib::source(&name) {
            source.to_owned()
        } else {
            let file = File::open(&path)
                .map_err(|e| error("E-MODULE-IO", format!("{}: {e}", path.display())))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = file
                    .metadata()
                    .map_err(|e| error("E-MODULE-IO", format!("{}: {e}", path.display())))?;
                let identity = (metadata.dev(), metadata.ino());
                if let Some(old) = self.identities.get(&identity) {
                    return Err(error(
                        "E-MODULE-DUPLICATE",
                        format!("{name} and {old} identify the same file"),
                    ));
                }
                self.identities.insert(identity, name.clone());
            }
            let mut bytes = Vec::new();
            file.take(MAX_SOURCE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| error("E-MODULE-IO", format!("{}: {e}", path.display())))?;
            if bytes.len() as u64 > MAX_SOURCE_BYTES {
                return Err(error(
                    "E-MODULE-LIMIT",
                    format!("{} exceeds 1 MiB", path.display()),
                ));
            }
            String::from_utf8(bytes)
                .map_err(|_| error("E-MODULE-UTF8", format!("{} is not UTF-8", path.display())))?
        };
        if source.len() as u64 > MAX_SOURCE_BYTES {
            return Err(error(
                "E-MODULE-LIMIT",
                format!("{} exceeds 1 MiB", path.display()),
            ));
        }
        let imports = parse_imports(&source, &path)?;
        self.paths.insert(path.clone(), name.clone());
        self.stack.push((name.clone(), path.clone()));
        let mut sorted = imports.clone();
        sorted.sort_by(|a, b| a.module.cmp(&b.module));
        for import in &sorted {
            if import.module == "std" || import.module.starts_with("std.") {
                if stdlib::source(&import.module).is_none() {
                    return Err(error(
                        "E-MODULE-STDLIB",
                        format!(
                            "{}:{}:{}: unknown bundled module {}",
                            path.display(),
                            import.line,
                            import.column,
                            import.module
                        ),
                    ));
                }
                self.visit(
                    import.module.clone(),
                    PathBuf::from(format!("<{}>", import.module)),
                )?;
                continue;
            }
            if stdlib::source(&name).is_some() {
                return Err(error(
                    "E-MODULE-STDLIB",
                    format!(
                        "bundled module {name} cannot import local module {}",
                        import.module
                    ),
                ));
            }
            let candidate = self
                .root
                .join(import.module.replace('.', "/"))
                .with_extension("rill");
            let target = fs::canonicalize(&candidate).map_err(|e| {
                error(
                    "E-MODULE-IO",
                    format!(
                        "{}:{}:{}: {}: {e}",
                        path.display(),
                        import.line,
                        import.column,
                        candidate.display()
                    ),
                )
            })?;
            if !target.starts_with(&self.root) {
                return Err(error(
                    "E-MODULE-PATH",
                    format!(
                        "{}:{}:{}: import {} escapes project root",
                        path.display(),
                        import.line,
                        import.column,
                        import.module
                    ),
                ));
            }
            self.visit(import.module.clone(), target)?;
        }
        self.stack.pop();
        self.modules.insert(
            name.clone(),
            Module {
                name,
                path,
                source,
                imports,
            },
        );
        Ok(())
    }
}

fn valid_name(s: &str) -> bool {
    let mut chars = s.bytes();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && s != "import"
        && s != "as"
}
fn is_ident(token: &Token, name: &str) -> bool {
    matches!(&token.kind, Kind::Ident(s) if s == name)
}
fn parse_imports(source: &str, path: &Path) -> Result<Vec<Import>, ModuleError> {
    let tokens =
        lex(source).map_err(|e| error("E-MODULE-LEX", format!("{}:{e}", path.display())))?;
    let mut pos = 0;
    let mut imports = Vec::new();
    let mut aliases = HashSet::new();
    let mut modules = HashSet::new();
    while is_ident(&tokens[pos], "import") || tokens[pos].kind == Kind::At {
        if tokens[pos].kind == Kind::At {
            // Native directives share the leading preamble. The source parser
            // validates their full grammar; graph discovery only skips them.
            while !matches!(tokens[pos].kind, Kind::Semi | Kind::Eof) {
                pos += 1;
            }
            if tokens[pos].kind == Kind::Eof {
                break;
            }
            pos += 1;
            continue;
        }
        let start = &tokens[pos];
        pos += 1;
        let Kind::Ident(name) = &tokens[pos].kind else {
            return Err(error(
                "E-MODULE-IMPORT",
                format!(
                    "{}:{}:{}: expected module name",
                    path.display(),
                    start.line,
                    start.column
                ),
            ));
        };
        if !valid_name(name) {
            return Err(error(
                "E-MODULE-IMPORT",
                format!(
                    "{}:{}:{}: invalid module name {name:?}",
                    path.display(),
                    start.line,
                    start.column
                ),
            ));
        }
        let mut name = name.clone();
        let mut default_alias = name.clone();
        pos += 1;
        while tokens[pos].kind == Kind::Dot {
            pos += 1;
            let Kind::Ident(segment) = &tokens[pos].kind else {
                return Err(error(
                    "E-MODULE-IMPORT",
                    format!(
                        "{}:{}:{}: expected module segment",
                        path.display(),
                        start.line,
                        start.column
                    ),
                ));
            };
            if !valid_name(segment) {
                return Err(error(
                    "E-MODULE-IMPORT",
                    format!(
                        "{}:{}:{}: invalid module segment {segment:?}",
                        path.display(),
                        start.line,
                        start.column
                    ),
                ));
            }
            name.push('.');
            name.push_str(segment);
            default_alias = segment.clone();
            pos += 1;
        }
        let alias = if is_ident(&tokens[pos], "as") {
            pos += 1;
            let Kind::Ident(alias) = &tokens[pos].kind else {
                return Err(error(
                    "E-MODULE-IMPORT",
                    format!(
                        "{}:{}:{}: expected alias",
                        path.display(),
                        start.line,
                        start.column
                    ),
                ));
            };
            if !valid_name(alias) {
                return Err(error(
                    "E-MODULE-IMPORT",
                    format!(
                        "{}:{}:{}: invalid alias {alias:?}",
                        path.display(),
                        start.line,
                        start.column
                    ),
                ));
            }
            pos += 1;
            alias.clone()
        } else {
            default_alias
        };
        if tokens[pos].kind != Kind::Semi {
            return Err(error(
                "E-MODULE-IMPORT",
                format!(
                    "{}:{}:{}: import requires a semicolon",
                    path.display(),
                    start.line,
                    start.column
                ),
            ));
        }
        pos += 1;
        if !aliases.insert(alias.clone()) {
            return Err(error(
                "E-MODULE-DUPLICATE",
                format!(
                    "{}:{}:{}: duplicate alias {alias}",
                    path.display(),
                    start.line,
                    start.column
                ),
            ));
        }
        if !modules.insert(name.clone()) {
            return Err(error(
                "E-MODULE-DUPLICATE",
                format!(
                    "{}:{}:{}: duplicate import {name}",
                    path.display(),
                    start.line,
                    start.column
                ),
            ));
        }
        imports.push(Import {
            module: name,
            alias,
            line: start.line,
            column: start.column,
        });
    }
    let late_import = tokens[pos..].windows(3).find(|window| {
        is_ident(&window[0], "import")
            && matches!(&window[1].kind, Kind::Ident(_))
            && (window[2].kind == Kind::Semi
                || window[2].kind == Kind::Dot
                || is_ident(&window[2], "as"))
    });
    if let Some(token) = late_import.map(|window| &window[0]) {
        return Err(error(
            "E-MODULE-IMPORT",
            format!(
                "{}:{}:{}: imports must precede declarations",
                path.display(),
                token.line,
                token.column
            ),
        ));
    }
    Ok(imports)
}
