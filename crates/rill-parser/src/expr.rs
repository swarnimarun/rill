//! Pratt expression parser (PLAN.md §2.5).
//!
//! Binding powers come from [`rill_syntax::binary_operator`]. Operands are
//! parsed as *application spines*: `f x y` becomes one `CallExpr` with the
//! callee first and one `Arg` per operand. Postfix brackets bind by adjacency
//! (`xs[i]` is an index, `xs [i]` is an application to an array literal).
//! Parenthesised argument lists — `f(a, b)` and `f (a, b)` — splice their
//! arguments into the call, so `rotate_point (inout t.position, t.rotation)`
//! passes two arguments, the first with convention `inout`.

use crate::{CompletedMarker, Parser};
use rill_syntax::{binary_operator, contextual_operator, Assoc, SyntaxKind as K};

impl<'a> Parser<'a> {
    /// Parses an expression at the lowest binding power.
    pub fn expr(&mut self) {
        self.expr_bp(1);
    }

    /// Precedence-climbing expression parser. Returns the completed node so
    /// callers (notably the `.`-flow and application tiers) can wrap it.
    pub fn expr_bp(&mut self, min_bp: u8) -> CompletedMarker {
        let mut lhs = self.application();
        let mut prev_non_assoc: Option<u8> = None;

        loop {
            let kind = self.operator_kind();
            let Some(op) = kind.and_then(binary_operator) else {
                break;
            };
            if op.bp < min_bp {
                break;
            }
            if prev_non_assoc == Some(op.bp) {
                self.error(format!(
                    "`{}` is non-associative; add parentheses",
                    op.spelling
                ));
            }

            let marker = lhs.precede(self);
            self.bump(); // operator token
            match op.kind {
                // `as` / `is` take a type, not an expression.
                K::KwAs | K::KwIs => self.parse_type(),
                _ => {
                    let next_min = match op.assoc {
                        Assoc::Left | Assoc::NonAssoc => op.bp + 1,
                        Assoc::Right => op.bp,
                    };
                    self.expr_bp(next_min);
                }
            }
            prev_non_assoc = if op.assoc == Assoc::NonAssoc {
                Some(op.bp)
            } else {
                None
            };
            lhs = marker.complete(self, K::BinaryExpr);
        }
        lhs
    }

    /// The current token kind if it acts as a binary operator here, mapping
    /// contextual `and` / `or` names onto their operator kinds.
    fn operator_kind(&self) -> Option<K> {
        let kind = self.kind();
        if kind == K::Name {
            if let Some(ctx) = contextual_operator(self.current_text()) {
                if matches!(ctx, K::KwAnd | K::KwOr) {
                    return Some(ctx);
                }
            }
            return None;
        }
        Some(kind)
    }

    /// Unary prefixes, then the primary, postfix chain and application spine.
    fn application(&mut self) -> CompletedMarker {
        let base = self.unary();
        let base = self.postfix_chain(base);
        self.application_spine(base)
    }

    /// `not`, `size`, `shapeof`, `reshape`, `-` and `*` (raw-pointer deref).
    fn unary(&mut self) -> CompletedMarker {
        let contextual_not =
            self.at(K::Name) && matches!(contextual_operator(self.current_text()), Some(K::KwNot));
        match self.kind() {
            K::KwSize | K::KwShapeof | K::KwReshape | K::Minus | K::Star => {
                let marker = self.marker();
                self.bump();
                self.unary_operand();
                marker.complete(self, K::UnaryExpr)
            }
            K::Name if contextual_not => {
                let marker = self.marker();
                self.bump();
                self.unary_operand();
                marker.complete(self, K::UnaryExpr)
            }
            _ => self.primary(),
        }
    }

    /// Operand of a prefix operator: primary + postfix + application, no
    /// binary operators (so `size x + 1` stays `(size x) + 1`).
    fn unary_operand(&mut self) {
        let base = self.primary();
        let base = self.postfix_chain(base);
        self.application_spine(base);
    }

    /// Adjacent postfix forms: `e[args]`, `e(args)`, `e?`.
    fn postfix_chain(&mut self, mut base: CompletedMarker) -> CompletedMarker {
        while self.adjacent() {
            match self.kind() {
                K::LBracket => {
                    let marker = base.precede(self);
                    self.arg_list(K::LBracket, K::RBracket);
                    base = marker.complete(self, K::IndexExpr);
                }
                K::LParen => {
                    let marker = base.precede(self);
                    self.arg_list(K::LParen, K::RParen);
                    base = marker.complete(self, K::CallExpr);
                }
                K::Question => {
                    let marker = base.precede(self);
                    self.bump();
                    base = marker.complete(self, K::TryExpr);
                }
                _ => break,
            }
        }
        base
    }

