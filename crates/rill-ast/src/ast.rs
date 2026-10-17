//! Typed AST node definitions (PLAN.md §2, §6.1).
//!
//! The AST is produced by [`crate::lower_module`] from a lossless CST and then
//! rewritten by the builder-desugaring pass in [`crate::desugar`]. Every node
//! keeps its source [`Span`]; generated nodes use the span of the construct
//! they replace.

use rill_span::{Span, Symbol};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ident {
    pub sym: Symbol,
    pub span: Span,
}

impl Ident {
    pub fn new(sym: Symbol, span: Span) -> Self {
        Ident { sym, span }
    }
}

/// Passing convention (§2.2). `let` is the default when absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Convention {
    Let,
    Inout,
    Sink,
    Set,
}

/// A whole file.
#[derive(Clone, Debug, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Use(UseDecl),
    Signature(SignatureDecl),
    Fn(FnDecl),
    Project(ProjectDecl),
    Type(TypeDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Builder(BuilderDecl),
    Extern(ExternDecl),
    Stmt(Stmt),
    Error(Span),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Use(d) => d.span,
            Item::Signature(d) => d.span,
            Item::Fn(d) => d.span,
            Item::Project(d) => d.span,
            Item::Type(d) => d.span,
            Item::Trait(d) => d.span,
            Item::Impl(d) => d.span,
            Item::Builder(d) => d.span,
            Item::Extern(d) => d.span,
            Item::Stmt(s) => s.span(),
            Item::Error(span) => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UseDecl {
    pub path: UsePath,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum UsePath {
    /// `use core::alloc::StackArena;`
    Plain(Vec<Ident>),
    /// `use core::alloc::{A, B};`
    Group {
        prefix: Vec<Ident>,
        names: Vec<Ident>,
    },
    /// `use "path";`
    Text { text: String },
}

#[derive(Clone, Debug)]
pub struct SignatureDecl {
    pub name: Ident,
    pub ty: Type,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    pub span: Span,
}

/// A `let` declaration: a function when it has parameter groups, a value
/// binding otherwise. The body is `None` for bodyless trait members.
#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: Ident,
    pub params: Vec<ParamGroup>,
    pub ret: Option<ParamSlot>,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    pub body: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ProjectDecl {
    pub name: Ident,
    pub params: Vec<ParamGroup>,
    pub ret: Option<ParamSlot>,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: Ident,
    pub params: Vec<TypeParam>,
    pub body: TypeBody,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeBody {
    Record(RecordDef),
    View(RecordDef),
    Variants(Vec<Variant>),
    Alias(Type),
    Error(Span),
}

#[derive(Clone, Debug)]
pub struct RecordDef {
    pub fields: Vec<FieldDef>,
    pub deinit: Option<DeinitClause>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub mutable: bool,
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct DeinitClause {
    pub binding: Option<Ident>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: Ident,
    pub payload: Vec<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TraitDecl {
    pub name: Ident,
    pub params: Vec<TypeParam>,
    pub constraints: Vec<Constraint>,
    pub members: Vec<Item>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImplDecl {
    pub name: Ident,
    pub params: Vec<TypeParam>,
    pub constraints: Vec<Constraint>,
    pub members: Vec<Item>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct BuilderDecl {
    pub name: Ident,
    pub params: Vec<TypeParam>,
    pub members: Vec<Item>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExternDecl {
    pub abi: Option<String>,
    pub inner: Box<Item>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: Ident,
    pub comptime_ty: Option<Type>,
    pub span: Span,
}

/// A `(name: Conv? Type)` parameter.
#[derive(Clone, Debug)]
pub struct ParamSlot {
    pub convention: Option<Convention>,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParamGroup {
    pub params: Vec<Param>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub comptime: bool,
    pub name: Option<Ident>,
    pub convention: Option<Convention>,
    pub ty: Option<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EffectRow {
    pub items: Vec<EffectItem>,
    pub span: Span,
}

impl EffectRow {
    pub fn empty() -> Self {
        EffectRow {
            items: Vec::new(),
            span: Span::DUMMY,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct EffectItem {
    pub name: Ident,
    pub args: Vec<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Constraint {
    /// `Type is Trait args...`
    Is {
        ty: Type,
        trait_path: Vec<Ident>,
        args: Vec<Type>,
        span: Span,
    },
    /// `name : <effects>`
    Effect {
        name: Ident,
        effects: EffectRow,
        span: Span,
    },
}

impl Constraint {
    pub fn span(&self) -> Span {
        match self {
            Constraint::Is { span, .. } | Constraint::Effect { span, .. } => *span,
        }
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Type {
    Path {
        segments: Vec<Ident>,
        span: Span,
    },
    App {
        ctor: Box<Type>,
        args: Vec<Type>,
        span: Span,
    },
    Tuple {
        elems: Vec<Type>,
        span: Span,
    },
    /// `[T]`, `[T; n]` — slices and fixed-size tensors (§2.3).
    Array {
        elem: Box<Type>,
        len: Option<Box<Expr>>,
        span: Span,
    },
    Fn {
        params: Vec<ParamSlot>,
        ret: Option<Box<Type>>,
        effects: EffectRow,
        span: Span,
    },
    /// `&raw T`, `&raw mut T`.
    Ptr {
        mutable: bool,
        pointee: Box<Type>,
        span: Span,
    },
    Dyn {
        inner: Box<Type>,
        span: Span,
    },
    Error(Span),
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Path { span, .. }
            | Type::App { span, .. }
            | Type::Tuple { span, .. }
            | Type::Array { span, .. }
            | Type::Fn { span, .. }
            | Type::Ptr { span, .. }
            | Type::Dyn { span, .. }
            | Type::Error(span) => *span,
        }
    }

    pub fn unit(span: Span) -> Self {
        Type::Tuple {
            elems: Vec::new(),
            span,
        }
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `var x: T = e;`
    Var {
        name: Ident,
        ty: Option<Type>,
        init: Expr,
        span: Span,
    },
    /// `set place = e;`
    Set {
        place: Expr,
        value: Expr,
        span: Span,
    },
    /// `yield inout? e;`
    Yield {
        convention: Option<Convention>,
        value: Expr,
        span: Span,
    },
    /// `x := e;` and loop/condition bindings.
    Bind {
        name: Pattern,
        op: BindOp,
        value: Expr,
        span: Span,
    },
    /// `let? x = e;`
    TryBind {
        name: Ident,
        value: Expr,
        span: Span,
    },
    /// `when (cond);`
    When {
        cond: Cond,
        span: Span,
    },
    /// `for (x := xs) body`
    For {
        pattern: Pattern,
        iter: Expr,
        body: Block,
        span: Span,
    },
    /// A `let` declaration at block scope.
    Let(FnDecl),
    /// A bare expression statement.
    Expr(Expr),
    Error(Span),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Var { span, .. }
            | Stmt::Set { span, .. }
            | Stmt::Yield { span, .. }
            | Stmt::Bind { span, .. }
            | Stmt::TryBind { span, .. }
            | Stmt::When { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Error(span) => *span,
            Stmt::Let(d) => d.span,
            Stmt::Expr(e) => e.span(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindOp {
    /// `:=`
    ColonEq,
    /// `:` — loop bindings (`for (x : xs)`).
    Colon,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The block's tail expression: `{ a; b }` — boxed because expressions
    /// contain blocks.
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

impl Block {
    pub fn empty(span: Span) -> Self {
        Block {
            stmts: Vec::new(),
            tail: None,
            span,
        }
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard(Span),
    Binding {
        name: Ident,
        span: Span,
    },
    Literal {
        text: String,
        span: Span,
    },
    Constructor {
        path: Vec<Ident>,
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

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiteralKind {
    Int,
    Float,
    Text,
    RawText,
    Char,
    Bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnaryOp {
    Neg,
    Not,
    /// `*p` — raw-pointer dereference (unsafe).
    Deref,
    Size,
    Shapeof,
    Reshape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinaryOp {
    Choice,         // |
    RangeInclusive, // ..
    RangeHalfOpen,  // ..<
    Or,             // or
    And,            // and
    Eq,             // =
    EqEq,           // ==
    NotEq,          // !=
    Lt,
    Gt,
    LtEq,
    GtEq,
    Flow,   // .
    Concat, // ++
    Add,    // +
    Sub,    // -
    BitXor, // ^
    BitOr,  // |&
    Mul,    // *
    Div,    // /
    Rem,    // %
    Shl,    // <<
    Shr,    // >>
    BitAnd, // &
}

/// Built-in lenient-logic handlers (PLAN.md §3.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerKind {
    Option,
    One,
    All,
}

/// Structured-concurrency forms (PLAN.md §5.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConcurrentKind {
    Sync,
    Race,
    Rush,
    Branch,
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub convention: Option<Convention>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Cond {
    Expr(Box<Expr>),
    /// `if (x := f[y])` / `when (x := e)`.
    Bind {
        pattern: Pattern,
        value: Box<Expr>,
        span: Span,
    },
}

impl Cond {
    pub fn span(&self) -> Span {
        match self {
            Cond::Expr(e) => e.span(),
            Cond::Bind { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Literal {
        kind: LiteralKind,
        text: String,
        span: Span,
    },
    Path {
        segments: Vec<Ident>,
        span: Span,
    },
    Tuple {
        elems: Vec<Expr>,
        span: Span,
    },
    Array {
        elems: Vec<Expr>,
        span: Span,
    },
    Block(Block),
    Lambda {
        params: Vec<Pattern>,
        body: Block,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
        span: Span,
    },
    /// `f[x]`, `xs[i]` — adjacent bracket application.
    Index {
        base: Box<Expr>,
        args: Vec<Arg>,
        span: Span,
    },
    /// `e?`
    Try {
        base: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// `e as T` — type cast / static layout reinterpretation.
    Cast {
        expr: Box<Expr>,
        ty: Type,
        span: Span,
    },
    /// `e is T` — trait/type test.
    Is {
        expr: Box<Expr>,
        ty: Type,
        span: Span,
    },
    If {
        cond: Cond,
        then_branch: Block,
        else_branch: Option<ElseBranch>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Comptime {
        body: Block,
        span: Span,
    },
    Unsafe {
        body: Block,
        span: Span,
    },
    Handler {
        kind: HandlerKind,
        body: Block,
        span: Span,
    },
    Concurrent {
        kind: ConcurrentKind,
        arms: Vec<Expr>,
        span: Span,
    },
    Error(Span),
}

#[derive(Clone, Debug)]
pub enum ElseBranch {
    Block(Block),
    If(Box<Expr>),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. }
            | Expr::Path { span, .. }
            | Expr::Tuple { span, .. }
            | Expr::Array { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::Call { span, .. }
            | Expr::Index { span, .. }
            | Expr::Try { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Cast { span, .. }
            | Expr::Is { span, .. }
            | Expr::If { span, .. }
            | Expr::Match { span, .. }
            | Expr::Comptime { span, .. }
            | Expr::Unsafe { span, .. }
            | Expr::Handler { span, .. }
            | Expr::Concurrent { span, .. }
            | Expr::Error(span) => *span,
            Expr::Block(b) => b.span,
        }
    }

    /// Convenience for synthesized nodes.
    pub fn error(span: Span) -> Self {
        Expr::Error(span)
    }
}
