//! Name resolution: desugared AST -> [`Hir`] (PLAN.md §7.1/§7.2, M1).
//!
//! Two passes over the module:
//!
//! 1. **Declaration pass** — every module-level item gets a slot and its name
//!    is registered in the value or the type namespace, so items are visible
//!    regardless of source order (mutual recursion, builders, imports).
//! 2. **Body pass** — bodies are lowered into [`crate::hir`] nodes, binding
//!    parameters, locals and pattern variables into lexical scopes and
//!    resolving every path against them.
//!
//! Resolution rules:
//!
//! * A one-segment path checks locals, then block-scoped items, then module
//!   values, then module types (a type name in value position is its
//!   constructor), then `use` imports.
//! * A qualified path resolves its *head* the same way; the remaining segments
//!   are field / associated-item accesses, resolved with places and methods.
//! * Shadowing is allowed across nested scopes; re-binding inside one scope is
//!   reported.
//! * A module-level name may be declared more than once (overload sets are
//!   disambiguated by arity in `rill-ty`); the earliest declaration wins.

use std::collections::HashMap;
use std::path::PathBuf;

use rill_ast::{
    Block as AstBlock, Cond as AstCond, Constraint as AstConstraint, EffectRow as AstEffectRow,
    ElseBranch as AstElse, Expr as AstExpr, FnDecl, Frontend, Item as AstItem, Module, ParamGroup,
    Pattern as AstPattern, ProjectDecl, Stmt as AstStmt, Type as AstType, TypeBody as AstTypeBody,
    UsePath, VarStorage,
};
use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_span::{Interner, Span, Symbol};

use crate::cimport;
use crate::hir::*;

/// What lowering needs beyond the frontend: where `@include` searches.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Directories searched for `@include` headers, in order, before the
    /// defaults (`$CPATH`, the system prefixes). The driver puts the source
    /// file's own directory here.
    pub include_dirs: Vec<PathBuf>,
}

/// Lowers a desugared AST into HIR with resolved names.
///
/// The frontend is borrowed mutably because lowering interns the names a C
/// header contributes; `lower` is the no-options form.
pub fn lower(frontend: &mut Frontend) -> Hir {
    lower_with(frontend, &Options::default())
}

pub fn lower_with(frontend: &mut Frontend, options: &Options) -> Hir {
    // The resolver interns C names, so the frontend's module is moved aside
    // rather than borrowed alongside the interner.
    let module = std::mem::take(&mut frontend.module);
    let mut resolver = Resolver::new(&mut frontend.interner, options);
    resolver.module(&module);
    let hir = resolver.finish();
    frontend.module = module;
    hir
}

#[derive(Default)]
struct Scope {
    locals: HashMap<Symbol, LocalId>,
    items: HashMap<Symbol, ItemId>,
}

struct Resolver<'a> {
    interner: &'a mut Interner,
    out: HirBuilder,
    diagnostics: Diagnostics,
    scopes: Vec<Scope>,
    owner: Option<ItemId>,
    next_expr: u32,
    /// Module-level declarations by namespace.
    module_values: HashMap<Symbol, Vec<ItemId>>,
    module_types: HashMap<Symbol, Vec<ItemId>>,
    /// `use` imports: local name -> full path segments.
    imports: HashMap<Symbol, Vec<Symbol>>,
    /// `type M = @include("…")` modules: module name -> member name -> item.
    modules: HashMap<Symbol, HashMap<Symbol, ItemId>>,
    /// Module-level bindings by name, declared in the first pass.
    module_locals: HashMap<Symbol, LocalId>,
    /// Where `@include` looks for headers, in order.
    include_dirs: Vec<PathBuf>,
}

impl<'a> Resolver<'a> {
    fn new(interner: &'a mut Interner, options: &Options) -> Self {
        let mut include_dirs = options.include_dirs.clone();
        include_dirs.extend(cimport::default_include_dirs());
        Resolver {
            interner,
            out: HirBuilder::default(),
            diagnostics: Diagnostics::new(),
            scopes: vec![Scope::default()],
            owner: None,
            next_expr: 0,
            module_values: HashMap::new(),
            module_types: HashMap::new(),
            imports: HashMap::new(),
            modules: HashMap::new(),
            module_locals: HashMap::new(),
            include_dirs,
        }
    }

    fn finish(self) -> Hir {
        self.out.finish(self.diagnostics)
    }

    fn text(&self, sym: Symbol) -> &str {
        self.interner.get(sym)
    }

