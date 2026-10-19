//! `rill-hir` — name resolution and the high-level IR (PLAN.md §7.1, M1).
//!
//! ```text
//! desugared AST (rill-ast) -> Hir with Res/LocalId/ExprId (rill-hir)
//! ```
//!
//! [`lower`] resolves every name reference and records it in the HIR's flat
//! tables, so later phases can ask "what does this path mean?" and "which
//! locals exist in this item?" without re-walking syntax:
//!
//! ```ignore
//! let frontend = rill_ast::parse_and_lower(&source_map, file);
//! let hir = rill_hir::lower(&frontend);
//! for reference in hir.refs() {
//!     println!("{:?} -> {:?}", frontend.text_of(reference.name), reference.res);
//! }
//! ```
//!
//! Type *names* are deliberately left to `rill-ty`: HIR keeps type syntax
//! ([`Type`], [`TypeBody`]) and resolves only the value namespace, so the
//! inference engine owns type/trait resolution together with the constraint
//! solver.

pub mod cimport;
pub mod hir;
pub mod resolve;

pub use hir::{
    Arg, AttributeDef, Block, BuilderDef, CSource, Cond, ConstantDef, ElseBranch, Expr, ExprId,
    ExprKind, FnDef, Hir, ImplDef, IncludeDef, Item, ItemId, ItemKind, LocalId, LocalInfo,
    LocalKind, MatchArm, Namespace, Param, ParamSlot, Pattern, ProjectDef, RefInfo, RefKind, Res,
    SignatureDef, Stmt, TraitDef, TypeDef, UseDef,
};
pub use resolve::{is_builtin, lower, lower_with, Options, BUILTIN_NAMES};

// Type syntax and operator tags live in `rill-ast`; re-exported so consumers
// of HIR do not need a second import path.
pub use rill_ast::{
    BinaryOp, ConcurrentKind, Constraint, Convention, EffectRow, HandlerKind, LiteralKind, Type,
    TypeBody, UnaryOp, UsePath,
};

// `lower(&frontend)` is the entry point; the `Frontend` type is defined in
// `rill-ast`, so no inherent impl is possible here.