    /// Space-separated application: `f x y`, `f { ... }`, `f [a, b]`.
    fn application_spine(&mut self, mut lhs: CompletedMarker) -> CompletedMarker {
        loop {
            let starts_arg = self.kind().starts_expr();
            if !starts_arg {
                break;
            }
            // `match x { ... }`: the brace opens the match, not an argument.
            if self.block_args_suppressed() && self.at(K::LBrace) {
                break;
            }
            // Contextual operators are names: `a and b` is not an application.
            if self.at(K::Name)
                && matches!(
                    contextual_operator(self.current_text()),
                    Some(K::KwAnd) | Some(K::KwOr)
                )
            {
                break;
            }
            let marker = lhs.precede(self);
            let before = self.pos_marker();
            if self.at(K::LParen) {
                // Spaced argument list: `rotate_point (inout a, b)`.
                self.arg_list(K::LParen, K::RParen);
            } else {
                self.node(K::Arg, |p| {
                    p.convention();
                    let inner = p.primary();
                    p.postfix_chain(inner);
                });
            }
            debug_assert!(self.pos_marker() > before, "application made no progress");
            lhs = marker.complete(self, K::CallExpr);
        }
        lhs
    }

    /// One argument of a `(...)` / `[...]` call.
    fn arg(&mut self) {
        self.node(K::Arg, |p| {
            p.convention();
            p.expr();
        });
    }

    /// `open Arg ("," Arg)* close`
    fn arg_list(&mut self, open: K, close: K) {
        self.node(K::ArgList, |p| {
            p.expect(open, "an argument list");
            if !p.at(close) {
                p.arg();
                while p.eat(K::Comma) {
                    if p.at(close) {
                        break; // trailing comma
                    }
                    p.arg();
                }
            }
            p.expect(close, "a closing delimiter");
        });
    }

    /// A primary expression (no postfix, no application).
    fn primary(&mut self) -> CompletedMarker {
        match self.kind() {
            k if k.is_literal() => {
                let marker = self.marker();
                self.bump();
                marker.complete(self, K::LiteralExpr)
            }
            K::Name | K::Underscore => self.path_expr(),
            K::KwIf => self.if_expr(),
            K::KwMatch => self.match_expr(),
            K::KwComptime => {
                let marker = self.marker();
                self.bump();
                self.block();
                marker.complete(self, K::ComptimeExpr)
            }
            K::KwUnsafe => {
                let marker = self.marker();
                self.bump();
                self.block();
                marker.complete(self, K::UnsafeExpr)
            }
            K::LParen => self.paren_expr(),
            K::LBracket => self.array_literal(),
            K::LBrace => self.block_or_lambda(),
            K::KwSize | K::KwShapeof | K::KwReshape => self.unary(),
            _ => {
                self.error("expected an expression");
                // An empty marker keeps the event stream balanced; callers
                // detect the lack of progress and recover.
                self.marker().complete(self, K::LiteralExpr)
            }
        }
    }

    /// `name ("::" name)*` — also the callee of constructors (`Step::Yield x`).
    fn path_expr(&mut self) -> CompletedMarker {
        let marker = self.marker();
        self.bump();
        while self.at(K::ColonColon) {
            self.bump();
            if self.at(K::Name) {
                self.bump();
            } else {
                self.error("expected a name after `::`");
                break;
            }
        }
        marker.complete(self, K::PathExpr)
    }

    /// `(e)`, `(a, b)`, or a grouped convention argument `(inout e)`.
    fn paren_expr(&mut self) -> CompletedMarker {
        // `(inout e)` / `(sink e)` / `(set e)` / `(let e)`: a single argument
        // carrying a passing convention, as in `quicksort (inout pair.0)`.
        if matches!(self.nth(1), K::KwInout | K::KwSink | K::KwSet | K::KwLet) {
            let marker = self.marker();
            self.expect(K::LParen, "`(`");
            self.convention();
            self.expr();
            self.expect(K::RParen, "`)`");
            return marker.complete(self, K::Arg);
        }

        let marker = self.marker();
        self.expect(K::LParen, "`(`");
        if self.at(K::RParen) {
            self.bump(); // unit value `()`
            return marker.complete(self, K::ParenExpr);
        }
        self.expr();
        if self.eat(K::Comma) {
            while !self.at(K::RParen) && !self.at_end() {
                self.expr();
                if !self.eat(K::Comma) {
                    break;
                }
            }
            self.expect(K::RParen, "`)`");
            marker.complete(self, K::TupleExpr)
        } else {
            self.expect(K::RParen, "`)`");
            marker.complete(self, K::ParenExpr)
        }
    }

    /// `[e, ...]` — an array literal (only when *not* adjacent to a callee).
    fn array_literal(&mut self) -> CompletedMarker {
        let marker = self.marker();
        self.node(K::ArgList, |p| {
            p.expect(K::LBracket, "`[`");
            if !p.at(K::RBracket) {
                p.arg();
                while p.eat(K::Comma) {
                    if p.at(K::RBracket) {
                        break;
                    }
                    p.arg();
                }
            }
            p.expect(K::RBracket, "`]`");
        });
        marker.complete(self, K::ArrayExpr)
    }