    fn err(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    // -- module ------------------------------------------------------------

    fn module(&mut self, module: &Module) {
        let slots = self.declare_module(module);
        // Traits, impls and builders first: their member names must be in scope
        // before any body that calls them is resolved.
        let declarations = |item: &AstItem| {
            matches!(
                item,
                AstItem::Trait(_) | AstItem::Impl(_) | AstItem::Builder(_)
            )
        };
        for (item, slot) in module.items.iter().zip(&slots) {
            if declarations(item) {
                self.item(item, Some(*slot));
            }
        }
        // Trait and impl members are nameable from the module: `show p` names
        // the trait's method, and static dispatch finds the impl.
        let members: Vec<(Symbol, ItemId)> = {
            let mut found = Vec::new();
            for item in self.out.items() {
                // Only trait members are nameable: a method name must resolve to
                // the trait's polymorphic signature, and static dispatch picks
                // the impl. Registering impl members too would let the first
                // call fix the method's type parameter for every later call.
                let member_ids = match &item.kind {
                    ItemKind::Trait(def) => &def.members,
                    _ => continue,
                };
                for member in member_ids {
                    if let Some(name) = self.out.try_item(*member).and_then(|m| m.name) {
                        found.push((name, *member));
                    }
                }
            }
            found
        };
        for (name, owner) in members {
            self.module_values
                .entry(name)
                .or_insert_with(|| vec![owner]);
        }
        for (item, slot) in module.items.iter().zip(&slots) {
            if !declarations(item) {
                // Global storage is spelled out: a bare `var` at module level
                // would silently be a static, which reads as a local.
                if let AstItem::Stmt(AstStmt::Var {
                    storage: VarStorage::Local,
                    name,
                    span,
                    ..
                }) = item
                {
                    let text = self.text(name.sym).to_string();
                    self.diagnostics.push(
                        Diagnostic::error(
                            *span,
                            "a module-level `var` must be declared `static var`",
                        )
                        .note(format!(
                            "`static var {text} = …;` is one copy for the program; \
                             `static threadlocal var {text} = …;` is one per thread"
                        )),
                    );
                }
                self.item(item, Some(*slot));
            }
        }
    }

    /// Pass 1: create a slot per module item and register its name.
    fn declare_module(&mut self, module: &Module) -> Vec<ItemId> {
        let mut slots = Vec::new();
        for item in &module.items {
            let (name, namespace) = item_name(item);
            let id = self.push_item_owned(name, ItemKind::Placeholder, item.span(), true);
            if let Some(name) = name {
                let table = match namespace {
                    Namespace::Value => &mut self.module_values,
                    Namespace::Type => &mut self.module_types,
                };
                table.entry(name).or_default().push(id);
            }
            // A module-level binding is a local of the module scope. Declaring
            // it here, before any body, makes every reference resolve to the
            // same local whether it is written before or after the statement;
            // without this a forward reference would resolve to the item and
            // lose its storage.
            if let AstItem::Stmt(stmt) = item {
                for (name, span, kind) in module_bindings(stmt) {
                    let local = self.declare_module_local(name, span, kind, id);
                    self.module_locals.entry(name).or_insert(local);
                }
            }
            slots.push(id);
        }
        // `type M = @include("…")` reads C headers, whose declarations must
        // exist before any `use M::…` names them.
        for item in &module.items {
            if let AstItem::Type(decl) = item {
                if let rill_ast::TypeBody::Include { path, span } = &decl.body {
                    self.expand_include(decl.name.sym, path, *span);
                }
            }
        }
        for item in &module.items {
            if let AstItem::Use(decl) = item {
                self.declare_use(&decl.path, decl.span);
            }
        }
        // Enum variants are nameable on their own (`Circle r`, `Rect 6 7`) as
        // well as qualified (`Step::Yield x`); both resolve to the enum's item.
        for (item, slot) in module.items.iter().zip(&slots) {
            let AstItem::Type(decl) = item else { continue };
            let rill_ast::TypeBody::Variants(variants) = &decl.body else {
                continue;
            };
            for variant in variants {
                self.module_values
                    .entry(variant.name.sym)
                    .or_default()
                    .push(*slot);
            }
        }
        slots
    }

    /// Expands one `@include` into items: each C declaration becomes an item
    /// named after its C name, recorded under the module so `M::name` resolves
    /// and `use M::*;` can prelude the lot.
    fn expand_include(&mut self, module: Symbol, path: &str, span: Span) {
        if self.modules.contains_key(&module) {
            return;
        }
        let dirs = self.include_dirs.clone();
        match cimport::import(module, path, span, &dirs, self.interner) {
            Ok(imported) => {
                let mut members: HashMap<Symbol, ItemId> = HashMap::new();
                for decl in imported.decls {
                    let id = self.push_item_owned(Some(decl.name), decl.kind, span, true);
                    if !decl.docs.is_empty() {
                        self.out.set_item_docs(id, decl.docs);
                    }
                    self.out.set_item_c_source(
                        id,
                        CSource {
                            file: decl.file,
                            line: decl.line,
                        },
                    );
                    members.insert(decl.name, id);
                }
                let headers = imported.headers;
                if members.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            span,
                            format!("`{path}` declared nothing this front end can use"),
                        )
                        .note(
                            "variadic functions, function-pointer parameters and unknown types are left out"
                                .to_string(),
                        ),
                    );
                }
                self.out.add_module(
                    IncludeDef {
                        module,
                        path: path.to_string(),
                        span,
                        headers,
                    },
                    members.clone(),
                );
                self.modules.insert(module, members);
            }
            Err(err) => self.err(span, err),
        }
    }

    fn declare_use(&mut self, path: &UsePath, span: Span) {
        match path {
            UsePath::Glob { prefix } => {
                let Some(module) = prefix.last().map(|ident| ident.sym) else {
                    self.err(span, "expected a module name before `::*`");
                    return;
                };
                let Some(members) = self.modules.get(&module).cloned() else {
                    let text = self.text(module).to_string();
                    self.err(span, format!("`{text}` is not an `@include`d module"));
                    return;
                };
                for (name, id) in &members {
                    let namespace = self
                        .out
                        .try_item(*id)
                        .map(|item| item.kind.namespace())
                        .unwrap_or(Namespace::Value);
                    let table = match namespace {
                        Namespace::Value => &mut self.module_values,
                        Namespace::Type => &mut self.module_types,
                    };
                    table.entry(*name).or_default().push(*id);
                }
                self.out.glob_module(module);
            }
            UsePath::Plain(segments)
                if segments.len() == 2
                    && segments
                        .first()
                        .map(|head| self.modules.contains_key(&head.sym))
                        .unwrap_or(false) =>
            {
                let module = segments[0].sym;
                let name = segments[1].sym;
                let Some(id) = self.modules[&module].get(&name).copied() else {
                    let module_text = self.text(module).to_string();
                    let name_text = self.text(name).to_string();
                    self.err(span, format!("`{module_text}` declares no `{name_text}`"));
                    return;
                };
                let namespace = self
                    .out
                    .try_item(id)
                    .map(|item| item.kind.namespace())
                    .unwrap_or(Namespace::Value);
                let table = match namespace {
                    Namespace::Value => &mut self.module_values,
                    Namespace::Type => &mut self.module_types,
                };
                table.entry(name).or_default().push(id);
            }
            _ => {
                for (name, full) in import_names(path) {
                    self.imports.insert(name, full);
                }
            }
        }
    }

    // -- items -------------------------------------------------------------

    /// Lowers one item; `slot` is the declaration-pass slot for module items.
    /// Returns the id of the item this declaration created or filled.
    fn item(&mut self, item: &AstItem, slot: Option<ItemId>) -> ItemId {
        let previous = self.owner;
        self.owner = slot.or(previous);
        let lowered = match item {
            AstItem::Use(decl) => {
                let def = UseDef {
                    path: decl.path.clone(),
                    names: use_names(&decl.path),
                };
                self.fill_or_push(slot, None, ItemKind::Use(def), decl.span)
            }
            AstItem::Fn(decl) => {
                let def = self.fn_def(decl, None);
                self.fill_or_push(slot, Some(decl.name.sym), ItemKind::Fn(def), decl.span)
            }
            AstItem::Project(decl) => {
                let def = self.project_def(decl);
                self.fill_or_push(slot, Some(decl.name.sym), ItemKind::Project(def), decl.span)
            }
            AstItem::Type(decl) => {
                for param in &decl.params {
                    if let Some(ty) = &param.comptime_ty {
                        self.type_refs(ty);
                    }
                }
                self.type_body_refs(&decl.body);
                let kind = ItemKind::Type(TypeDef {
                    params: decl.params.iter().map(|p| p.name.sym).collect(),
                    body: decl.body.clone(),
                    // Only `@include` imports carry a C name; the resolver sets
                    // it when it creates their items.
                    c_name: None,
                });
                self.fill_or_push(slot, Some(decl.name.sym), kind, decl.span)
            }
            AstItem::Trait(decl) => {
                self.constraint_type_refs(&decl.constraints);
                for param in &decl.params {
                    if let Some(ty) = &param.comptime_ty {
                        self.type_refs(ty);
                    }
                }
                let members = self.members(&decl.members);
                let kind = ItemKind::Trait(TraitDef {
                    params: decl.params.iter().map(|p| p.name.sym).collect(),
                    constraints: decl.constraints.clone(),
                    members,
                });
                self.fill_or_push(slot, Some(decl.name.sym), kind, decl.span)
            }
            AstItem::Impl(decl) => {
                self.constraint_type_refs(&decl.constraints);
                for param in &decl.params {
                    if let Some(ty) = &param.comptime_ty {
                        self.type_refs(ty);
                    }
                }
                let members = self.members(&decl.members);
                let kind = ItemKind::Impl(ImplDef {
                    params: decl.params.iter().map(|p| p.name.sym).collect(),
                    constraints: decl.constraints.clone(),
                    members,
                });
                self.fill_or_push(slot, Some(decl.name.sym), kind, decl.span)
            }
            AstItem::Builder(decl) => {
                for param in &decl.params {
                    if let Some(ty) = &param.comptime_ty {
                        self.type_refs(ty);
                    }
                }
                let members = self.members(&decl.members);
                let kind = ItemKind::Builder(BuilderDef {
                    params: decl.params.iter().map(|p| p.name.sym).collect(),
                    members,
                });
                self.fill_or_push(slot, Some(decl.name.sym), kind, decl.span)
            }
            AstItem::Signature(decl) => {
                self.type_refs(&decl.ty);
                self.constraint_type_refs(&decl.constraints);
                self.effect_type_refs(&decl.effects);
                let kind = ItemKind::Signature(SignatureDef {
                    abi: None,
                    ty: decl.ty.clone(),
                    constraints: decl.constraints.clone(),
                    effects: decl.effects.clone(),
                });
                self.fill_or_push(slot, Some(decl.name.sym), kind, decl.span)
            }
            AstItem::Extern(decl) => {
                // The inner declaration owns the name; the ABI is attached.
                match &*decl.inner {
                    AstItem::Fn(inner) => {
                        let def = self.fn_def(inner, decl.abi.clone());
                        self.fill_or_push(slot, Some(inner.name.sym), ItemKind::Fn(def), inner.span)
                    }
                    AstItem::Signature(inner) => {
                        self.type_refs(&inner.ty);
                        self.constraint_type_refs(&inner.constraints);
                        self.effect_type_refs(&inner.effects);
                        let kind = ItemKind::Signature(SignatureDef {
                            abi: decl.abi.clone(),
                            ty: inner.ty.clone(),
                            constraints: inner.constraints.clone(),
                            effects: inner.effects.clone(),
                        });
                        self.fill_or_push(slot, Some(inner.name.sym), kind, inner.span)
                    }
                    other => {
                        self.err(other.span(), "unsupported `extern` declaration");
                        self.item(other, None)
                    }
                }
            }
            AstItem::Stmt(stmt) => {
                let lowered = self.stmt(stmt, true);
                let name = top_level_bindings(stmt).first().map(|(name, _)| *name);
                self.fill_or_push(slot, name, ItemKind::Stmt(lowered), stmt.span())
            }
            AstItem::Attribute(decl) => {
                let attribute = self.text(decl.name.sym).to_string();
                match attribute.as_str() {
                    "link" => match &decl.value {
                        Some(library) => self.out.add_link(library.clone()),
                        None => {
                            self.err(decl.span, "`@link` needs a library name: `@link(\"SDL3\")`")
                        }
                    },
                    _ => self.diagnostics.push(
                        Diagnostic::warning(
                            decl.span,
                            format!("`@{attribute}` is not an attribute this compiler knows"),
                        )
                        .note("known: `@link(\"library\")`".to_string()),
                    ),
                }
                let def = AttributeDef {
                    value: decl.value.clone(),
                };
                self.fill_or_push(
                    slot,
                    Some(decl.name.sym),
                    ItemKind::Attribute(def),
                    decl.span,
                )
            }
            AstItem::Error(span) => {
                self.err(*span, "malformed item");
                self.fill_or_push(slot, None, ItemKind::Stmt(Stmt::Error(*span)), *span)
            }
        };
        let docs = item.docs().to_vec();
        if !docs.is_empty() {
            self.out.set_item_docs(lowered, docs);
        }
        if let Some(span) = ast_name_span(item) {
            self.out.set_item_name_span(lowered, span);
        }
        self.owner = previous;
        lowered
    }

    fn fill_or_push(
        &mut self,
        slot: Option<ItemId>,
        name: Option<Symbol>,
        kind: ItemKind,
        span: Span,
    ) -> ItemId {
        match slot {
            Some(id) => {
                self.out.set_item(id, kind, span);
                self.out.set_item_name(id, name);
                id
            }
            None => self.push_item_owned(name, kind, span, false),
        }
    }

    fn push_item_owned(
        &mut self,
        name: Option<Symbol>,
        kind: ItemKind,
        span: Span,
        module_level: bool,
    ) -> ItemId {
        let id = self.out.next_item_id();
        self.out.add_item(
            Item {
                id,
                name,
                name_span: None,
                kind,
                docs: Vec::new(),
                c_source: None,
                span,
            },
            module_level,
        )
    }

    /// Trait / impl / builder members.
    fn members(&mut self, members: &[AstItem]) -> Vec<ItemId> {
        members
            .iter()
            .map(|member| self.item(member, None))
            .collect()
    }

    fn fn_def(&mut self, decl: &FnDecl, abi: Option<String>) -> FnDef {
        let params = self.declare_params(&decl.params);
        self.param_type_refs(&decl.params);
        if let Some(ret) = &decl.ret {
            self.type_refs(&ret.ty);
        }
        self.constraint_type_refs(&decl.constraints);
        self.effect_type_refs(&decl.effects);
        let ret = decl.ret.as_ref().map(slot_of);
        let body = decl.body.as_ref().map(|body| self.expr(body));
        self.pop_scope();
        FnDef {
            abi,
            params,
            ret,
            constraints: decl.constraints.clone(),
            effects: decl.effects.clone(),
            body,
        }
    }

    fn project_def(&mut self, decl: &ProjectDecl) -> ProjectDef {
        let params = self.declare_params(&decl.params);
        self.param_type_refs(&decl.params);
        if let Some(ret) = &decl.ret {
            self.type_refs(&ret.ty);
        }
        self.constraint_type_refs(&decl.constraints);
        self.effect_type_refs(&decl.effects);
        let ret = decl.ret.as_ref().map(slot_of);
        let body = self.expr(&decl.body);
        self.pop_scope();
        ProjectDef {
            params,
            ret,
            constraints: decl.constraints.clone(),
            effects: decl.effects.clone(),
            body,
        }
    }

    /// Opens the parameter scope; callers close it once the body is resolved.
    fn declare_params(&mut self, groups: &[ParamGroup]) -> Vec<Param> {
        self.push_scope();
        let mut params = Vec::new();
        for group in groups {
            for param in &group.params {
                let local = match &param.name {
                    Some(name) => {
                        let kind = if param.comptime {
                            LocalKind::Binding
                        } else {
                            LocalKind::Param
                        };
                        Some(self.declare_local(name.sym, name.span, kind))
                    }
                    None => None,
                };
                params.push(Param {
                    local,
                    name: param.name.map(|name| name.sym),
                    convention: param.convention,
                    ty: param.ty.clone(),
                    comptime: param.comptime,
                    span: param.span,
                });
            }
        }
        params
    }

    // -- scopes and locals -------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        debug_assert!(!self.scopes.is_empty(), "the module scope is never popped");
    }

    fn declare_local(&mut self, name: Symbol, span: Span, kind: LocalKind) -> LocalId {
        let id = self.add_local(name, span, kind, self.owner);
        let scope = self.scopes.last_mut().expect("a scope is always open");
        if scope.locals.insert(name, id).is_some() {
            let text = self.text(name).to_string();
            self.err(span, format!("`{text}` is already bound in this scope"));
        }
        id
    }

    /// A module-level binding: a local of the module scope from the first
    /// pass on, owned by the statement item that declares it.
    fn declare_module_local(
        &mut self,
        name: Symbol,
        span: Span,
        kind: LocalKind,
        owner: ItemId,
    ) -> LocalId {
        let id = self.add_local(name, span, kind, Some(owner));
        let scope = self.scopes.first_mut().expect("the module scope is open");
        if scope.locals.insert(name, id).is_some() {
            let text = self.text(name).to_string();
            self.err(span, format!("`{text}` is already bound in this scope"));
        }
        id
    }

    fn add_local(
        &mut self,
        name: Symbol,
        span: Span,
        kind: LocalKind,
        owner: Option<ItemId>,
    ) -> LocalId {
        let id = self.out.next_local_id();
        self.out.add_local(LocalInfo {
            id,
            name,
            span,
            kind,
            owner,
        });
        id
    }

    /// A block-level `let f (…) = …` declares an item in the current scope.
    fn declare_scoped_item(&mut self, name: Symbol, id: ItemId, span: Span) {
        let scope = self.scopes.last_mut().expect("a scope is always open");
        if scope.items.insert(name, id).is_some() {
            let text = self.text(name).to_string();
            self.err(span, format!("`{text}` is already declared in this scope"));
        }
    }

    // -- resolution --------------------------------------------------------

    fn resolve_path(&mut self, segments: &[Symbol], span: Span) -> Res {
        let head = segments[0];
        // A qualified name into an `@include`d module: `SDL3::SDL_Init`.
        if segments.len() > 1 {
            if let Some(id) = self.out.module_item(head, segments[1]) {
                return Res::Item(id);
            }
            if self.modules.contains_key(&head) {
                let module = self.text(head).to_string();
                let name = self.text(segments[1]).to_string();
                self.err(span, format!("`{module}` declares no `{name}`"));
                return Res::Error;
            }
        }
        if let Some(res) = self.lookup_local(head) {
            return res;
        }
        if let Some(id) = self.lookup_scoped_item(head) {
            return Res::Item(id);
        }
        if let Some(id) = self.lookup_module(head, Namespace::Value) {
            return Res::Item(id);
        }
        if let Some(id) = self.lookup_module(head, Namespace::Type) {
            // A type name in value position is its constructor.
            return Res::Item(id);
        }
        if let Some(path) = self.imports.get(&head) {
            return Res::Import(path.clone());
        }
        let text = self.text(head).to_string();
        if is_builtin(&text) {
            return Res::Builtin;
        }
        self.err(span, format!("unknown name `{text}`"));
        Res::Error
    }

    fn lookup_local(&self, name: Symbol) -> Option<Res> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.locals.get(&name) {
                return Some(Res::Local(*local));
            }
        }
        None
    }

    fn lookup_scoped_item(&self, name: Symbol) -> Option<ItemId> {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.items.get(&name) {
                return Some(*id);
            }
        }
        None
    }

    fn lookup_module(&self, name: Symbol, ns: Namespace) -> Option<ItemId> {
        let table = match ns {
            Namespace::Value => &self.module_values,
            Namespace::Type => &self.module_types,
        };
        table.get(&name).and_then(|ids| ids.first()).copied()
    }

    // -- expressions -------------------------------------------------------

    fn expr(&mut self, expr: &AstExpr) -> Expr {
        let id = self.next_expr_id();
        let span = expr.span();
        let kind = match expr {
            AstExpr::Literal { kind, text, .. } => ExprKind::Literal {
                kind: *kind,
                text: text.clone(),
            },
            AstExpr::Path { segments, .. } => {
                let symbols: Vec<Symbol> = segments.iter().map(|s| s.sym).collect();
                let res = self.resolve_path(&symbols, span);
                self.record_ref(span, &symbols, res.clone());
                ExprKind::Path {
                    segments: symbols,
                    res,
                }
            }
            AstExpr::Tuple { elems, .. } => {
                ExprKind::Tuple(elems.iter().map(|e| self.expr(e)).collect())
            }
            AstExpr::Array { elems, .. } => {
                ExprKind::Array(elems.iter().map(|e| self.expr(e)).collect())
            }
            AstExpr::Block(block) => ExprKind::Block(self.block(block)),
            AstExpr::Lambda { params, body, .. } => {
                self.push_scope();
                let params = params.iter().map(|p| self.pattern(p)).collect();
                let body = self.block(body);
                self.pop_scope();
                ExprKind::Lambda { params, body }
            }
            AstExpr::Call { callee, args, .. } => ExprKind::Call {
                callee: Box::new(self.expr(callee)),
                args: self.args(args),
            },
            AstExpr::Index { base, args, .. } => ExprKind::Index {
                base: Box::new(self.expr(base)),
                args: self.args(args),
            },
            AstExpr::Try { base, .. } => ExprKind::Try {
                base: Box::new(self.expr(base)),
            },
            AstExpr::Unary { op, operand, .. } => ExprKind::Unary {
                op: *op,
                operand: Box::new(self.expr(operand)),
            },
            AstExpr::Binary { op, lhs, rhs, .. } => {
                let lhs = Box::new(self.expr(lhs));
                // The right-hand side of `.` names a field or a method, not a
                // binding: resolve it quietly and fall back to `Res::Field`.
                let rhs = match op {
                    rill_ast::BinaryOp::Flow => Box::new(self.flow_rhs(rhs)),
                    _ => Box::new(self.expr(rhs)),
                };
                ExprKind::Binary { op: *op, lhs, rhs }
            }
            AstExpr::Cast { expr, ty, .. } => {
                self.type_refs(ty);
                ExprKind::Cast {
                    expr: Box::new(self.expr(expr)),
                    ty: ty.clone(),
                }
            }
            AstExpr::Is { expr, ty, .. } => {
                self.type_refs(ty);
                ExprKind::Is {
                    expr: Box::new(self.expr(expr)),
                    ty: ty.clone(),
                }
            }
            AstExpr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                // `if (x := e) { … }` keeps its binding for the then-branch.
                let (cond, bound) = self.cond_in_scope(cond);
                let then_branch = self.block(then_branch);
                if bound {
                    self.pop_scope();
                }
                let else_branch = else_branch.as_ref().map(|branch| match branch {
                    AstElse::Block(block) => ElseBranch::Block(self.block(block)),
                    AstElse::If(expr) => ElseBranch::If(Box::new(self.expr(expr))),
                });
                ExprKind::If {
                    cond,
                    then_branch,
                    else_branch,
                }
            }
            AstExpr::Match {
                scrutinee, arms, ..
            } => {
                let scrutinee = Box::new(self.expr(scrutinee));
                let mut lowered = Vec::new();
                for arm in arms {
                    self.push_scope();
                    let pattern = self.pattern(&arm.pattern);
                    let guard = arm.guard.as_ref().map(|g| self.expr(g));
                    let body = self.expr(&arm.body);
                    self.pop_scope();
                    lowered.push(MatchArm {
                        pattern,
                        guard,
                        body,
                        span: arm.span,
                    });
                }
                ExprKind::Match {
                    scrutinee,
                    arms: lowered,
                }
            }
            AstExpr::Comptime { body, .. } => ExprKind::Comptime {
                body: self.block(body),
            },
            AstExpr::Unsafe { body, .. } => ExprKind::Unsafe {
                body: self.block(body),
            },
            AstExpr::Handler { kind, body, .. } => ExprKind::Handler {
                kind: *kind,
                body: self.block(body),
            },
            AstExpr::Concurrent { kind, arms, .. } => ExprKind::Concurrent {
                kind: *kind,
                arms: arms.iter().map(|a| self.expr(a)).collect(),
            },
            AstExpr::Error(_) => {
                self.err(span, "malformed expression");
                ExprKind::Error
            }
        };
        Expr { id, kind, span }
    }

    /// The right-hand side of `.`: a resolvable name when one exists (the
    /// field or method is then resolved with types), else a field selector.
    /// Handles the whole spine abutted to `.` — `from.balance`,
    /// `xs.split_at[i]`, `items.fold 0 { … }`.
    fn flow_rhs(&mut self, expr: &AstExpr) -> Expr {
        match expr {
            AstExpr::Path { segments, span } => {
                let symbols: Vec<Symbol> = segments.iter().map(|s| s.sym).collect();
                let res = self.resolve_quietly(&symbols);
                self.record_ref(*span, &symbols, res.clone());
                let id = self.next_expr_id();
                Expr {
                    id,
                    kind: ExprKind::Path {
                        segments: symbols,
                        res,
                    },
                    span: *span,
                }
            }
            AstExpr::Index { base, args, span } => {
                let base = Box::new(self.flow_rhs(base));
                Expr {
                    id: self.next_expr_id(),
                    kind: ExprKind::Index {
                        base,
                        args: self.args(args),
                    },
                    span: *span,
                }
            }
            AstExpr::Call { callee, args, span } => {
                let callee = Box::new(self.flow_rhs(callee));
                Expr {
                    id: self.next_expr_id(),
                    kind: ExprKind::Call {
                        callee,
                        args: self.args(args),
                    },
                    span: *span,
                }
            }
            AstExpr::Try { base, span } => {
                let base = Box::new(self.flow_rhs(base));
                Expr {
                    id: self.next_expr_id(),
                    kind: ExprKind::Try { base },
                    span: *span,
                }
            }
            _ => self.expr(expr),
        }
    }

    /// Resolution for *member* position (the right of `.`): locals are never
    /// members, so a field named after a local still selects the field. Items
    /// are consulted because methods (`xs.split_at[i]`, `from.balance_mut`) are
    /// functions and projections.
    fn resolve_quietly(&self, segments: &[Symbol]) -> Res {
        let head = segments[0];
        if let Some(id) = self.lookup_scoped_item(head) {
            return Res::Item(id);
        }
        if let Some(id) = self.lookup_module(head, Namespace::Value) {
            return Res::Item(id);
        }
        if let Some(id) = self.lookup_module(head, Namespace::Type) {
            return Res::Item(id);
        }
        if let Some(path) = self.imports.get(&head) {
            return Res::Import(path.clone());
        }
        let text = self.text(head);
        if is_builtin(text) {
            return Res::Builtin;
        }
        Res::Field
    }

    fn next_expr_id(&mut self) -> ExprId {
        let id = ExprId(self.next_expr);
        self.next_expr += 1;
        id
    }

    fn record_ref(&mut self, span: Span, segments: &[Symbol], res: Res) {
        self.out.add_ref(RefInfo {
            span,
            name: segments[0],
            segments: segments.to_vec(),
            res,
            kind: RefKind::Expr,
        });
    }

    fn record_type_ref(&mut self, span: Span, segments: &[Symbol], res: Res) {
        self.out.add_ref(RefInfo {
            span,
            name: segments[0],
            segments: segments.to_vec(),
            res,
            kind: RefKind::Type,
        });
    }

    // -- type references ---------------------------------------------------
    //
    // Types are not expressions, so a name written in type position (`x: T`,
    // `M::Rect`, `e as T`) has no expression to hang a reference on. These
    // walk the type syntax and record refs anyway, which is what makes goto
    // definition and find-references work on type names.

    fn type_refs(&mut self, ty: &AstType) {
        match ty {
            AstType::Path { segments, span } => {
                let symbols: Vec<Symbol> = segments.iter().map(|s| s.sym).collect();
                let res = self.resolve_type_path(&symbols);
                if !res.is_error() {
                    self.record_type_ref(*span, &symbols, res);
                }
            }
            AstType::App { ctor, args, .. } => {
                self.type_refs(ctor);
                for arg in args {
                    self.type_refs(arg);
                }
            }
            AstType::Tuple { elems, .. } => {
                for elem in elems {
                    self.type_refs(elem);
                }
            }
            AstType::Array { elem, .. } => self.type_refs(elem),
            AstType::Fn { params, ret, .. } => {
                for param in params {
                    self.type_refs(&param.ty);
                }
                if let Some(ret) = ret {
                    self.type_refs(ret);
                }
            }
            AstType::Ptr { pointee, .. } | AstType::Dyn { inner: pointee, .. } => {
                self.type_refs(pointee)
            }
            AstType::Error(_) => {}
        }
    }

    /// Resolution for type position: module members (`M::T`), the type
    /// namespace, then `use` imports. Locals never name types.
    fn resolve_type_path(&self, segments: &[Symbol]) -> Res {
        let head = segments[0];
        if segments.len() > 1 {
            if let Some(id) = self.out.module_item(head, segments[1]) {
                return Res::Item(id);
            }
        }
        if let Some(id) = self.lookup_module(head, Namespace::Type) {
            return Res::Item(id);
        }
        if let Some(path) = self.imports.get(&head) {
            return Res::Import(path.clone());
        }
        Res::Error
    }

    fn param_type_refs(&mut self, groups: &[ParamGroup]) {
        for group in groups {
            for param in &group.params {
                if let Some(ty) = &param.ty {
                    self.type_refs(ty);
                }
            }
        }
    }

    fn constraint_type_refs(&mut self, constraints: &[AstConstraint]) {
        for constraint in constraints {
            match constraint {
                AstConstraint::Is {
                    ty,
                    trait_path,
                    args,
                    ..
                } => {
                    self.type_refs(ty);
                    for arg in args {
                        self.type_refs(arg);
                    }
                    if let Some(first) = trait_path.first() {
                        let span = trait_path
                            .iter()
                            .fold(first.span, |acc, ident| acc.merge(ident.span));
                        let symbols: Vec<Symbol> = trait_path.iter().map(|i| i.sym).collect();
                        let res = self.resolve_type_path(&symbols);
                        if !res.is_error() {
                            self.record_type_ref(span, &symbols, res);
                        }
                    }
                }
                AstConstraint::Effect { effects, .. } => self.effect_type_refs(effects),
            }
        }
    }

    fn effect_type_refs(&mut self, row: &AstEffectRow) {
        for item in &row.items {
            for arg in &item.args {
                self.type_refs(arg);
            }
        }
    }

    fn type_body_refs(&mut self, body: &AstTypeBody) {
        match body {
            AstTypeBody::Record(record) | AstTypeBody::View(record) => {
                for field in &record.fields {
                    self.type_refs(&field.ty);
                }
            }
            AstTypeBody::Variants(variants) => {
                for variant in variants {
                    for payload in &variant.payload {
                        self.type_refs(payload);
                    }
                }
            }
            AstTypeBody::Alias(ty) => self.type_refs(ty),
            AstTypeBody::Include { .. } | AstTypeBody::Error(_) => {}
        }
    }

    fn args(&mut self, args: &[rill_ast::Arg]) -> Vec<Arg> {
        args.iter()
            .map(|arg| Arg {
                convention: arg.convention,
                value: self.expr(&arg.value),
                span: arg.span,
            })
            .collect()
    }

    /// Lowers a condition; `true` means the pattern's scope was left open (the
    /// caller decides where it ends).
    fn cond_in_scope(&mut self, cond: &AstCond) -> (Cond, bool) {
        match cond {
            AstCond::Expr(expr) => (Cond::Expr(Box::new(self.expr(expr))), false),
            AstCond::Bind {
                pattern,
                value,
                span,
            } => {
                let value = Box::new(self.expr(value));
                self.push_scope();
                let pattern = self.pattern(pattern);
                (
                    Cond::Bind {
                        pattern,
                        value,
                        span: *span,
                    },
                    true,
                )
            }
        }
    }

    // -- statements --------------------------------------------------------

    fn block(&mut self, block: &AstBlock) -> Block {
        self.push_scope();
        let stmts = block
            .stmts
            .iter()
            .map(|stmt| {
                if let AstStmt::Var { storage, span, .. } = stmt {
                    if !matches!(storage, VarStorage::Local) {
                        self.err(*span, "`static` storage is only allowed at module level");
                    }
                }
                self.stmt(stmt, false)
            })
            .collect();
        let tail = block.tail.as_ref().map(|tail| Box::new(self.expr(tail)));
        self.pop_scope();
        Block {
            stmts,
            tail,
            span: block.span,
        }
    }

    fn stmt(&mut self, stmt: &AstStmt, top_level: bool) -> Stmt {
        match stmt {
            AstStmt::Var {
                storage,
                name,
                ty,
                init,
                span,
            } => {
                if let Some(ty) = ty {
                    self.type_refs(ty);
                }
                let init = self.expr(init);
                let local = self.binding_local(top_level, name.sym, name.span, LocalKind::Var);
                Stmt::Var {
                    storage: *storage,
                    local,
                    ty: ty.clone(),
                    init,
                    span: *span,
                }
            }
            AstStmt::Set { place, value, span } => Stmt::Set {
                place: self.expr(place),
                value: self.expr(value),
                span: *span,
            },
            AstStmt::Yield {
                convention,
                value,
                span,
            } => Stmt::Yield {
                convention: *convention,
                value: self.expr(value),
                span: *span,
            },
            AstStmt::Bind {
                name,
                op: _,
                value,
                span,
            } => {
                let value = self.expr(value);
                let pattern = self.pattern_at(name, top_level);
                Stmt::Bind {
                    pattern,
                    value,
                    span: *span,
                }
            }
            AstStmt::TryBind { name, value, span } => {
                let value = self.expr(value);
                let local = self.binding_local(top_level, name.sym, name.span, LocalKind::TryBind);
                Stmt::TryBind {
                    local,
                    value,
                    span: *span,
                }
            }
            AstStmt::When { cond, span } => {
                // A guard's binding stays visible for the rest of the block.
                let (cond, _) = self.cond_in_scope(cond);
                Stmt::When { cond, span: *span }
            }
            AstStmt::For {
                pattern,
                iter,
                body,
                span,
            } => {
                let iter = self.expr(iter);
                self.push_scope();
                let pattern = self.pattern(pattern);
                let body = self.block(body);
                self.pop_scope();
                Stmt::For {
                    pattern,
                    iter,
                    body,
                    span: *span,
                }
            }
            AstStmt::While { cond, body, span } => {
                let cond = self.expr(cond);
                let body = self.block(body);
                Stmt::While {
                    cond,
                    body,
                    span: *span,
                }
            }
            AstStmt::Break { span } => Stmt::Break { span: *span },
            AstStmt::Let(decl) => {
                // A block-level `let` is an item declared in the block scope,
                // before its body is lowered so it can recurse.
                let id = self.push_item_owned(
                    Some(decl.name.sym),
                    ItemKind::Placeholder,
                    decl.span,
                    false,
                );
                self.declare_scoped_item(decl.name.sym, id, decl.span);
                let def = self.fn_def(decl, None);
                self.out.set_item(id, ItemKind::Fn(def), decl.span);
                Stmt::Let {
                    item: id,
                    span: decl.span,
                }
            }
            AstStmt::Expr(expr) => Stmt::Expr(self.expr(expr)),
            AstStmt::Error(span) => {
                self.err(*span, "malformed statement");
                Stmt::Error(*span)
            }
        }
    }

    /// Binds a pattern's names in the current scope.
    fn pattern(&mut self, pattern: &AstPattern) -> Pattern {
        self.pattern_at(pattern, false)
    }

    /// A single-name binding (`var x`, `let? x`): the pre-declared module
    /// local at module level, a fresh block local otherwise.
    fn binding_local(
        &mut self,
        top_level: bool,
        name: Symbol,
        span: Span,
        kind: LocalKind,
    ) -> LocalId {
        if top_level {
            if let Some(local) = self.module_locals.get(&name).copied() {
                return local;
            }
        }
        self.declare_local(name, span, kind)
    }

    /// `top_level` reuses the module locals declared in the first pass, so a
    /// module-level `x := e` is the same local everywhere it is referenced.
    fn pattern_at(&mut self, pattern: &AstPattern, top_level: bool) -> Pattern {
        match pattern {
            AstPattern::Wildcard(span) => Pattern::Wildcard(*span),
            AstPattern::Binding { name, span } => {
                let local = self.binding_local(top_level, name.sym, name.span, LocalKind::Pattern);
                Pattern::Binding { local, span: *span }
            }
            AstPattern::Literal { text, span } => Pattern::Literal {
                text: text.clone(),
                span: *span,
            },
            AstPattern::Constructor { path, args, span } => {
                let segments: Vec<Symbol> = path.iter().map(|s| s.sym).collect();
                let res = self.resolve_path(&segments, *span);
                self.record_ref(*span, &segments, res.clone());
                let args = args
                    .iter()
                    .map(|arg| self.pattern_at(arg, top_level))
                    .collect();
                Pattern::Constructor {
                    path: segments,
                    res,
                    args,
                    span: *span,
                }
            }
            AstPattern::Tuple { elems, span } => Pattern::Tuple {
                elems: elems
                    .iter()
                    .map(|elem| self.pattern_at(elem, top_level))
                    .collect(),
                span: *span,
            },
            AstPattern::Error(span) => {
                self.err(*span, "malformed pattern");
                Pattern::Error(*span)
            }
        }
    }
}

