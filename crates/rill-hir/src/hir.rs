//! High-level IR with resolved names (PLAN.md §7.1/§7.2, M1).
//!
//! HIR mirrors the desugared AST but fuses name resolution into the tree:
//! every expression carries an [`ExprId`], every reference a [`Res`], and
//! every binding a [`LocalId`]. Later passes (type inference, borrow checking)
//! index their tables by these ids instead of re-walking syntax.
//!
//! Type *syntax* is reused from `rill-ast` ([`Type`], [`TypeBody`],
//! [`EffectRow`], [`Constraint`]): resolving type names is `rill-ty`'s job,
//! which is also where shape/`comptime` evaluation lives.

use rill_ast::{
    BinaryOp, ConcurrentKind, Convention, EffectRow, HandlerKind, LiteralKind, Type, TypeBody,
    UnaryOp, UsePath,
};
use rill_span::{Span, Symbol};

pub use rill_ast::Constraint;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ItemId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ExprId(pub u32);

/// What a name reference resolved to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Res {
    /// A module-level item: a function, projection, type constructor, builder…
    Item(ItemId),
    /// A local binding.
    Local(LocalId),
    /// A `use`-imported name; the import's path segments (resolution of the
    /// import itself belongs to the module graph / linker).
    Import(Vec<Symbol>),
    /// A compiler-provided name whose shape is contextual (`zeroed ()` inside
    /// `comptime { … }`, the seed of `core`'s intrinsics).
    Builtin,
    /// The name selected by `.` after an expression (`t.rotation`,
    /// `xs.fold`): a field or a method, decided with types. Not an error when
    /// nothing of that name is in scope, because fields are not bindings.
    Field,
    /// Nothing matched.
    Error,
}

impl Res {
    pub fn is_error(&self) -> bool {
        matches!(self, Res::Error)
    }
}

/// How a local binding came to be, which decides its passing convention.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalKind {
    /// Parameter of a function, projection or lambda.
    Param,
    /// `let x = e;` / `x := e;` / `var x = e;`
    Binding,
    /// `var x = e;` — mutable storage.
    Var,
    /// Bound by a pattern (`match`, `for`, lambda parameters).
    Pattern,
    /// `let? x = e;`
    TryBind,
}

#[derive(Clone, Debug)]
pub struct LocalInfo {
    pub id: LocalId,
    pub name: Symbol,
    pub span: Span,
    pub kind: LocalKind,
    /// The item whose body owns this local.
    pub owner: Option<ItemId>,
}

