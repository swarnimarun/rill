//! Source-backed module resolution into the existing checked compiler path.
//! The flattened names are internal and cannot occur in source identifiers.
use super::provenance::{self, Contribution, Lowering, SourceId, SourceProgram, Trace};
use super::resolve::{Declaration, Kind, Namespace, Resolver, Symbol, SymbolId};
use super::{ModuleError, ModuleGraph};
use crate::ast::{AllocationTypes, Expr, Pattern, Program, TypeExpr};
use crate::check::Type;
use crate::parser::{self, ExportKind, ParsedModule};
use crate::source_map::{AstPath, AstStep};
use crate::syntax::AstDeclaration;
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

/// One checked program and the exact immutable source graph used to lower it.
/// This small snapshot seam does not construct optional origin indexes.
pub struct ProgramSnapshot {
    program: Program,
    graph: ModuleGraph,
}
impl ProgramSnapshot {
    pub fn program(&self) -> &Program {
        &self.program
    }
    pub fn source(&self, logical_name: &str) -> Option<&str> {
        self.graph
            .modules
            .get(logical_name)
            .map(|module| module.source.as_str())
    }
}

/// Load each source through the ordinary bounded graph loader exactly once,
/// check that graph, then retain it for byte-identity comparisons. Callers must
/// not reread a path to establish the identity of this program's source.
pub fn load_program_snapshot(entry: &Path) -> Result<ProgramSnapshot, ModuleError> {
    let graph = super::load(entry)?;
    let program = lower(&graph)?;
    Ok(ProgramSnapshot { program, graph })
}

/// Load/check through the same bounded loader and lowering pass, then preserve
/// owned origins for the exact flattened program. Native ABI validation is still
/// the driver's separate responsibility. Metadata errors use E-MODULE-SOURCE-MAP;
/// ordinary load_program never constructs metadata and cannot fail for that reason.
pub fn load_program_with_sources(entry: &Path) -> Result<SourceProgram, ModuleError> {
    let graph = super::load(entry)?;
    let mut trace = Trace::default();
    let program = lower_traced(&graph, Some(&mut trace))?;
    provenance::finish(graph, program, trace)
}

/// Validate generated bindings as an imported library, not an entry program.
/// The synthetic entry is only a checking harness and is never published.
/// This checks Rill syntax/types, not the target C ABI, and performs no I/O.
#[cfg(feature = "c-import")]
pub fn check_binding_source(source: &str) -> Result<Program, ModuleError> {
    let entry = "import bindings; fn main() { 0 }";
    let mut modules = BTreeMap::new();
    for (name, text) in [("@entry", entry), ("bindings", source)] {
        modules.insert(
            name.to_owned(),
            super::Module {
                name: name.to_owned(),
                path: format!("<{name}.rill>").into(),
                source: text.to_owned(),
                imports: if name == "@entry" {
                    vec![super::Import {
                        module: "bindings".into(),
                        alias: "bindings".into(),
                        line: 1,
                        column: 1,
                    }]
                } else {
                    vec![]
                },
            },
        );
    }
    lower(&ModuleGraph {
        root: ".".into(),
        modules,
    })
}

fn lower(graph: &ModuleGraph) -> Result<Program, ModuleError> {
    lower_traced(graph, None)
}