fn slot_of(slot: &rill_ast::ParamSlot) -> ParamSlot {
    ParamSlot {
        convention: slot.convention,
        ty: slot.ty.clone(),
        span: slot.span,
    }
}

/// The name an item declares and the namespace it lives in.
fn item_name(item: &AstItem) -> (Option<Symbol>, Namespace) {
    match item {
        AstItem::Fn(decl) => (Some(decl.name.sym), Namespace::Value),
        AstItem::Project(decl) => (Some(decl.name.sym), Namespace::Value),
        AstItem::Type(decl) => (Some(decl.name.sym), Namespace::Type),
        AstItem::Trait(decl) => (Some(decl.name.sym), Namespace::Type),
        AstItem::Builder(decl) => (Some(decl.name.sym), Namespace::Value),
        AstItem::Signature(decl) => (Some(decl.name.sym), Namespace::Value),
        AstItem::Extern(decl) => match &*decl.inner {
            AstItem::Fn(inner) => (Some(inner.name.sym), Namespace::Value),
            AstItem::Signature(inner) => (Some(inner.name.sym), Namespace::Value),
            _ => (None, Namespace::Value),
        },
        AstItem::Attribute(decl) => (Some(decl.name.sym), Namespace::Value),
        AstItem::Stmt(stmt) => (
            top_level_bindings(stmt).first().map(|(name, _)| *name),
            Namespace::Value,
        ),
        AstItem::Impl(_) | AstItem::Use(_) | AstItem::Error(_) => (None, Namespace::Value),
    }
}

