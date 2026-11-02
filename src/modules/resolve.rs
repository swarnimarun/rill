//! Experimental symbol indexing for an already loaded module graph.
//!
//! Declarations and their visibility are supplied by the caller, not extracted
//! from source. This module does not validate bodies, infer interfaces, check
//! private type leakage, or produce a compilable program. Do not use it as an
//! authorization or checking boundary until a parser supplies trusted metadata.
use super::{ModuleError, ModuleGraph};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(usize);
impl SymbolId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    Type,
    Value,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Type,
    Constructor,
    Function,
    Constant,
}
impl Kind {
    fn namespace(self) -> Namespace {
        match self {
            Self::Type => Namespace::Type,
            Self::Constructor | Self::Function | Self::Constant => Namespace::Value,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub kind: Kind,
    pub exported: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub id: SymbolId,
    pub owner: ModuleId,
    pub name: String,
    pub kind: Kind,
    pub exported: bool,
}
struct Scope {
    id: ModuleId,
    aliases: HashMap<String, String>,
    symbols: HashMap<(Namespace, String), Symbol>,
}
/// Read-only lookup over supplied declarations. Module and symbol IDs are
/// unique only within this index and stable for the same sorted input.
pub struct Resolver {
    scopes: BTreeMap<String, Scope>,
}
fn error(code: &'static str, message: impl Into<String>) -> ModuleError {
    ModuleError {
        code,
        message: message.into(),
    }
}
fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn reserved_value(name: &str) -> bool {
    matches!(
        name,
        "byte_len"
            | "byte_at"
            | "bytes_new"
            | "bytes_len"
            | "bytes_get"
            | "bytes_set"
            | "u8x16_load"
            | "u8x16_lane"
            | "u8x16_and"
            | "u8x16_or"
            | "u8x16_xor"
            | "u8x16_eq"
            | "u8x16_any"
            | "u8x16_all"
            | "system_box_i32"
            | "system_bytes_zeroed"
            | "system_box_i32_abort"
            | "box_read"
            | "box_write"
            | "system_bytes_zeroed_abort"
            | "owned_bytes_len"
            | "owned_bytes_get"
            | "owned_bytes_set"
            | "u8"
            | "u32"
            | "u64"
            | "f32"
            | "c_null"
            | "is_null"
    )
}
fn reserved_type(name: &str) -> bool {
    matches!(
        name,
        "i32"
            | "bool"
            | "unit"
            | "str"
            | "u8x16"
            | "SystemBoxI32"
            | "SystemBytes"
            | "u8"
            | "u32"
            | "u64"
            | "f32"
            | "ptr"
            | "const_ptr"
    )
}
impl Resolver {
    /// Build an index from declarations provided for every graph node.
    /// The caller must not treat these supplied export flags as source-verified.
    pub fn new(
        graph: &ModuleGraph,
        declarations: &BTreeMap<String, Vec<Declaration>>,
    ) -> Result<Self, ModuleError> {
        if !graph.modules.contains_key("@entry") {
            return Err(error("E-MODULE-GRAPH", "missing entry module"));
        }
        if declarations.len() != graph.modules.len()
            || declarations
                .keys()
                .any(|key| !graph.modules.contains_key(key))
        {
            return Err(error(
                "E-MODULE-GRAPH",
                "declarations must cover exactly the loaded modules",
            ));
        }
        let mut scopes = BTreeMap::new();
        let mut next_symbol = 0usize;
        for (index, (name, module)) in graph.modules.iter().enumerate() {
            if module.name != *name {
                return Err(error(
                    "E-MODULE-GRAPH",
                    format!("module key {name} disagrees with its identity"),
                ));
            }
            let id = ModuleId(index);
            let mut symbols = HashMap::new();
            let mut sorted = declarations
                .get(name)
                .ok_or_else(|| error("E-MODULE-GRAPH", format!("missing declarations for {name}")))?
                .clone();
            sorted.sort_by(|a, b| {
                (a.kind.namespace() as u8, &a.name).cmp(&(b.kind.namespace() as u8, &b.name))
            });
            for decl in sorted {
                if !valid_identifier(&decl.name) {
                    return Err(error(
                        "E-MODULE-NAME",
                        format!("{name}: invalid declaration {:?}", decl.name),
                    ));
                }
                let space = decl.kind.namespace();
                if (space == Namespace::Value && reserved_value(&decl.name))
                    || (space == Namespace::Type && reserved_type(&decl.name))
                {
                    return Err(error(
                        "E-MODULE-NAME",
                        format!("{name}: reserved declaration {}", decl.name),
                    ));
                }
                let key = (space, decl.name.clone());
                let symbol = Symbol {
                    id: SymbolId(next_symbol),
                    owner: id,
                    name: decl.name,
                    kind: decl.kind,
                    exported: decl.exported,
                };
                next_symbol = next_symbol
                    .checked_add(1)
                    .ok_or_else(|| error("E-MODULE-LIMIT", "too many symbols"))?;
                if symbols.insert(key, symbol).is_some() {
                    return Err(error(
                        "E-MODULE-DUPLICATE",
                        format!("{name}: duplicate declaration"),
                    ));
                }
            }
            let mut aliases = HashMap::new();
            let mut imported = HashSet::new();
            for import in &module.imports {
                if !valid_identifier(&import.alias)
                    || reserved_value(&import.alias)
                    || reserved_type(&import.alias)
                {
                    return Err(error(
                        "E-MODULE-NAME",
                        format!("{name}: invalid or reserved import alias {}", import.alias),
                    ));
                }
                if !graph.modules.contains_key(&import.module) {
                    return Err(error(
                        "E-MODULE-GRAPH",
                        format!("{name}: missing imported module {}", import.module),
                    ));
                }
                if symbols.keys().any(|(_, symbol)| symbol == &import.alias) {
                    return Err(error(
                        "E-MODULE-DUPLICATE",
                        format!(
                            "{name}: import alias {} collides with a declaration",
                            import.alias
                        ),
                    ));
                }
                if !imported.insert(import.module.clone())
                    || aliases
                        .insert(import.alias.clone(), import.module.clone())
                        .is_some()
                {
                    return Err(error(
                        "E-MODULE-DUPLICATE",
                        format!("{name}: duplicate import or alias {}", import.alias),
                    ));
                }
            }
            scopes.insert(
                name.clone(),
                Scope {
                    id,
                    aliases,
                    symbols,
                },
            );
        }
        Ok(Self { scopes })
    }
    /// Resolve locally or through a single import alias; imported symbols must
    /// be exported. A local symbol is visible regardless of its export flag.
    pub fn resolve(
        &self,
        module: &str,
        alias: Option<&str>,
        name: &str,
        namespace: Namespace,
    ) -> Result<&Symbol, ModuleError> {
        let scope = self
            .scopes
            .get(module)
            .ok_or_else(|| error("E-MODULE-GRAPH", format!("unknown module {module}")))?;
        let (target, external) = if let Some(alias) = alias {
            let target = scope.aliases.get(alias).ok_or_else(|| {
                error("E-MODULE-ALIAS", format!("{module}: unknown alias {alias}"))
            })?;
            (
                self.scopes
                    .get(target)
                    .ok_or_else(|| error("E-MODULE-GRAPH", format!("missing module {target}")))?,
                true,
            )
        } else {
            (scope, false)
        };
        let symbol = target
            .symbols
            .get(&(namespace, name.to_string()))
            .ok_or_else(|| error("E-MODULE-NAME", format!("{module}: unknown symbol {name}")))?;
        if external && !symbol.exported {
            return Err(error(
                "E-MODULE-PRIVATE",
                format!("{module}: symbol {name} is private"),
            ));
        }
        Ok(symbol)
    }
    pub fn module_id(&self, module: &str) -> Option<ModuleId> {
        self.scopes.get(module).map(|scope| scope.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{Import, Module};
    use std::path::PathBuf;

    fn module(name: &str, imports: Vec<Import>) -> Module {
        Module {
            name: name.into(),
            path: PathBuf::from(name),
            source: String::new(),
            imports,
        }
    }
    fn import(module: &str, alias: &str) -> Import {
        Import {
            module: module.into(),
            alias: alias.into(),
            line: 1,
            column: 1,
        }
    }
    fn decl(name: &str, kind: Kind, exported: bool) -> Declaration {
        Declaration {
            name: name.into(),
            kind,
            exported,
        }
    }
    fn fixture() -> (ModuleGraph, BTreeMap<String, Vec<Declaration>>) {
        let graph = ModuleGraph {
            root: PathBuf::from("."),
            modules: BTreeMap::from([
                (
                    "@entry".into(),
                    module("@entry", vec![import("a", "left"), import("b", "right")]),
                ),
                ("a".into(), module("a", vec![])),
                ("b".into(), module("b", vec![])),
            ]),
        };
        let decls = BTreeMap::from([
            ("@entry".into(), vec![decl("main", Kind::Function, false)]),
            (
                "a".into(),
                vec![
                    decl("Box", Kind::Type, true),
                    decl("Box", Kind::Constructor, true),
                    decl("secret", Kind::Function, false),
                    decl("run", Kind::Function, true),
                ],
            ),
            (
                "b".into(),
                vec![
                    decl("Box", Kind::Type, true),
                    decl("run", Kind::Function, true),
                ],
            ),
        ]);
        (graph, decls)
    }
    #[test]
    fn qualified_lookup_preserves_identity_and_privacy() {
        let (graph, decls) = fixture();
        let resolver = Resolver::new(&graph, &decls).unwrap();
        let left = resolver
            .resolve("@entry", Some("left"), "Box", Namespace::Type)
            .unwrap();
        let right = resolver
            .resolve("@entry", Some("right"), "Box", Namespace::Type)
            .unwrap();
        assert_ne!(left.id, right.id);
        assert_ne!(left.owner, right.owner);
        assert_ne!(
            left.id,
            resolver
                .resolve("@entry", Some("left"), "Box", Namespace::Value)
                .unwrap()
                .id
        );
        assert_eq!(
            resolver
                .resolve("@entry", Some("left"), "secret", Namespace::Value)
                .unwrap_err()
                .code,
            "E-MODULE-PRIVATE"
        );
        assert!(resolver
            .resolve("a", None, "secret", Namespace::Value)
            .is_ok());
        assert_eq!(
            resolver
                .resolve("@entry", Some("missing"), "run", Namespace::Value)
                .unwrap_err()
                .code,
            "E-MODULE-ALIAS"
        );
        assert_eq!(
            resolver
                .resolve("@entry", None, "run", Namespace::Value)
                .unwrap_err()
                .code,
            "E-MODULE-NAME"
        );
        assert_eq!(
            resolver
                .resolve("@entry", Some("right"), "secret", Namespace::Value)
                .unwrap_err()
                .code,
            "E-MODULE-NAME"
        );
    }
    #[test]
    fn malformed_tables_aliases_and_duplicate_symbols_are_rejected() {
        let (mut graph, mut decls) = fixture();
        decls.remove("a");
        assert_eq!(
            Resolver::new(&graph, &decls).err().unwrap().code,
            "E-MODULE-GRAPH"
        );
        let (_, mut decls) = fixture();
        decls
            .get_mut("a")
            .unwrap()
            .push(decl("run", Kind::Constructor, true));
        assert_eq!(
            Resolver::new(&graph, &decls).err().unwrap().code,
            "E-MODULE-DUPLICATE"
        );
        let (_, mut decls) = fixture();
        decls
            .get_mut("@entry")
            .unwrap()
            .push(decl("left", Kind::Function, false));
        assert_eq!(
            Resolver::new(&graph, &decls).err().unwrap().code,
            "E-MODULE-DUPLICATE"
        );
        let (_, decls) = fixture();
        graph.modules.get_mut("@entry").unwrap().imports[0].module = "not_loaded".into();
        assert_eq!(
            Resolver::new(&graph, &decls).err().unwrap().code,
            "E-MODULE-GRAPH"
        );
    }
    #[test]
    fn symbol_ids_are_deterministic_for_reordered_declarations() {
        let (graph, mut decls) = fixture();
        let first = Resolver::new(&graph, &decls).unwrap();
        decls.get_mut("a").unwrap().reverse();
        let second = Resolver::new(&graph, &decls).unwrap();
        assert_eq!(
            first
                .resolve("@entry", Some("left"), "run", Namespace::Value)
                .unwrap()
                .id,
            second
                .resolve("@entry", Some("left"), "run", Namespace::Value)
                .unwrap()
                .id
        );
    }
}