fn lower_traced(
    graph: &ModuleGraph,
    mut trace: Option<&mut Trace>,
) -> Result<Program, ModuleError> {
    let mut parsed = BTreeMap::<String, ParsedModule>::new();
    let mut catalogue = BTreeMap::new();
    let mut annotations = crate::check::annotations::Budget::default();
    for (module, source) in &graph.modules {
        let unit = parser::parse_module(&source.source)
            .map_err(|e| err("E-MODULE-PARSE", format!("{}:{e}", source.path.display())))?;
        annotations
            .program(&unit.program)
            .map_err(|e| err("E-MODULE-CHECK", e))?;
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
    preflight_resolved_annotations(
        &parsed,
        &resolver,
        &mut crate::check::annotations::Budget::default(),
    )?;
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
    for (source_index, (module, unit)) in parsed.iter().enumerate() {
        let source = SourceId(source_index);
        for (original, f) in unit.program.externs.iter().enumerate() {
            crate::check::validate_extern_function(f).map_err(|e| err("E-MODULE-CHECK", e))?;
            let symbol = resolver.resolve(module, None, &f.name, Namespace::Value)?;
            foreign_names.insert(symbol.id, f.name.clone());
            if constant_indices.contains_key(&f.name) {
                return Err(err(
                    "E-MODULE-EXTERN",
                    format!("conflicting foreign declaration {}", f.name),
                ));
            }
            let (index, contribution) = if let Some(&index) = function_indices.get(&f.name) {
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
                (index, Contribution::CoalescedNative)
            } else {
                let index = output.externs.len();
                function_indices.insert(f.name.clone(), index);
                output.externs.push(f.clone());
                (index, Contribution::Retained)
            };
            if let Some(trace) = trace.as_deref_mut() {
                trace.transfer(
                    source,
                    AstDeclaration::ExternFunction(original),
                    AstDeclaration::ExternFunction(index),
                    contribution,
                    symbol,
                );
            }
        }
        for (original, c) in unit.program.constants.iter().enumerate() {
            crate::check::validate_extern_constant(c).map_err(|e| err("E-MODULE-CHECK", e))?;
            let symbol = resolver.resolve(module, None, &c.name, Namespace::Value)?;
            foreign_names.insert(symbol.id, c.name.clone());
            if function_indices.contains_key(&c.name) {
                return Err(err(
                    "E-MODULE-EXTERN",
                    format!("conflicting foreign declaration {}", c.name),
                ));
            }
            let (index, contribution) = if let Some(&index) = constant_indices.get(&c.name) {
                let previous = &output.constants[index];
                if previous.ty != c.ty {
                    return Err(err(
                        "E-MODULE-EXTERN",
                        format!("conflicting foreign signature {}", c.name),
                    ));
                }
                (index, Contribution::CoalescedNative)
            } else {
                let index = output.constants.len();
                constant_indices.insert(c.name.clone(), index);
                output.constants.push(c.clone());
                (index, Contribution::Retained)
            };
            if let Some(trace) = trace.as_deref_mut() {
                trace.transfer(
                    source,
                    AstDeclaration::Constant(original),
                    AstDeclaration::Constant(index),
                    contribution,
                    symbol,
                );
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
    for (source_index, (module, unit)) in parsed.iter().enumerate() {
        let mut rewrite = Rewrite {
            resolver: &resolver,
            module,
            locals: HashMap::new(),
            foreign_names: &foreign_names,
            source: SourceId(source_index),
            trace: trace.as_deref_mut(),
            path: AstPath::root(AstDeclaration::Type(0)),
        };
        for (original, ty) in unit.program.types.iter().enumerate() {
            let mut ty = ty.clone();
            let symbol = resolver.resolve(module, None, &ty.name, Namespace::Type)?;
            let name = internal(symbol);
            let target = AstDeclaration::Type(output.types.len());
            rewrite.declaration(AstDeclaration::Type(original), target, symbol);
            let params: HashSet<_> = ty.params.iter().cloned().collect();
            for (index, variant) in ty.variants.iter_mut().enumerate() {
                let symbol = resolver.resolve(module, None, &variant.name, Namespace::Value)?;
                variant.name = internal(symbol);
                rewrite.at(AstStep::Variant(index), |rewrite| {
                    if let Some(trace) = rewrite.trace.as_deref_mut() {
                        trace.symbol(
                            rewrite.source,
                            rewrite.path.clone(),
                            AstPath::root(target).child(AstStep::Variant(index)),
                            symbol,
                        );
                    }
                    rewrite.record(Lowering::ResolvedName(symbol.id));
                    for (index, field) in variant.fields.iter_mut().enumerate() {
                        rewrite.at(AstStep::FieldType(index), |rewrite| {
                            rewrite.ty(field, &params)
                        })?;
                    }
                    Ok(())
                })?;
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
        for (original, function) in unit.program.functions.iter().enumerate() {
            let symbol = resolver.resolve(module, None, &function.name, Namespace::Value)?;
            let mut f = function.clone();
            f.name = if module == "@entry" && f.name == "main" {
                "main".into()
            } else {
                internal(symbol)
            };
            rewrite.declaration(
                AstDeclaration::Function(original),
                AstDeclaration::Function(output.functions.len()),
                symbol,
            );
            if symbol.exported {
                exported_functions.push(f.name.clone());
            }
            for (index, p) in f.params.iter_mut().enumerate() {
                if let Some(annotation) = &mut p.annotation {
                    rewrite.at(AstStep::Parameter(index), |rewrite| {
                        rewrite.at(AstStep::Annotation, |rewrite| {
                            rewrite.ty(annotation, &HashSet::new())
                        })
                    })?;
                }
            }
            if let Some(annotation) = &mut f.result_annotation {
                rewrite.at(AstStep::FunctionResult, |rewrite| {
                    rewrite.ty(annotation, &HashSet::new())
                })?;
            }
            for p in &f.params {
                rewrite.push(&p.name);
            }
            rewrite.at(AstStep::FunctionBody, |rewrite| rewrite.expr(&mut f.body))?;
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
// Compute exact final annotation name lengths without allocating rewritten
// names or cloning trees. The same allowance spans every parsed module.
fn preflight_resolved_annotations(
    parsed: &BTreeMap<String, ParsedModule>,
    resolver: &Resolver,
    budget: &mut crate::check::annotations::Budget,
) -> Result<(), ModuleError> {
    for (module, unit) in parsed {
        budget.program_named(
            &unit.program,
            |name| {
                if primitive(name) {
                    return Ok(name.len());
                }
                let (alias, name) = qualify(name);
                let symbol = resolver.resolve(module, alias, name, Namespace::Type)?;
                let digits = symbol.id.index().checked_ilog10().unwrap_or(0) as usize + 1;
                symbol
                    .name
                    .len()
                    .checked_add(digits + 3)
                    .ok_or_else(annotation_limit)
            },
            annotation_limit,
        )?;
    }
    Ok(())
}
fn annotation_limit() -> ModuleError {
    // Parameter and result annotations share this admission diagnostic.
    err(
        "E-MODULE-CHECK",
        "E-TYPE-COMPLEXITY: signature annotation input budget exceeded",
    )
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
    source: SourceId,
    trace: Option<&'a mut Trace>,
    path: AstPath,
}
impl Rewrite<'_> {
    fn declaration(&mut self, from: AstDeclaration, to: AstDeclaration, symbol: &Symbol) {
        if let Some(trace) = self.trace.as_deref_mut() {
            self.path = AstPath::root(from);
            trace.transfer(self.source, from, to, Contribution::Retained, symbol);
        }
        self.record(Lowering::ResolvedName(symbol.id));
    }
    fn record(&mut self, change: Lowering) {
        if let Some(trace) = self.trace.as_deref_mut() {
            trace.change(self.source, self.path.clone(), change);
        }
    }
    fn at<T>(
        &mut self,
        step: AstStep,
        action: impl FnOnce(&mut Self) -> Result<T, ModuleError>,
    ) -> Result<T, ModuleError> {
        if self.trace.is_some() {
            self.path.steps.push(step);
        }
        let result = action(self);
        if self.trace.is_some() {
            self.path.steps.pop();
        }
        result
    }
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
    fn ty(&mut self, ty: &mut TypeExpr, params: &HashSet<String>) -> Result<(), ModuleError> {
        if matches!(ty.name.as_str(), "ptr" | "const_ptr") && !params.contains(&ty.name) {
            // Pointees name C types from headers, not declarations in a Rill module.
            // The checker validates their spelling and arity.
            return Ok(());
        }
        if !params.contains(&ty.name) && !primitive(&ty.name) {
            let symbol = self.symbol(&ty.name, Namespace::Type)?;
            let id = symbol.id;
            ty.name = internal(symbol);
            self.record(Lowering::ResolvedName(id));
        }
        for (index, arg) in ty.args.iter_mut().enumerate() {
            self.at(AstStep::TypeArgument(index), |rewrite| {
                rewrite.ty(arg, params)
            })?;
        }
        Ok(())
    }
    fn pattern(&mut self, p: &mut Pattern, bindings: &mut Vec<String>) -> Result<(), ModuleError> {
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
                let id = s.id;
                *n = internal(s);
                self.record(Lowering::ResolvedName(id));
                for (index, p) in args.iter_mut().enumerate() {
                    self.at(AstStep::PatternField(index), |rewrite| {
                        rewrite.pattern(p, bindings)
                    })?;
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
                let id = symbol.id;
                *e = Expr::Var(self.value_name(&name)?);
                self.record(Lowering::QualifiedValue(id));
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
                    let id = s.id;
                    *n = self.value_name(n)?;
                    self.record(Lowering::ResolvedName(id));
                }
            }
            Expr::Let(n, value, body) | Expr::LetMut(n, value, body) => {
                self.at(AstStep::LetValue, |rewrite| rewrite.expr(value))?;
                self.push(n);
                let r = self.at(AstStep::LetBody, |rewrite| rewrite.expr(body));
                self.pop(n);
                r?;
            }
            Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => {
                self.at(AstStep::AssignValue, |rewrite| rewrite.expr(value))?;
            }
            Expr::Field(_, _) => {}
            Expr::Record(name, fields) => {
                let symbol = self.symbol(name, Namespace::Type)?;
                let id = symbol.id;
                *name = internal(symbol);
                self.record(Lowering::ResolvedName(id));
                for (index, (_, value)) in fields.iter_mut().enumerate() {
                    self.at(AstStep::RecordField(index), |rewrite| rewrite.expr(value))?;
                }
            }
            Expr::Binary(_, a, b) => {
                self.at(AstStep::BinaryLeft, |rewrite| rewrite.expr(a))?;
                self.at(AstStep::BinaryRight, |rewrite| rewrite.expr(b))?;
            }
            Expr::While(a, b) => {
                self.at(AstStep::Condition, |rewrite| rewrite.expr(a))?;
                self.at(AstStep::WhileBody, |rewrite| rewrite.expr(b))?;
            }
            Expr::If(c, a, b) => {
                self.at(AstStep::Condition, |rewrite| rewrite.expr(c))?;
                self.at(AstStep::Then, |rewrite| rewrite.expr(a))?;
                self.at(AstStep::Else, |rewrite| rewrite.expr(b))?;
            }
            Expr::Call(n, args) => {
                if !builtin(n) {
                    let id = self.symbol(n, Namespace::Value)?.id;
                    *n = self.value_name(n)?;
                    self.record(Lowering::ResolvedName(id));
                }
                for (index, a) in args.iter_mut().enumerate() {
                    self.at(AstStep::Argument(index), |rewrite| rewrite.expr(a))?;
                }
            }
            Expr::Match(value, arms) => {
                self.at(AstStep::MatchValue, |rewrite| rewrite.expr(value))?;
                for (index, arm) in arms.iter_mut().enumerate() {
                    self.at(AstStep::Arm(index), |rewrite| {
                        let mut bindings = Vec::new();
                        rewrite.at(AstStep::ArmPattern, |rewrite| {
                            rewrite.pattern(&mut arm.pattern, &mut bindings)
                        })?;
                        for n in &bindings {
                            rewrite.push(n)
                        }
                        let r = rewrite.at(AstStep::ArmBody, |rewrite| rewrite.expr(&mut arm.body));
                        for n in bindings.iter().rev() {
                            rewrite.pop(n)
                        }
                        r
                    })?;
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

#[cfg(test)]
mod result_annotation_budget_tests {
    use super::*;
    fn fixture(source: &str, modules: &[&str]) -> (BTreeMap<String, ParsedModule>, Resolver) {
        let mut graph = ModuleGraph {
            root: ".".into(),
            modules: BTreeMap::new(),
        };
        let mut parsed = BTreeMap::<String, ParsedModule>::new();
        let mut catalogue = BTreeMap::new();
        for &module in modules {
            graph.modules.insert(
                module.into(),
                super::super::Module {
                    name: module.into(),
                    path: format!("{module}.rill").into(),
                    source: source.into(),
                    imports: vec![],
                },
            );
            parsed.insert(module.into(), parser::parse_module(source).unwrap());
            catalogue.insert(
                module.into(),
                vec![
                    Declaration {
                        name: "T".into(),
                        kind: Kind::Type,
                        exported: false,
                    },
                    Declaration {
                        name: "Tag".into(),
                        kind: Kind::Constructor,
                        exported: false,
                    },
                    Declaration {
                        name: "value".into(),
                        kind: Kind::Function,
                        exported: false,
                    },
                ],
            );
        }
        let resolver = Resolver::new(&graph, &catalogue).unwrap();
        (parsed, resolver)
    }

    #[test]
    fn graph_wide_resolved_annotation_weight_precedes_cloning() {
        let (parsed, resolver) =
            fixture("type T = Tag\nfn value() -> T {Tag}", &["@entry", "other"]);
        let cost: usize = parsed
            .keys()
            .map(|module| {
                64 + internal(
                    resolver
                        .resolve(module, None, "T", Namespace::Type)
                        .unwrap(),
                )
                .len()
            })
            .sum();
        let mut exact = crate::check::annotations::Budget::with_limits(cost, 2);
        preflight_resolved_annotations(&parsed, &resolver, &mut exact).unwrap();
        let mut short = crate::check::annotations::Budget::with_limits(cost - 1, 2);
        assert!(
            preflight_resolved_annotations(&parsed, &resolver, &mut short)
                .unwrap_err()
                .message
                .starts_with("E-TYPE-COMPLEXITY")
        );
        let mut original = crate::check::annotations::Budget::with_limits(2 * 65, 2);
        for unit in parsed.values() {
            original.program(&unit.program).unwrap();
        }
        let mut rewritten = crate::check::annotations::Budget::with_limits(2 * 65, 2);
        assert!(
            preflight_resolved_annotations(&parsed, &resolver, &mut rewritten).is_err(),
            "renaming growth must be budgeted before cloning"
        );
    }

    #[test]
    fn parameter_and_mixed_graph_passes_charge_exact_resolved_names_and_work() {
        for source in [
            "type T = Tag\nlet value (x: T) = x",
            "type T = Tag\nfn value(x: T) -> T {x}",
        ] {
            // Enough declarations to cross a symbol-ID decimal digit boundary.
            let (parsed, resolver) = fixture(source, &["@entry", "a", "b", "c", "d"]);
            let mut cost = 0;
            let mut original_cost = 0;
            let mut nodes = 0;
            for (module, unit) in &parsed {
                let function = &unit.program.functions[0];
                let count = 1 + usize::from(function.result_annotation.is_some());
                let symbol = resolver
                    .resolve(module, None, "T", Namespace::Type)
                    .unwrap();
                cost += count * (64 + internal(symbol).len());
                original_cost += count * 65;
                nodes += count;
            }
            assert!(cost > original_cost);
            let mut original = crate::check::annotations::Budget::with_limits(original_cost, nodes);
            for unit in parsed.values() {
                original.program(&unit.program).unwrap();
            }
            // Independent complete allowances, each shared across the graph.
            let mut exact = crate::check::annotations::Budget::with_limits(cost, nodes);
            preflight_resolved_annotations(&parsed, &resolver, &mut exact).unwrap();
            for (weight, work) in [(cost - 1, nodes), (cost, nodes - 1), (original_cost, nodes)] {
                let mut short = crate::check::annotations::Budget::with_limits(weight, work);
                let error =
                    preflight_resolved_annotations(&parsed, &resolver, &mut short).unwrap_err();
                assert_eq!(error.code, "E-MODULE-CHECK");
                assert_eq!(
                    error.message,
                    "E-TYPE-COMPLEXITY: signature annotation input budget exceeded"
                );
            }
            for (weight, work) in [(original_cost - 1, nodes), (original_cost, nodes - 1)] {
                let mut short = crate::check::annotations::Budget::with_limits(weight, work);
                let error = parsed
                    .values()
                    .try_for_each(|unit| short.program(&unit.program))
                    .unwrap_err();
                assert_eq!(
                    error,
                    "E-TYPE-COMPLEXITY: signature annotation input budget exceeded"
                );
            }
        }
    }

    #[test]
    fn resolved_parameter_pass_counts_malformed_raw_trees_without_rewriting() {
        let (mut parsed, resolver) = fixture(
            "type T = Tag\nfn value(x: T) -> T {x}",
            &["@entry", "other"],
        );
        let mut cost = 0;
        for (module, unit) in &mut parsed {
            let raw = TypeExpr {
                name: "const_ptr".into(),
                args: vec![
                    TypeExpr {
                        name: "not_a_module.Missing".into(),
                        args: vec![TypeExpr {
                            name: "T".into(),
                            args: vec![],
                        }],
                    },
                    TypeExpr {
                        name: "éλ".into(),
                        args: vec![],
                    },
                ],
            };
            cost += 4 * 64 + "const_ptr".len() + "not_a_module.Missing".len() + 1 + 4;
            cost += 64
                + internal(
                    resolver
                        .resolve(module, None, "T", Namespace::Type)
                        .unwrap(),
                )
                .len();
            unit.program.functions[0].params[0].annotation = Some(raw);
        }
        let mut exact = crate::check::annotations::Budget::with_limits(cost, 10);
        preflight_resolved_annotations(&parsed, &resolver, &mut exact).unwrap();
        for (weight, work) in [(cost - 1, 10), (cost, 9)] {
            let mut short = crate::check::annotations::Budget::with_limits(weight, work);
            assert_eq!(
                preflight_resolved_annotations(&parsed, &resolver, &mut short)
                    .unwrap_err()
                    .message,
                "E-TYPE-COMPLEXITY: signature annotation input budget exceeded"
            );
        }
    }
}
