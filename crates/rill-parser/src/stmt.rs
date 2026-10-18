//! Statements and blocks (PLAN.md §2.2, §2.5).
//!
//! A [`SyntaxKind::Block`] holds statement nodes plus an optional *tail
//! expression*: the final element is a bare expression node when it is not
//! terminated by `;`, exactly as in the examples of §6, §11.

use crate::Parser;
use rill_syntax::SyntaxKind as K;

impl<'a> Parser<'a> {
    /// One statement (or declaration) at module or block level.
    pub fn statement(&mut self) {
        match self.kind() {
            K::At => self.attribute_decl(),
            K::KwUse => self.use_decl(),
            K::KwType => self.type_decl(),
            K::KwTrait => self.trait_decl(),
            K::KwImpl => self.impl_decl(),
            K::KwBuilder => self.builder_decl(),
            K::KwExtern => self.extern_decl(),
            K::KwProject => self.project_decl(),
            K::KwLet => {
                if self.nth(1) == K::Question {
                    self.try_bind_stmt();
                } else {
                    self.let_decl();
                }
            }
            K::KwVar => self.var_stmt(),
            K::KwSet => self.set_stmt(),
            K::KwYield => self.yield_stmt(),
            K::KwWhen => self.when_stmt(),
            K::Name if self.current_text() == "while" && self.nth(1) == K::LParen => {
                self.while_stmt();
                // A block-bodied loop may be followed by `;`.
                self.eat(K::Semi);
            }
            K::Name if self.current_text() == "break" => self.break_stmt(),
            K::KwFor => {
                self.for_stmt();
                // The body may already have consumed the `;`, so its trailing
                // semicolon is opportunistic rather than required.
                self.eat(K::Semi);
            }
            K::Name | K::Underscore if self.nth(1) == K::ColonEq => {
                self.bind_stmt();
                self.semi_or_block_end();
            }
            K::Name if self.nth(1) == K::Colon => self.signature_decl(),
            _ => self.expr_statement_or_tail(),
        }
    }

    /// `{ statement* tail_expr? }` — braces included.
    pub fn block(&mut self) -> crate::CompletedMarker {
        self.node(K::Block, |p| {
            p.expect(K::LBrace, "`{`");
            p.block_contents();
            p.expect(K::RBrace, "`}`");
        })
    }

    /// Statements up to (but excluding) the closing `}`.
    pub(crate) fn block_contents(&mut self) {
        while !self.at(K::RBrace) && !self.at_end() {
            let before = self.pos_marker();
            self.statement();
            if self.pos_marker() == before {
                self.bump();
            }
        }
    }

    /// Expression statement with `;`, or the block's tail expression.
    fn expr_statement_or_tail(&mut self) {
        let mut tail = false;
        let marker = self.node(K::ExprStmt, |p| {
            p.expr();
            if p.eat(K::Semi) {
                // Ordinary statement.
            } else if p.at(K::RBrace) || p.at_end() {
                // Tail expression: drop the ExprStmt wrapper.
                tail = true;
            } else if p.last_kind() == K::RBrace {
                // Block-like forms (`if`, `match`, blocks, brace DSLs) may omit `;`.
            } else {
                p.error("expected `;` after the expression");
            }
        });
        if tail {
            self.unwrap_node(marker);
        }
    }

    /// `name := expr ;` (also used for `if`/`for` bindings).
    pub fn bind_stmt(&mut self) {
        self.node(K::BindStmt, |p| {
            if p.at(K::Name) || p.at(K::Underscore) {
                p.bump();
            } else {
                p.error("expected a binding name");
            }
            p.expect(K::ColonEq, "`:=`");
            p.expr();
        });
    }

    /// `let? name = expr ;` — the `Try`/failable binding of §6.1.
    fn try_bind_stmt(&mut self) {
        self.node(K::TryBindStmt, |p| {
            p.bump(); // `let`
            p.expect(K::Question, "`?`");
            if p.at(K::Name) || p.at(K::Underscore) {
                p.bump();
            } else {
                p.error("expected a name after `let?`");
            }
            p.expect(K::Assign, "`=`");
            p.expr();
            p.semi_or_block_end();
        });
    }

    /// `var name (: type)? = expr ;`
    fn var_stmt(&mut self) {
        self.node(K::VarStmt, |p| {
            p.bump(); // `var`
            if p.at(K::Name) || p.at(K::Underscore) {
                p.bump();
            } else {
                p.error("expected a variable name");
            }
            if p.eat(K::Colon) {
                p.parse_type();
            }
            p.expect(K::Assign, "`=`");
            p.expr();
            p.semi_or_block_end();
        });
    }

    /// `set place = expr ;` — `place` never swallows the `=` (no assignment
    /// expression in Rill, so `=` here is the store operator, not equality).
    fn set_stmt(&mut self) {
        self.node(K::SetStmt, |p| {
            p.bump(); // `set`
            p.expr_bp(rill_syntax::bp::EQUALITY + 1);
            p.expect(K::Assign, "`=`");
            p.expr();
            p.semi_or_block_end();
        });
    }

    /// `yield inout? expr ;`
    fn yield_stmt(&mut self) {
        self.node(K::YieldStmt, |p| {
            p.bump(); // `yield`
            if p.at(K::KwInout) {
                p.bump();
            }
            p.expr();
            p.semi_or_block_end();
        });
    }

    /// `when (cond) ;` — a builder guard (§11 `all { ... }`).
    fn when_stmt(&mut self) {
        self.node(K::WhenStmt, |p| {
            p.bump(); // `when`
            p.expect(K::LParen, "`(`");
            p.cond_binding_or_expr();
            p.expect(K::RParen, "`)`");
            p.semi_or_block_end();
        });
    }

    /// `for (name := iter)` / `for (name : iter)` plus a body.
    fn for_stmt(&mut self) {
        self.node(K::ForStmt, |p| {
            p.bump(); // `for`
            p.expect(K::LParen, "`(`");
            p.node(K::BindStmt, |p| {
                if p.at(K::Name) || p.at(K::Underscore) {
                    p.bump();
                } else {
                    p.error("expected a loop variable");
                }
                if !p.eat(K::ColonEq) {
                    p.expect(K::Colon, "`:=` or `:`");
                }
                p.expr();
            });
            p.expect(K::RParen, "`)`");
            // Body: a block, or a single statement / tail expression.
            if p.at(K::LBrace) {
                p.block();
            } else {
                p.statement();
            }
        });
    }

    /// `while (cond) body` — an early-exit loop, which scanners need.
    fn while_stmt(&mut self) {
        self.node(K::WhileStmt, |p| {
            p.bump(); // `while`
            p.expect(K::LParen, "`(`");
            p.expr();
            p.expect(K::RParen, "`)`");
            if p.at(K::LBrace) {
                p.block();
            } else {
                p.statement();
            }
        });
    }

    /// `break ;` — leaves the innermost loop.
    fn break_stmt(&mut self) {
        self.node(K::BreakStmt, |p| {
            p.bump(); // `break`
            p.semi_or_block_end();
        });
    }

    /// Condition inside `(`...`)`: either `name := expr` or a plain expression.
    fn cond_binding_or_expr(&mut self) {
        if (self.at(K::Name) || self.at(K::Underscore)) && self.nth(1) == K::ColonEq {
            self.bind_stmt();
        } else {
            self.expr();
        }
    }

    /// Consumes the statement's `;`; before `}` or at EOF it is optional.
    pub(crate) fn semi_or_block_end(&mut self) {
        if !self.eat(K::Semi) && !self.at(K::RBrace) && !self.at_end() {
            self.error("expected `;`");
        }
    }
}
