//! Source-backed module resolution into the existing checked compiler path.
//! The flattened names are internal and cannot occur in source identifiers.
use super::resolve::{Declaration, Kind, Namespace, Resolver, Symbol, SymbolId};
use super::{ModuleError, ModuleGraph};
use crate::ast::{AllocationTypes, Expr, Pattern, Program, TypeExpr};
use crate::check::Type;
use crate::parser::{self, ExportKind, ParsedModule};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

fn err(code: &'static str, message: impl Into<String>) -> ModuleError {
    ModuleError {
        code,
        message: message.into(),
    }
}
fn internal(symbol: &Symbol) -> String {
    format!("M${}${}", symbol.id.index(), symbol.name)
}
fn primitive(name: &str) -> bool {
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
    )
}
fn builtin(name: &str) -> bool {
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
fn qualify(name: &str) -> (Option<&str>, &str) {
    match name.split_once('.') {
        Some((alias, name)) => (Some(alias), name),
        None => (None, name),
    }
}

/// Load sources, resolve declarations and check one flattened program.
/// Import paths and source snapshots come only from the bounded graph loader.
pub fn load_program(entry: &Path) -> Result<Program, ModuleError> {
    lower(&super::load(entry)?)
}

fn lower(graph: &ModuleGraph) -> Result<Program, ModuleError> {
    let mut parsed = BTreeMap::<String, ParsedModule>::new();
    let mut catalogue = BTreeMap::new();
    for (module, source) in &graph.modules {
        let unit = parser::parse_module(&source.source)
            .map_err(|e| err("E-MODULE-PARSE", format!("{}:{e}", source.path.display())))?;
        if module != "@entry" && unit.program.functions.iter().any(|f| f.name == "main") {
            return Err(err(
                "E-MODULE-ENTRY",
                format!(
                    "{}: imported modules cannot declare main",
                    source.path.display()
                ),
            ));
        }
        let exports: HashSet<_> = unit
            .exports
            .iter()
            .map(|e| (e.name.as_str(), e.kind))
            .collect();
        if exports.len() != unit.exports.len() {
            return Err(err(
                "E-MODULE-DUPLICATE",
                format!("{}: duplicate export", source.path.display()),
            ));
        }
        let mut decls = Vec::new();
        for ty in &unit.program.types {
            let exported = exports.contains(&(ty.name.as_str(), ExportKind::Type));
            decls.push(Declaration {
                name: ty.name.clone(),
                kind: Kind::Type,
                exported,
            });
            for v in &ty.variants {
                decls.push(Declaration {
                    name: v.name.clone(),
                    kind: Kind::Constructor,
                    exported,
                });
            }
        }
        for f in &unit.program.functions {
            decls.push(Declaration {
                name: f.name.clone(),
                kind: Kind::Function,
                exported: exports.contains(&(f.name.as_str(), ExportKind::Function)),
            });
        }
        for f in &unit.program.externs {
            decls.push(Declaration {
                name: f.name.clone(),
                kind: Kind::Function,
                exported: exports.contains(&(f.name.as_str(), ExportKind::Function)),
            });
        }
        for constant in &unit.program.constants {
            decls.push(Declaration {
                name: constant.name.clone(),
                kind: Kind::Constant,
                exported: exports.contains(&(constant.name.as_str(), ExportKind::Constant)),
            });
        }
        catalogue.insert(module.clone(), decls);
        parsed.insert(module.clone(), unit);
    }
    let resolver = Resolver::new(graph, &catalogue)?;
    let mut foreign_names = HashMap::<SymbolId, String>::new();
    let mut output = Program {
        allocation_types: None,
        native: crate::c_import::Config::default(),
        constants: vec![],
        types: vec![],
        externs: vec![],
        functions: vec![],
    };
    let mut exported_functions = Vec::<String>::new();
    let mut exported_types = HashSet::<String>::new();
    let mut private_types = HashSet::<String>::new();
    let mut function_indices = HashMap::<String, usize>::new();
    let mut constant_indices = HashMap::<String, usize>::new();
    // Resolve every foreign declaration by identity before rewriting any caller.
    // Raw C names are global, so only exactly matching ABI declarations coalesce.
    for (module, unit) in &parsed {
        for f in &unit.program.externs {
            crate::check::validate_extern_function(f).map_err(|e| err("E-MODULE-CHECK", e))?;
            let symbol = resolver.resolve(module, None, &f.name, Namespace::Value)?;
            foreign_names.insert(symbol.id, f.name.clone());
            if constant_indices.contains_key(&f.name) {
                return Err(err(
                    "E-MODULE-EXTERN",
                    format!("conflicting foreign declaration {}", f.name),
                ));
            }
            if let Some(&index) = function_indices.get(&f.name) {
                let previous = &output.externs[index];
                if previous.result != f.result
                    || previous
                        .params
                        .iter()
                        .map(|p| &p.ty)
                        .ne(f.params.iter().map(|p| &p.ty))
                {
                    return Err(err(
                        "E-MODULE-EXTERN",
                        format!("conflicting foreign signature {}", f.name),
                    ));
                }
            } else {
                function_indices.insert(f.name.clone(), output.externs.len());
                output.externs.push(f.clone());
            }
        }
        for c in &unit.program.constants {
            crate::check::validate_extern_constant(c).map_err(|e| err("E-MODULE-CHECK", e))?;
            let symbol = resolver.resolve(module, None, &c.name, Namespace::Value)?;
            foreign_names.insert(symbol.id, c.name.clone());
            if function_indices.contains_key(&c.name) {
                return Err(err(
                    "E-MODULE-EXTERN",
                    format!("conflicting foreign declaration {}", c.name),
                ));
            }
            if let Some(&index) = constant_indices.get(&c.name) {
                let previous = &output.constants[index];
                if previous.ty != c.ty {
                    return Err(err(
                        "E-MODULE-EXTERN",
                        format!("conflicting foreign signature {}", c.name),
                    ));
                }
            } else {
                constant_indices.insert(c.name.clone(), output.constants.len());
                output.constants.push(c.clone());
            }
        }
    }
    for (module, unit) in &parsed {
        for ty in &unit.program.types {
            let s = resolver.resolve(module, None, &ty.name, Namespace::Type)?;
            let name = internal(s);
            if s.exported {
                exported_types.insert(name);
            } else {
                private_types.insert(name);
            }
        }
    }
    for (module, unit) in &parsed {
        let mut rewrite = Rewrite {
            resolver: &resolver,
            module,
            locals: HashMap::new(),
            foreign_names: &foreign_names,
        };
        for ty in &unit.program.types {
            let mut ty = ty.clone();
            let name = internal(resolver.resolve(module, None, &ty.name, Namespace::Type)?);
            let params: HashSet<_> = ty.params.iter().cloned().collect();
            for variant in &mut ty.variants {
                variant.name =
                    internal(resolver.resolve(module, None, &variant.name, Namespace::Value)?);
                for field in &mut variant.fields {
                    rewrite.ty(field, &params)?;
                }
            }
            ty.name = name;
            output.types.push(ty);
        }
        output
            .native
            .includes
            .extend(unit.program.native.includes.iter().cloned());
        output
            .native
            .links
            .extend(unit.program.native.links.iter().cloned());
        output
            .native
            .packages
            .extend(unit.program.native.packages.iter().cloned());
        for function in &unit.program.functions {
            let symbol = resolver.resolve(module, None, &function.name, Namespace::Value)?;
            let mut f = function.clone();
            f.name = if module == "@entry" && f.name == "main" {
                "main".into()
            } else {
                internal(symbol)
            };
            if symbol.exported {
                exported_functions.push(f.name.clone());
            }
            for p in &mut f.params {
                if let Some(annotation) = &mut p.annotation {
                    rewrite.ty(annotation, &HashSet::new())?;
                }
            }
            for p in &f.params {
                rewrite.push(&p.name);
            }
            rewrite.expr(&mut f.body)?;
            for p in &f.params {
                rewrite.pop(&p.name);
            }
            output.functions.push(f);
        }
    }
    if graph.modules.contains_key("std.alloc") {
        // Only bundled source snapshots establish canonical allocation identities.
        for module in ["std.alloc", "std.result"] {
            let source = super::stdlib::source(module)
                .ok_or_else(|| err("E-MODULE-ALLOC", format!("missing bundled module {module}")))?;
            if graph
                .modules
                .get(module)
                .is_none_or(|loaded| loaded.source != source)
            {
                return Err(err(
                    "E-MODULE-ALLOC",
                    format!("{module} is not bundled source"),
                ));
            }
        }
        let identity = |module: &str, name: &str, kind: Kind| -> Result<String, ModuleError> {
            let space = if kind == Kind::Type {
                Namespace::Type
            } else {
                Namespace::Value
            };
            let symbol = resolver.resolve(module, None, name, space)?;
            if symbol.kind != kind || !symbol.exported {
                return Err(err(
                    "E-MODULE-ALLOC",
                    format!("invalid bundled declaration {module}.{name}"),
                ));
            }
            Ok(internal(symbol))
        };
        output.allocation_types = Some(AllocationTypes {
            result_type: identity("std.result", "Result", Kind::Type)?,
            ok_constructor: identity("std.result", "Ok", Kind::Constructor)?,
            err_constructor: identity("std.result", "Err", Kind::Constructor)?,
            error_type: identity("std.alloc", "AllocError", Kind::Type)?,
            invalid_length_constructor: identity("std.alloc", "InvalidLength", Kind::Constructor)?,
            size_overflow_constructor: identity("std.alloc", "SizeOverflow", Kind::Constructor)?,
            out_of_memory_constructor: identity("std.alloc", "OutOfMemory", Kind::Constructor)?,
        });
    }
    // Exporting a type exposes its constructors and their field types.
    for ty in &output.types {
        if exported_types.contains(&ty.name) {
            for v in &ty.variants {
                for field in &v.fields {
                    if contains_private_expr(field, &private_types) {
                        return Err(err(
                            "E-MODULE-PRIVATE-TYPE",
                            format!("exported type {} exposes a private type", ty.name),
                        ));
                    }
                }
            }
        }
    }
    let checked = crate::check::analyze(&output).map_err(|e| err("E-MODULE-CHECK", e))?;
    for name in exported_functions {
        let sig = &checked.functions[&name];
        if sig
            .params
            .iter()
            .chain(std::iter::once(&sig.result))
            .any(|t| contains_private_type(t, &private_types))
        {
            return Err(err(
                "E-MODULE-PRIVATE-TYPE",
                format!("exported function {name} exposes a private type"),
            ));
        }
    }
    Ok(output)
}
fn contains_private_expr(t: &TypeExpr, private: &HashSet<String>) -> bool {
    private.contains(&t.name) || t.args.iter().any(|t| contains_private_expr(t, private))
}
fn contains_private_type(t: &Type, private: &HashSet<String>) -> bool {
    match t {
        Type::Nominal(name, args) => {
            private.contains(name) || args.iter().any(|t| contains_private_type(t, private))
        }
        _ => false,
    }
}

struct Rewrite<'a> {
    resolver: &'a Resolver,
    module: &'a str,
    locals: HashMap<String, usize>,
    foreign_names: &'a HashMap<SymbolId, String>,
}
impl Rewrite<'_> {
    fn push(&mut self, name: &str) {
        *self.locals.entry(name.into()).or_default() += 1;
    }
    fn pop(&mut self, name: &str) {
        let n = self.locals.get_mut(name).expect("balanced scope");
        *n -= 1;
        if *n == 0 {
            self.locals.remove(name);
        }
    }
    fn symbol(&self, name: &str, space: Namespace) -> Result<&Symbol, ModuleError> {
        let (alias, name) = qualify(name);
        self.resolver.resolve(self.module, alias, name, space)
    }
    fn value_name(&self, name: &str) -> Result<String, ModuleError> {
        let s = self.symbol(name, Namespace::Value)?;
        if let Some(raw) = self.foreign_names.get(&s.id) {
            return Ok(raw.clone());
        }
        if self.module == "@entry" && !name.contains('.') && name == "main" {
            return Ok("main".into());
        }
        Ok(internal(s))
    }
    fn ty(&self, ty: &mut TypeExpr, params: &HashSet<String>) -> Result<(), ModuleError> {
        if matches!(ty.name.as_str(), "ptr" | "const_ptr") && !params.contains(&ty.name) {
            // Pointees name C types from headers, not declarations in a Rill module.
            // The checker validates their spelling and arity.
            return Ok(());
        }
        if !params.contains(&ty.name) && !primitive(&ty.name) {
            ty.name = internal(self.symbol(&ty.name, Namespace::Type)?)
        }
        for arg in &mut ty.args {
            self.ty(arg, params)?
        }
        Ok(())
    }
    fn pattern(&self, p: &mut Pattern, bindings: &mut Vec<String>) -> Result<(), ModuleError> {
        match p {
            Pattern::Binding(n) => bindings.push(n.clone()),
            Pattern::Constructor(n, args) => {
                let s = self.symbol(n, Namespace::Value)?;
                if s.kind != Kind::Constructor {
                    return Err(err(
                        "E-MODULE-NAME",
                        format!("{}: {n} is not a constructor", self.module),
                    ));
                }
                *n = internal(s);
                for p in args {
                    self.pattern(p, bindings)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn expr(&mut self, e: &mut Expr) -> Result<(), ModuleError> {
        if let Expr::Field(root, fields) = e {
            if !self.locals.contains_key(root) {
                if fields.len() != 1 {
                    return Err(err(
                        "E-MODULE-NAME",
                        format!("{}: qualified names may have only one member", self.module),
                    ));
                }
                let name = format!("{root}.{}", fields[0]);
                let symbol = self.symbol(&name, Namespace::Value)?;
                if !matches!(symbol.kind, Kind::Constructor | Kind::Constant) {
                    return Err(err(
                        "E-MODULE-NAME",
                        format!("{}: {name} is not a constant or constructor", self.module),
                    ));
                }
                *e = Expr::Var(self.value_name(&name)?);
                return Ok(());
            }
        }
        match e {
            Expr::Var(n) => {
                if !self.locals.contains_key(n) {
                    let s = self.symbol(n, Namespace::Value)?;
                    if !matches!(s.kind, Kind::Constructor | Kind::Constant) {
                        return Err(err(
                            "E-MODULE-NAME",
                            format!(
                                "{}: {n} is not a local, constant or constructor",
                                self.module
                            ),
                        ));
                    }
                    *n = self.value_name(n)?;
                }
            }
            Expr::Let(n, value, body) | Expr::LetMut(n, value, body) => {
                self.expr(value)?;
                self.push(n);
                let r = self.expr(body);
                self.pop(n);
                r?;
            }
            Expr::Assign(_, value) => self.expr(value)?,
            Expr::FieldAssign(_, _, value) => self.expr(value)?,
            Expr::Field(_, _) => {}
            Expr::Record(name, fields) => {
                *name = internal(self.symbol(name, Namespace::Type)?);
                for (_, value) in fields {
                    self.expr(value)?;
                }
            }
            Expr::Binary(_, a, b) | Expr::While(a, b) => {
                self.expr(a)?;
                self.expr(b)?
            }
            Expr::If(c, a, b) => {
                self.expr(c)?;
                self.expr(a)?;
                self.expr(b)?
            }
            Expr::Call(n, args) => {
                if !builtin(n) {
                    *n = self.value_name(n)?
                }
                for a in args {
                    self.expr(a)?
                }
            }
            Expr::Match(value, arms) => {
                self.expr(value)?;
                for arm in arms {
                    let mut bindings = Vec::new();
                    self.pattern(&mut arm.pattern, &mut bindings)?;
                    for n in &bindings {
                        self.push(n)
                    }
                    let r = self.expr(&mut arm.body);
                    for n in bindings.iter().rev() {
                        self.pop(n)
                    }
                    r?;
                }
            }
            Expr::Int(_)
            | Expr::Bool(_)
            | Expr::Unit
            | Expr::Str(_)
            | Expr::CStr(_)
            | Expr::Move(_)
            | Expr::Inout(_)
            | Expr::InoutField(_, _) => {}
        }
        Ok(())
    }
}
