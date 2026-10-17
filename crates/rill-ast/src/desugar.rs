//! Computation-builder desugaring (PLAN.md §6.1, §7.2).
//!
//! Runs after lowering, before type inference, and performs three rewrites:
//!
//! 1. **Builder brace forms** — `b { ... }` where `b` is a declared builder
//!    (or one of the standard builders shipped by `core`) is rewritten into
//!    the `bind` / `try_bind` / `yield_val` / `for_each` / `combine` / `delay`
//!    / `ret` protocol exactly as tabulated in §6.1.
//! 2. **Built-in forms** — `option { ... }`, `one { ... }`, `all { ... }`
//!    become [`Expr::Handler`]; `sync` / `race` / `rush` / `branch` become
//!    [`Expr::Concurrent`]. Their semantics are provided by the compiler
//!    (§3.3, §5.3), so they are *not* method-desugared.
//! 3. **`and` / `or`** — short-circuit operators expand to `if` (§7.2).
//!
//! `when (cond)` has no row in the §6.1 table. Inside a builder block it is
//! treated as a guard and expands to
//! `b.combine (if (cond) { () } else { fail }) (b.delay { rest })`, using the
//! failure primitive `fail` of §5.1. Inside built-in handlers the guard is kept
//! as a [`Stmt::When`] for the handler to interpret.

use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_span::{Interner, Span};

use crate::ast::*;

/// Builders provided by `core` (PLAN §6.2, §6.3, and the §6.1 DSL list).
pub const STANDARD_BUILDERS: &[&str] = &["seq", "simd", "gpu", "arena", "query"];

/// Rewrites `module` in place. Requires the interner that owns the module's
/// symbols, since desugaring introduces protocol method names.
pub fn run(module: &mut Module, interner: &mut Interner) -> Diagnostics {
    let names = collect_builders(module, interner);
    let mut pass = Desugar {
        builders: names,
        interner,
        diags: Diagnostics::new(),
    };
    for item in module.items.iter_mut() {
        pass.item(item);
    }
    std::mem::take(&mut pass.diags)
}