/// Where the declaration writes its name, for tools that point at definitions.
fn ast_name_span(item: &AstItem) -> Option<Span> {
    match item {
        AstItem::Fn(decl) => Some(decl.name.span),
        AstItem::Project(decl) => Some(decl.name.span),
        AstItem::Type(decl) => Some(decl.name.span),
        AstItem::Trait(decl) => Some(decl.name.span),
        AstItem::Impl(decl) => Some(decl.name.span),
        AstItem::Builder(decl) => Some(decl.name.span),
        AstItem::Signature(decl) => Some(decl.name.span),
        AstItem::Attribute(decl) => Some(decl.name.span),
        AstItem::Extern(decl) => ast_name_span(&decl.inner),
        AstItem::Stmt(_) | AstItem::Use(_) | AstItem::Error(_) => None,
    }
}

/// `(imported name, full path)` pairs introduced by a `use` declaration.
fn import_names(path: &UsePath) -> Vec<(Symbol, Vec<Symbol>)> {
    match path {
        UsePath::Plain(segments) => match segments.last() {
            Some(last) => vec![(last.sym, segments.iter().map(|s| s.sym).collect())],
            None => Vec::new(),
        },
        UsePath::Group { prefix, names } => names
            .iter()
            .map(|name| {
                let mut full: Vec<Symbol> = prefix.iter().map(|s| s.sym).collect();
                full.push(name.sym);
                (name.sym, full)
            })
            .collect(),
        // `use M::*;` is resolved against the module itself, not as a path.
        UsePath::Glob { .. } => Vec::new(),
        UsePath::Text { .. } => Vec::new(),
    }
}