/// A recorded name reference; `span` is the referencing path's span.
#[derive(Clone, Debug)]
pub struct RefInfo {
    pub span: Span,
    pub name: Symbol,
    pub segments: Vec<Symbol>,
    pub res: Res,
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Item {
    pub id: ItemId,
    /// `None` for anonymous/module-level statements.
    pub name: Option<Symbol>,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Fn(FnDef),
    Project(ProjectDef),
    Type(TypeDef),
    Trait(TraitDef),
    Impl(ImplDef),
    Builder(BuilderDef),
    Signature(SignatureDef),
    Use(UseDef),
    /// A module-level statement (`var`, `set`, an expression …).
    Stmt(Stmt),
    /// Slot created while registering module-level names, filled by the body
    /// pass. Never present in a finished [`Hir`].
    Placeholder,
}

impl ItemKind {
    /// The namespace a name declared here lives in.
    pub fn namespace(&self) -> Namespace {
        match self {
            ItemKind::Type(_) | ItemKind::Trait(_) | ItemKind::Impl(_) => Namespace::Type,
            _ => Namespace::Value,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Namespace {
    Value,
    Type,
}

#[derive(Clone, Debug)]
pub struct FnDef {
    /// `Some("c")` for `extern "c" let …`.
    pub abi: Option<String>,
    pub params: Vec<Param>,
    pub ret: Option<ParamSlot>,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    /// `None` for bodyless declarations (trait members, `extern`).
    pub body: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct ProjectDef {
    pub params: Vec<Param>,
    pub ret: Option<ParamSlot>,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct SignatureDef {
    pub abi: Option<String>,
    pub ty: Type,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    pub params: Vec<Symbol>,
    pub body: TypeBody,
}

#[derive(Clone, Debug)]
pub struct TraitDef {
    pub params: Vec<Symbol>,
    pub constraints: Vec<Constraint>,
    pub members: Vec<ItemId>,
}

#[derive(Clone, Debug)]
pub struct ImplDef {
    pub params: Vec<Symbol>,
    pub constraints: Vec<Constraint>,
    pub members: Vec<ItemId>,
}

#[derive(Clone, Debug)]
pub struct BuilderDef {
    pub params: Vec<Symbol>,
    pub members: Vec<ItemId>,
}

#[derive(Clone, Debug)]
pub struct UseDef {
    pub path: UsePath,
    /// Names this import introduces, in declaration order.
    pub names: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub local: Option<LocalId>,
    pub name: Option<Symbol>,
    pub convention: Option<Convention>,
    pub ty: Option<Type>,
    pub comptime: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParamSlot {
    pub convention: Option<Convention>,
    pub ty: Type,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Statements and expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        item: ItemId,
        span: Span,
    },
    Var {
        local: LocalId,
        ty: Option<Type>,
        init: Expr,
        span: Span,
    },
    Set {
        place: Expr,
        value: Expr,
        span: Span,
    },
    Yield {
        convention: Option<Convention>,
        value: Expr,
        span: Span,
    },
    Bind {
        pattern: Pattern,
        value: Expr,
        span: Span,
    },
    TryBind {
        local: LocalId,
        value: Expr,
        span: Span,
    },
    When {
        cond: Cond,
        span: Span,
    },
    For {
        pattern: Pattern,
        iter: Expr,
        body: Block,
        span: Span,
    },
    /// `while (cond) body`
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    /// `break;`
    Break {
        span: Span,
    },
    Expr(Expr),
    Error(Span),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Var { span, .. }
            | Stmt::Set { span, .. }
            | Stmt::Yield { span, .. }
            | Stmt::Bind { span, .. }
            | Stmt::TryBind { span, .. }
            | Stmt::When { span, .. }
            | Stmt::For { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Break { span }
            | Stmt::Error(span) => *span,
            Stmt::Expr(expr) => expr.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The block's tail expression; boxed because expressions contain blocks.
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard(Span),
    Binding {
        local: LocalId,
        span: Span,
    },
    Literal {
        text: String,
        span: Span,
    },
    Constructor {
        path: Vec<Symbol>,
        res: Res,
        args: Vec<Pattern>,
        span: Span,
    },
    Tuple {
        elems: Vec<Pattern>,
        span: Span,
    },
    Error(Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(span)
            | Pattern::Binding { span, .. }
            | Pattern::Literal { span, .. }
            | Pattern::Constructor { span, .. }
            | Pattern::Tuple { span, .. }
            | Pattern::Error(span) => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub convention: Option<Convention>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Cond {
    Expr(Box<Expr>),
    Bind {
        pattern: Pattern,
        value: Box<Expr>,
        span: Span,
    },
}

impl Cond {
    pub fn span(&self) -> Span {
        match self {
            Cond::Expr(expr) => expr.span,
            Cond::Bind { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ElseBranch {
    Block(Block),
    If(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: ExprId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Literal {
        kind: LiteralKind,
        text: String,
    },
    Path {
        segments: Vec<Symbol>,
        res: Res,
    },
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    Block(Block),
    Lambda {
        params: Vec<Pattern>,
        body: Block,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    /// `f[x]`, `xs[i]` — adjacent bracket application.
    Index {
        base: Box<Expr>,
        args: Vec<Arg>,
    },
    Try {
        base: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    Is {
        expr: Box<Expr>,
        ty: Type,
    },
    If {
        cond: Cond,
        then_branch: Block,
        else_branch: Option<ElseBranch>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Comptime {
        body: Block,
    },
    Unsafe {
        body: Block,
    },
    Handler {
        kind: HandlerKind,
        body: Block,
    },
    Concurrent {
        kind: ConcurrentKind,
        arms: Vec<Expr>,
    },
    Error,
}

impl Expr {
    /// The single name this path resolves to, when it is a plain name.
    pub fn path_res(&self) -> Option<(&[Symbol], &Res)> {
        match &self.kind {
            ExprKind::Path { segments, res } => Some((segments, res)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The HIR container
// ---------------------------------------------------------------------------

/// A resolved module: the item table, the local table, every reference and
/// the diagnostics produced while resolving.
#[derive(Clone, Debug, Default)]
pub struct Hir {
    items: Vec<Item>,
    module_items: Vec<ItemId>,
    locals: Vec<LocalInfo>,
    refs: Vec<RefInfo>,
    pub diagnostics: rill_diagnostics::Diagnostics,
}

impl Hir {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn item(&self, id: ItemId) -> &Item {
        &self.items[id.0 as usize]
    }

    pub fn try_item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id.0 as usize)
    }

    /// Module-level items, in declaration order.
    pub fn module_items(&self) -> impl Iterator<Item = &Item> {
        self.module_items.iter().map(|id| self.item(*id))
    }

    pub fn module_item_ids(&self) -> &[ItemId] {
        &self.module_items
    }

    pub fn locals(&self) -> &[LocalInfo] {
        &self.locals
    }

    pub fn local(&self, id: LocalId) -> &LocalInfo {
        &self.locals[id.0 as usize]
    }

    pub fn refs(&self) -> &[RefInfo] {
        &self.refs
    }

    /// The item a module-level name refers to, ignoring the namespace.
    pub fn named_items(&self, name: Symbol) -> Vec<ItemId> {
        self.module_items
            .iter()
            .copied()
            .filter(|id| self.item(*id).name == Some(name))
            .collect()
    }

    /// References whose recorded name is `name`, in source order.
    pub fn refs_named(&self, name: Symbol) -> impl Iterator<Item = &RefInfo> {
        self.refs.iter().filter(move |r| r.name == name)
    }
}

/// Builder used by [`crate::lower`]; kept here so the tables stay private.
#[derive(Default)]
pub(crate) struct HirBuilder {
    hir: Hir,
}

impl HirBuilder {
    /// The item table, for passes that run while the HIR is being built.
    pub(crate) fn items(&self) -> &[Item] {
        &self.hir.items
    }

    pub(crate) fn try_item(&self, id: ItemId) -> Option<&Item> {
        self.hir.try_item(id)
    }

    pub(crate) fn add_item(&mut self, item: Item, module_level: bool) -> ItemId {
        let id = item.id;
        debug_assert_eq!(id.0 as usize, self.hir.items.len());
        self.hir.items.push(item);
        if module_level {
            self.hir.module_items.push(id);
        }
        id
    }

    pub(crate) fn add_local(&mut self, info: LocalInfo) -> LocalId {
        let id = info.id;
        debug_assert_eq!(id.0 as usize, self.hir.locals.len());
        self.hir.locals.push(info);
        id
    }

    pub(crate) fn add_ref(&mut self, info: RefInfo) {
        self.hir.refs.push(info);
    }

    pub(crate) fn next_item_id(&self) -> ItemId {
        ItemId(self.hir.items.len() as u32)
    }

    /// Fills a slot created by the declaration pass.
    /// Records the declaring name on a slot created by the declaration pass
    /// (impls declare no module-level name, but their head names the trait).
    pub(crate) fn set_item_name(&mut self, id: ItemId, name: Option<Symbol>) {
        if let Some(name) = name {
            let item = &mut self.hir.items[id.0 as usize];
            if item.name.is_none() {
                item.name = Some(name);
            }
        }
    }

    pub(crate) fn set_item(&mut self, id: ItemId, kind: ItemKind, span: Span) {
        let item = &mut self.hir.items[id.0 as usize];
        debug_assert!(
            matches!(item.kind, ItemKind::Placeholder),
            "item slot filled twice"
        );
        item.kind = kind;
        item.span = span;
    }

    pub(crate) fn next_local_id(&self) -> LocalId {
        LocalId(self.hir.locals.len() as u32)
    }

    pub(crate) fn finish(self, diagnostics: rill_diagnostics::Diagnostics) -> Hir {
        debug_assert!(
            self.hir
                .items
                .iter()
                .all(|item| !matches!(item.kind, ItemKind::Placeholder)),
            "a declared item was never lowered"
        );
        Hir {
            diagnostics,
            ..self.hir
        }
    }
}