fn collect_builders(module: &Module, interner: &Interner) -> Vec<String> {
    let mut names: Vec<String> = STANDARD_BUILDERS.iter().map(|s| s.to_string()).collect();
    for item in &module.items {
        if let Item::Builder(decl) = item {
            let name = interner.get(decl.name.sym).to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

struct Desugar<'a> {
    builders: Vec<String>,
    interner: &'a mut Interner,
    diags: Diagnostics,
}

impl<'a> Desugar<'a> {
    // -- helpers -----------------------------------------------------------

    fn sym(&mut self, text: &str) -> rill_span::Symbol {
        self.interner.intern(text)
    }

    fn ident(&mut self, text: &str, span: Span) -> Ident {
        Ident::new(self.sym(text), span)
    }

    fn path(&mut self, segments: &[&str], span: Span) -> Expr {
        let segments = segments.iter().map(|s| self.ident(s, span)).collect();
        Expr::Path { segments, span }
    }

    /// `callee (prot)` applied to `args`.
    fn protocol_call(
        &mut self,
        builder: &[Ident],
        method: &str,
        args: Vec<Expr>,
        span: Span,
    ) -> Expr {
        let mut segments: Vec<Ident> = builder.to_vec();
        segments.push(self.ident(method, span));
        let callee = Expr::Path { segments, span };
        self.call(callee, args, span)
    }

    fn call(&mut self, callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
        let args = args
            .into_iter()
            .map(|value| {
                let arg_span = value_span(&value, span);
                Arg {
                    convention: None,
                    value,
                    span: arg_span,
                }
            })
            .collect();
        Expr::Call {
            callee: Box::new(callee),
            args,
            span,
        }
    }

    /// A one-expression closure: `{ params -> body }`.
    fn lambda(&mut self, params: Vec<Pattern>, body: Expr, span: Span) -> Expr {
        let body_block = Block {
            stmts: Vec::new(),
            tail: Some(Box::new(body)),
            span,
        };
        Expr::Lambda {
            params,
            body: body_block,
            span,
        }
    }

    /// `b.delay { body }`
    fn delay(&mut self, builder: &[Ident], body: Expr, span: Span) -> Expr {
        let lambda = self.lambda(Vec::new(), body, span);
        self.protocol_call(builder, "delay", vec![lambda], span)
    }

    /// The unit value `()`.
    fn unit(span: Span) -> Expr {
        Expr::Tuple {
            elems: Vec::new(),
            span,
        }
    }

    fn bool_literal(value: bool, span: Span) -> Expr {
        Expr::Literal {
            kind: LiteralKind::Bool,
            text: if value { "true" } else { "false" }.to_string(),
            span,
        }
    }

    /// Text of a single-segment path expression, if it is one.
    fn single_name(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Path { segments, .. } if segments.len() == 1 => {
                Some(self.interner.get(segments[0].sym).to_string())
            }
            _ => None,
        }
    }

    fn is_builder(&self, name: &str) -> bool {
        self.builders.iter().any(|b| b == name)
    }

    // -- builder protocol (§6.1) ------------------------------------------

    /// Desugars a builder block: `[[b { stmts; tail }]]`.
    fn builder_body(&mut self, builder: &[Ident], block: &Block, span: Span) -> Expr {
        let mut acc = match &block.tail {
            // `if (c) { t } else { f }` as the block's tail keeps its branch
            // structure: each branch is its own builder body (§6.1).
            Some(tail) => match &**tail {
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                    span: if_span,
                } => self.builder_if(
                    builder,
                    &cond.clone(),
                    &then_branch.clone(),
                    else_branch.as_ref(),
                    *if_span,
                ),
                other => {
                    let other = other.clone();
                    self.protocol_call(builder, "ret", vec![other], span)
                }
            },
            None => {
                let unit = Self::unit(span);
                self.protocol_call(builder, "ret", vec![unit], span)
            }
        };
        for stmt in block.stmts.iter().rev() {
            acc = self.builder_stmt(builder, stmt, acc, span);
        }
        acc
    }

    /// `stmt1; rest` -> `b.combine ([[stmt1]]) (b.delay { [[rest]] })`.
    fn builder_stmt(&mut self, builder: &[Ident], stmt: &Stmt, cont: Expr, span: Span) -> Expr {
        let combined = |this: &mut Self, head: Expr, cont: Expr| {
            let delayed = this.delay(builder, cont, span);
            this.protocol_call(builder, "combine", vec![head, delayed], span)
        };
        match stmt {
            Stmt::Bind { name, value, .. } => {
                let value = value.clone();
                let lambda = self.lambda(vec![name.clone()], cont, span);
                self.protocol_call(builder, "bind", vec![value, lambda], span)
            }
            Stmt::TryBind {
                name,
                value,
                span: stmt_span,
            } => {
                let value = value.clone();
                let pattern = Pattern::Binding {
                    name: *name,
                    span: *stmt_span,
                };
                let lambda = self.lambda(vec![pattern], cont, span);
                self.protocol_call(builder, "try_bind", vec![value, lambda], span)
            }
            Stmt::Yield { value, .. } => {
                let value = value.clone();
                let yielded = self.protocol_call(builder, "yield_val", vec![value], span);
                combined(self, yielded, cont)
            }
            Stmt::For {
                pattern,
                iter,
                body,
                ..
            } => {
                let iter = iter.clone();
                let inner = self.builder_body(builder, body, span);
                let lambda = self.lambda(vec![pattern.clone()], inner, span);
                let for_each = self.protocol_call(builder, "for_each", vec![iter, lambda], span);
                combined(self, for_each, cont)
            }
            Stmt::When { cond, .. } => {
                let guard = self.guard_expr(cond, span);
                combined(self, guard, cont)
            }
            Stmt::Expr(Expr::If {
                cond,
                then_branch,
                else_branch,
                span: if_span,
            }) => {
                let desugared =
                    self.builder_if(builder, cond, then_branch, else_branch.as_ref(), *if_span);
                combined(self, desugared, cont)
            }
            Stmt::Expr(expr) => {
                let expr = expr.clone();
                combined(self, expr, cont)
            }
            Stmt::Error(_) => cont,
            other => {
                let span = other.span();
                self.diags.push(Diagnostic::error(
                    span,
                    "this statement has no builder-protocol desugaring (PLAN.md §6.1)",
                ));
                cont
            }
        }
    }

    /// `if (c) { t } else { f }` inside a builder: both branches become builder
    /// bodies; a missing `else` is `b.ret (())`.
    fn builder_if(
        &mut self,
        builder: &[Ident],
        cond: &Cond,
        then_branch: &Block,
        else_branch: Option<&ElseBranch>,
        span: Span,
    ) -> Expr {
        let cond = self.cond(cond.clone());
        let then_body = self.builder_body(builder, then_branch, span);
        let then_block = Block {
            stmts: Vec::new(),
            tail: Some(Box::new(then_body)),
            span,
        };
        let else_branch = match else_branch {
            Some(ElseBranch::Block(block)) => {
                let body = self.builder_body(builder, block, span);
                Some(ElseBranch::Block(Block {
                    stmts: Vec::new(),
                    tail: Some(Box::new(body)),
                    span,
                }))
            }
            Some(ElseBranch::If(_)) => {
                // `else if` chains are rare inside builders; keep them as-is.
                let inner = self.expr_block_or_if(else_branch.cloned(), span);
                Some(inner)
            }
            None => {
                let unit = Self::unit(span);
                let ret = self.protocol_call(builder, "ret", vec![unit], span);
                Some(ElseBranch::Block(Block {
                    stmts: Vec::new(),
                    tail: Some(Box::new(ret)),
                    span,
                }))
            }
        };
        Expr::If {
            cond,
            then_branch: then_block,
            else_branch,
            span,
        }
    }

    fn expr_block_or_if(&mut self, branch: Option<ElseBranch>, span: Span) -> ElseBranch {
        match branch {
            Some(ElseBranch::Block(b)) => ElseBranch::Block(b),
            Some(ElseBranch::If(e)) => ElseBranch::If(e),
            None => ElseBranch::Block(Block::empty(span)),
        }
    }

    /// `if (cond) { () } else { fail }` — the guard mapping of `when`.
    fn guard_expr(&mut self, cond: &Cond, span: Span) -> Expr {
        let cond = self.cond(cond.clone());
        let unit = Self::unit(span);
        let fail = self.path(&["fail"], span);
        Expr::If {
            cond,
            then_branch: Block {
                stmts: Vec::new(),
                tail: Some(Box::new(unit)),
                span,
            },
            else_branch: Some(ElseBranch::Block(Block {
                stmts: Vec::new(),
                tail: Some(Box::new(fail)),
                span,
            })),
            span,
        }
    }

    fn cond(&mut self, cond: Cond) -> Cond {
        match cond {
            Cond::Expr(e) => Cond::Expr(Box::new(self.expr(*e))),
            Cond::Bind {
                pattern,
                value,
                span,
            } => Cond::Bind {
                pattern,
                value: Box::new(self.expr(*value)),
                span,
            },
        }
    }

    // -- expression rewriting ---------------------------------------------

    fn expr(&mut self, expr: Expr) -> Expr {
        match expr {
            Expr::Call {
                callee,
                mut args,
                span,
            } => {
                // Rewrite arguments first, so nested brace forms desugar.
                for arg in args.iter_mut() {
                    let value = std::mem::replace(&mut arg.value, Expr::error(arg.span));
                    arg.value = self.expr(value);
                }
                let callee = self.expr(*callee);
                // `name { block }` — a brace form.
                if let Some(name) = self.single_name(&callee) {
                    if args.len() == 1 && args[0].convention.is_none() {
                        if let Expr::Block(block) = &args[0].value {
                            let block = block.clone();
                            if let Some(kind) = handler_kind(&name) {
                                return Expr::Handler {
                                    kind,
                                    body: self.block(block),
                                    span,
                                };
                            }
                            if let Some(kind) = concurrent_kind(&name) {
                                return self.concurrent(kind, block, span);
                            }
                            if self.is_builder(&name) {
                                let builder = match callee {
                                    Expr::Path { segments, .. } => segments,
                                    _ => Vec::new(),
                                };
                                return self.builder_body(&builder, &block, span);
                            }
                        }
                    }
                }
                Expr::Call {
                    callee: Box::new(callee),
                    args,
                    span,
                }
            }
            Expr::Index {
                base,
                mut args,
                span,
            } => {
                for arg in args.iter_mut() {
                    let value = std::mem::replace(&mut arg.value, Expr::error(arg.span));
                    arg.value = self.expr(value);
                }
                let base = self.expr(*base);
                Expr::Index {
                    base: Box::new(base),
                    args,
                    span,
                }
            }
            Expr::Try { base, span } => Expr::Try {
                base: Box::new(self.expr(*base)),
                span,
            },
            Expr::Unary { op, operand, span } => Expr::Unary {
                op,
                operand: Box::new(self.expr(*operand)),
                span,
            },
            Expr::Binary { op, lhs, rhs, span } => {
                let lhs = self.expr(*lhs);
                let rhs = self.expr(*rhs);
                match op {
                    // `and` / `or` -> `if` (§7.2).
                    BinaryOp::And => Expr::If {
                        cond: Cond::Expr(Box::new(lhs)),
                        then_branch: Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(rhs)),
                            span,
                        },
                        else_branch: Some(ElseBranch::Block(Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(Self::bool_literal(false, span))),
                            span,
                        })),
                        span,
                    },
                    BinaryOp::Or => Expr::If {
                        cond: Cond::Expr(Box::new(lhs)),
                        then_branch: Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(Self::bool_literal(true, span))),
                            span,
                        },
                        else_branch: Some(ElseBranch::Block(Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(rhs)),
                            span,
                        })),
                        span,
                    },
                    _ => Expr::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                        span,
                    },
                }
            }
            Expr::Tuple { elems, span } => Expr::Tuple {
                elems: elems.into_iter().map(|e| self.expr(e)).collect(),
                span,
            },
            Expr::Array { elems, span } => Expr::Array {
                elems: elems.into_iter().map(|e| self.expr(e)).collect(),
                span,
            },
            Expr::Block(block) => Expr::Block(self.block(block)),
            Expr::Lambda { params, body, span } => Expr::Lambda {
                params,
                body: self.block(body),
                span,
            },
            Expr::If {
                cond,
                then_branch,
                else_branch,
                span,
            } => {
                let cond = self.cond(cond);
                let then_branch = self.block(then_branch);
                let else_branch = else_branch.map(|b| match b {
                    ElseBranch::Block(block) => ElseBranch::Block(self.block(block)),
                    ElseBranch::If(e) => ElseBranch::If(Box::new(self.expr(*e))),
                });
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                    span,
                }
            }
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                let scrutinee = self.expr(*scrutinee);
                let arms = arms
                    .into_iter()
                    .map(|arm| {
                        let body = self.expr(arm.body);
                        let guard = arm.guard.map(|g| self.expr(g));
                        MatchArm { body, guard, ..arm }
                    })
                    .collect();
                Expr::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    span,
                }
            }
            Expr::Comptime { body, span } => Expr::Comptime {
                body: self.block(body),
                span,
            },
            Expr::Unsafe { body, span } => Expr::Unsafe {
                body: self.block(body),
                span,
            },
            other => other,
        }
    }

    fn concurrent(&mut self, kind: ConcurrentKind, block: Block, span: Span) -> Expr {
        let mut arms = Vec::new();
        for stmt in block.stmts {
            match stmt {
                Stmt::Expr(e) => arms.push(self.expr(e)),
                other => {
                    self.diags.push(Diagnostic::error(
                        other.span(),
                        "concurrency arms must be expressions (PLAN.md §2.5 `concurrent`)",
                    ));
                }
            }
        }
        if let Some(tail) = block.tail {
            arms.push(self.expr(*tail));
        }
        Expr::Concurrent { kind, arms, span }
    }

    fn block(&mut self, block: Block) -> Block {
        let stmts = block.stmts.into_iter().map(|s| self.stmt(s)).collect();
        let tail = block.tail.map(|t| Box::new(self.expr(*t)));
        Block {
            stmts,
            tail,
            span: block.span,
        }
    }

    fn stmt(&mut self, stmt: Stmt) -> Stmt {
        match stmt {
            Stmt::Var {
                name,
                ty,
                init,
                span,
            } => Stmt::Var {
                name,
                ty: ty.map(|t| self.ty(t)),
                init: self.expr(init),
                span,
            },
            Stmt::Set { place, value, span } => Stmt::Set {
                place: self.expr(place),
                value: self.expr(value),
                span,
            },
            Stmt::Yield {
                convention,
                value,
                span,
            } => Stmt::Yield {
                convention,
                value: self.expr(value),
                span,
            },
            Stmt::Bind {
                name,
                op,
                value,
                span,
            } => Stmt::Bind {
                name,
                op,
                value: self.expr(value),
                span,
            },
            Stmt::TryBind { name, value, span } => Stmt::TryBind {
                name,
                value: self.expr(value),
                span,
            },
            Stmt::When { cond, span } => Stmt::When {
                cond: self.cond(cond),
                span,
            },
            Stmt::For {
                pattern,
                iter,
                body,
                span,
            } => Stmt::For {
                pattern,
                iter: self.expr(iter),
                body: self.block(body),
                span,
            },
            Stmt::Let(mut decl) => {
                if let Some(body) = decl.body.take() {
                    decl.body = Some(self.expr(body));
                }
                Stmt::Let(decl)
            }
            Stmt::Expr(e) => Stmt::Expr(self.expr(e)),
            Stmt::Error(span) => Stmt::Error(span),
        }
    }

    fn ty(&mut self, ty: Type) -> Type {
        match ty {
            Type::Array { elem, len, span } => {
                let len = len.map(|l| Box::new(self.expr(*l)));
                Type::Array {
                    elem: Box::new(self.ty(*elem)),
                    len,
                    span,
                }
            }
            Type::App { ctor, args, span } => Type::App {
                ctor: Box::new(self.ty(*ctor)),
                args: args.into_iter().map(|a| self.ty(a)).collect(),
                span,
            },
            Type::Tuple { elems, span } => Type::Tuple {
                elems: elems.into_iter().map(|t| self.ty(t)).collect(),
                span,
            },
            Type::Fn {
                params,
                ret,
                effects,
                span,
            } => Type::Fn {
                params: params
                    .into_iter()
                    .map(|p| ParamSlot {
                        ty: self.ty(p.ty),
                        ..p
                    })
                    .collect(),
                ret: ret.map(|r| Box::new(self.ty(*r))),
                effects,
                span,
            },
            Type::Ptr {
                mutable,
                pointee,
                span,
            } => Type::Ptr {
                mutable,
                pointee: Box::new(self.ty(*pointee)),
                span,
            },
            Type::Dyn { inner, span } => Type::Dyn {
                inner: Box::new(self.ty(*inner)),
                span,
            },
            other => other,
        }
    }

    fn item(&mut self, item: &mut Item) {
        match item {
            Item::Fn(decl) => {
                self.params(&mut decl.params);
                if let Some(ret) = decl.ret.take() {
                    decl.ret = Some(ParamSlot {
                        ty: self.ty(ret.ty),
                        ..ret
                    });
                }
                if let Some(body) = decl.body.take() {
                    decl.body = Some(self.expr(body));
                }
            }
            Item::Project(decl) => {
                self.params(&mut decl.params);
                if let Some(ret) = decl.ret.take() {
                    decl.ret = Some(ParamSlot {
                        ty: self.ty(ret.ty),
                        ..ret
                    });
                }
                let body = std::mem::replace(&mut decl.body, Expr::error(decl.span));
                decl.body = self.expr(body);
            }
            Item::Type(decl) => {
                let body = std::mem::replace(&mut decl.body, TypeBody::Error(decl.span));
                decl.body = match body {
                    TypeBody::Alias(ty) => TypeBody::Alias(self.ty(ty)),
                    other => other,
                };
            }
            Item::Trait(decl) => {
                for member in decl.members.iter_mut() {
                    self.item(member);
                }
            }
            Item::Impl(decl) => {
                for member in decl.members.iter_mut() {
                    self.item(member);
                }
            }
            Item::Builder(decl) => {
                for member in decl.members.iter_mut() {
                    self.item(member);
                }
            }
            Item::Extern(decl) => self.item(&mut decl.inner),
            Item::Signature(decl) => {
                let ty = std::mem::replace(&mut decl.ty, Type::Error(decl.span));
                decl.ty = self.ty(ty);
            }
            Item::Stmt(stmt) => {
                let taken = std::mem::replace(stmt, Stmt::Error(Span::DUMMY));
                *stmt = self.stmt(taken);
            }
            Item::Use(_) | Item::Error(_) => {}
        }
    }

    fn params(&mut self, groups: &mut [ParamGroup]) {
        for group in groups.iter_mut() {
            for param in group.params.iter_mut() {
                if let Some(ty) = param.ty.take() {
                    param.ty = Some(self.ty(ty));
                }
            }
        }
    }
}

fn handler_kind(name: &str) -> Option<HandlerKind> {
    Some(match name {
        "option" => HandlerKind::Option,
        "one" => HandlerKind::One,
        "all" => HandlerKind::All,
        _ => return None,
    })
}

fn concurrent_kind(name: &str) -> Option<ConcurrentKind> {
    Some(match name {
        "sync" => ConcurrentKind::Sync,
        "race" => ConcurrentKind::Race,
        "rush" => ConcurrentKind::Rush,
        "branch" => ConcurrentKind::Branch,
        _ => return None,
    })
}

fn value_span(value: &Expr, fallback: Span) -> Span {
    let span = value.span();
    if span.is_dummy() {
        fallback
    } else {
        span
    }
}