fn use_names(path: &UsePath) -> Vec<Symbol> {
    import_names(path)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Module-level bindings declared by a statement.
fn top_level_bindings(stmt: &AstStmt) -> Vec<(Symbol, Span)> {
    match stmt {
        AstStmt::Var { name, span, .. } => vec![(name.sym, *span)],
        AstStmt::Bind { name, .. } => pattern_bindings(name),
        AstStmt::TryBind { name, span, .. } => vec![(name.sym, *span)],
        AstStmt::For { pattern, .. } => pattern_bindings(pattern),
        _ => Vec::new(),
    }
}

/// The bindings a module-level statement declares in the module scope, with
/// the span of the name itself and the local kind the statement gives it.
/// `for` patterns are *not* included: a loop variable is scoped to its body.
fn module_bindings(stmt: &AstStmt) -> Vec<(Symbol, Span, LocalKind)> {
    match stmt {
        AstStmt::Var { name, .. } => vec![(name.sym, name.span, LocalKind::Var)],
        AstStmt::TryBind { name, .. } => vec![(name.sym, name.span, LocalKind::TryBind)],
        AstStmt::Bind { name, .. } => binding_idents(name)
            .into_iter()
            .map(|ident| (ident.sym, ident.span, LocalKind::Pattern))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every binding identifier in a pattern, with the identifier's own span.
fn binding_idents(pattern: &AstPattern) -> Vec<rill_ast::Ident> {
    match pattern {
        AstPattern::Binding { name, .. } => vec![*name],
        AstPattern::Tuple { elems, .. } => elems.iter().flat_map(binding_idents).collect(),
        AstPattern::Constructor { args, .. } => args.iter().flat_map(binding_idents).collect(),
        AstPattern::Wildcard(_) | AstPattern::Literal { .. } | AstPattern::Error(_) => Vec::new(),
    }
}

fn pattern_bindings(pattern: &AstPattern) -> Vec<(Symbol, Span)> {
    match pattern {
        AstPattern::Binding { name, span } => vec![(name.sym, *span)],
        AstPattern::Tuple { elems, .. } => elems.iter().flat_map(pattern_bindings).collect(),
        AstPattern::Constructor { args, .. } => args.iter().flat_map(pattern_bindings).collect(),
        _ => Vec::new(),
    }
}

/// Compiler-provided names the front end knows by name. The list is data rather
/// than a `matches!` because tools enumerate it: completion offers these names,
/// and the checker's signature table is keyed by them.
pub const BUILTIN_NAMES: &[&str] = &[
    // `fail` is the failure of a `<decides>` expression (PLAN §3).
    "threads_bind",
    "threads_count",
    "spawn_thread",
    "join_threads",
    "f64x4",
    "io_bind",
    "io_unbind",
    "io_spawn",
    "io_run",
    "arena_bind",
    "arena_unbind",
    "fail",
    "zeroed",
    "buf_new",
    "buf_push",
    "buf_len",
    "buf_get",
    "buf_text",
    "arena_new",
    "arena_mark",
    "arena_reset",
    "arena_used",
    "arena_alloc",
];

/// Whether `name` is one of the compiler's builtins.
pub fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}