    /// `{ ... }` as a block or, when it starts with `params ->`, a lambda.
    fn block_or_lambda(&mut self) -> CompletedMarker {
        if !self.lambda_ahead() {
            return self.block_marker();
        }
        let marker = self.marker();
        self.node(K::LambdaParams, |p| {
            p.expect(K::LBrace, "`{`");
            loop {
                if p.at(K::Name) || p.at(K::Underscore) {
                    p.bump();
                } else {
                    p.error("expected a lambda parameter");
                    break;
                }
                if !p.eat(K::Comma) {
                    break;
                }
            }
            p.expect(K::Arrow, "`->`");
        });
        self.node(K::Block, |p| {
            p.block_contents();
            p.expect(K::RBrace, "`}`");
        });
        marker.complete(self, K::LambdaExpr)
    }

    fn block_marker(&mut self) -> CompletedMarker {
        self.block()
    }

    /// Lookahead: `{` Name ("," Name)* "->" starts a lambda.
    fn lambda_ahead(&self) -> bool {
        let mut i = 1usize;
        let mut expect_name = true;
        loop {
            match self.nth(i) {
                k if expect_name && (k == K::Name || k == K::Underscore) => {
                    i += 1;
                    expect_name = false;
                }
                K::Comma if !expect_name => {
                    i += 1;
                    expect_name = true;
                }
                K::Arrow if !expect_name => return true,
                _ => return false,
            }
        }
    }

    /// `if (cond) block (else block | else if ...)?`
    fn if_expr(&mut self) -> CompletedMarker {
        let marker = self.marker();
        self.bump(); // `if`
        self.expect(K::LParen, "`(`");
        if (self.at(K::Name) || self.at(K::Underscore)) && self.nth(1) == K::ColonEq {
            // `if (x := f[y])` — failable binding condition.
            self.bind_stmt();
        } else {
            self.expr();
        }
        self.expect(K::RParen, "`)`");
        self.block();
        if self.eat(K::KwElse) {
            if self.at(K::KwIf) {
                self.if_expr();
            } else {
                self.block();
            }
        }
        marker.complete(self, K::IfExpr)
    }

    /// `match scrutinee { (|)? pat (if guard)? -> expr }`
    fn match_expr(&mut self) -> CompletedMarker {
        let marker = self.marker();
        self.bump(); // `match`
        self.with_no_block_args(|p| p.expr());
        self.expect(K::LBrace, "`{`");
        while !self.at(K::RBrace) && !self.at_end() {
            let before = self.pos_marker();
            self.node(K::MatchArm, |p| {
                p.eat(K::Pipe);
                p.match_pat();
                if p.eat(K::KwIf) {
                    p.expr();
                }
                p.expect(K::Arrow, "`->`");
                p.expr();
                if !p.eat(K::Comma) {
                    p.eat(K::Semi);
                }
            });
            if self.pos_marker() == before {
                self.bump();
            }
        }
        self.expect(K::RBrace, "`}`");
        marker.complete(self, K::MatchExpr)
    }

    /// Constructor / literal / tuple patterns, optionally applied to atoms.
    fn match_pat(&mut self) {
        self.node(K::MatchPat, |p| {
            match p.kind() {
                K::Name | K::Underscore => {
                    p.bump();
                    while p.at(K::ColonColon) {
                        p.bump();
                        if p.at(K::Name) {
                            p.bump();
                        } else {
                            p.error("expected a name after `::`");
                            break;
                        }
                    }
                    // Pattern arguments: `Some x`, `Push x xs`, `Pair (a, b)`.
                    while p.kind().starts_pattern_atom() {
                        if p.at(K::LParen) {
                            p.match_pat();
                        } else {
                            p.bump();
                        }
                    }
                }
                k if k.is_literal() => {
                    p.bump();
                }
                K::LParen => {
                    p.bump();
                    if !p.at(K::RParen) {
                        p.match_pat();
                        while p.eat(K::Comma) {
                            if p.at(K::RParen) {
                                break;
                            }
                            p.match_pat();
                        }
                    }
                    p.expect(K::RParen, "`)`");
                }
                _ => p.error("expected a pattern"),
            }
        });
    }
}

/// Kinds that may follow a constructor pattern.
trait StartsPatternAtom {
    fn starts_pattern_atom(self) -> bool;
}

impl StartsPatternAtom for K {
    fn starts_pattern_atom(self) -> bool {
        matches!(
            self,
            K::Name | K::Underscore | K::IntLiteral | K::FloatLiteral | K::CharLiteral | K::LParen
        )
    }
}
