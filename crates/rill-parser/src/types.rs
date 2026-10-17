//! Parsing of types, parameters, effect rows and trait constraints
//! (PLAN.md §2.2, §2.3, §2.4).

use crate::Parser;
use rill_syntax::SyntaxKind as K;

impl<'a> Parser<'a> {
    /// `type = type_atom_chain ("->" convention? type_atom_chain)* effects?`
    ///
    /// Wrapped in a [`K::TypeRef`] node.
    pub fn parse_type(&mut self) {
        self.node(K::TypeRef, |p| {
            p.type_atom_chain();
            let mut arrows = 0usize;
            while p.at(K::Arrow) {
                p.bump();
                arrows += 1;
                p.convention();
                p.type_atom_chain();
            }
            // Effect rows only bind to function types (a bare `Array i32` has none).
            if arrows > 0 && p.at(K::Lt) {
                p.effect_row();
            }
        });
    }

    /// `type_atom+` — application of a type constructor to type atoms.
    /// A single atom is emitted unwrapped; two or more form a [`K::TypeApp`].
    ///
    /// The wrapper marker is only completed once a second atom is seen, so
    /// `&raw u8` (which is one atom) is never mistaken for an application.
    fn type_atom_chain(&mut self) {
        if !starts_type_atom(self.kind()) {
            self.error("expected a type");
            return;
        }
        let marker = self.marker();
        self.type_atom();
        if !starts_type_atom(self.kind()) {
            return;
        }
        while starts_type_atom(self.kind()) {
            self.type_atom();
        }
        marker.complete(self, K::TypeApp);
    }

    pub(crate) fn type_atom(&mut self) {
        match self.kind() {
            K::Name => self.type_path(),
            K::KwDyn => {
                self.node(K::TypeDyn, |p| {
                    p.bump();
                    if starts_type_atom(p.kind()) {
                        p.parse_type();
                    } else {
                        p.error("expected a type after `dyn`");
                    }
                });
            }
            K::KwView => {
                self.node(K::TypeView, |p| {
                    p.bump();
                    if p.at(K::LBrace) {
                        p.view_fields_node();
                    } else if starts_type_atom(p.kind()) {
                        p.type_atom();
                    } else {
                        p.error("expected `{` or a type after `view`");
                    }
                });
            }
            K::LParen => self.type_tuple(),
            K::LBracket => self.type_array(),
            K::Amp => {
                self.node(K::TypePtr, |p| {
                    p.bump();
                    // `&raw T` / `&raw mut T` (grammar §2.3).
                    if p.at(K::Name) && p.current_text() == "raw" {
                        p.bump();
                        if p.at(K::Name) && p.current_text() == "mut" {
                            p.bump();
                        }
                    } else {
                        p.error("expected `raw` after `&`");
                    }
                    if starts_type_atom(p.kind()) {
                        p.type_atom_chain();
                    } else {
                        p.error("expected a pointee type");
                    }
                });
            }
            _ => {
                self.error("expected a type");
                // Consume one token so callers make progress.
                if !self.at_end() {
                    self.bump();
                }
            }
        }
    }

    fn type_path(&mut self) {
        self.node(K::TypePath, |p| {
            p.expect(K::Name, "a type name");
            while p.at(K::ColonColon) {
                p.bump();
                p.expect(K::Name, "a name after `::`");
            }
        });
    }

    /// `"(" type ")"`, `"(" type ("," type)+ ")"`, or unit `()`.
    ///
    /// A single parenthesised type is *transparent* (grammar §2.3 lists it as
    /// an alternative to the tuple form), so only tuples and the unit type get
    /// a [`K::TypeTuple`] node.
    fn type_tuple(&mut self) {
        let marker = self.marker();
        self.expect(K::LParen, "`(`");
        if self.at(K::RParen) {
            self.bump();
            marker.complete(self, K::TypeTuple);
            return;
        }
        self.parse_type();
        if self.eat(K::Comma) {
            while !self.at(K::RParen) && !self.at_end() {
                self.parse_type();
                if !self.eat(K::Comma) {
                    break;
                }
            }
            self.expect(K::RParen, "`)`");
            marker.complete(self, K::TypeTuple);
        } else {
            self.expect(K::RParen, "`)`");
        }
    }

    /// `"[" type (";" comptime_expr)? "]"` — fixed-size / shaped tensor type.
    fn type_array(&mut self) {
        self.node(K::TypeArray, |p| {
            p.expect(K::LBracket, "`[`");
            p.parse_type();
            if p.eat(K::Semi) {
                p.comptime_or_expr();
            }
            p.expect(K::RBracket, "`]`");
        });
    }

    /// `comptime { ... }` or a plain expression (shape/length positions).
    pub fn comptime_or_expr(&mut self) {
        if self.at(K::KwComptime) {
            self.node(K::ComptimeExpr, |p| {
                p.bump();
                p.block();
            });
        } else {
            self.expr();
        }
    }

    // -- parameters --------------------------------------------------------

    /// `"(" (parameter ("," parameter)*)? ")"`
    pub fn param_list(&mut self) {
        self.node(K::ParamList, |p| {
            p.expect(K::LParen, "`(`");
            if p.at(K::RParen) {
                p.bump();
                return;
            }
            p.param();
            while p.eat(K::Comma) {
                if p.at(K::RParen) {
                    break; // trailing comma
                }
                p.param();
            }
            p.expect(K::RParen, "`)`");
        });
    }

    /// `("comptime")? name (":" convention? type)?`
    fn param(&mut self) {
        self.node(K::Param, |p| {
            if p.at(K::KwComptime) {
                p.bump();
            }
            if p.at(K::Name) || p.at(K::Underscore) {
                p.bump();
            } else {
                p.error("expected a parameter name");
            }
            if p.eat(K::Colon) {
                p.convention();
                p.parse_type();
            }
        });
    }

    /// Consumes a passing convention token (`let`, `inout`, `sink`, `set`) if present.
    pub(crate) fn convention(&mut self) -> bool {
        if matches!(self.kind(), K::KwLet | K::KwInout | K::KwSink | K::KwSet) {
            self.bump();
            true
        } else {
            false
        }
    }

    // -- effect rows -------------------------------------------------------

    /// `"<" effect_item ("|" effect_item)* ">"` — `effect_item = name type_atom*`.
    pub fn effect_row(&mut self) {
        self.node(K::EffectRow, |p| {
            p.expect(K::Lt, "`<`");
            if p.at(K::Gt) {
                p.bump();
                return;
            }
            p.effect_item();
            while p.eat(K::Pipe) {
                p.effect_item();
            }
            p.expect(K::Gt, "`>`");
        });
    }

    fn effect_item(&mut self) {
        self.node(K::EffectItem, |p| {
            if p.at(K::Name) {
                p.bump();
            } else {
                p.error("expected an effect name");
                return;
            }
            // Optional type arguments, e.g. `State s`.
            while starts_type_atom(p.kind()) {
                p.type_atom_chain();
            }
        });
    }

    // -- constraints -------------------------------------------------------

    /// `"where" constraint ("|" constraint)*`
    pub fn constraints(&mut self) {
        self.node(K::ConstraintList, |p| {
            p.expect(K::KwWhere, "`where`");
            p.constraint();
            while p.eat(K::Pipe) {
                p.constraint();
            }
        });
    }

    /// `type "is" name type_atom*` or `name ":" effect_row`.
    fn constraint(&mut self) {
        self.node(K::Constraint, |p| {
            if p.at(K::Name) && p.nth(1) == K::Colon {
                p.bump();
                p.bump();
                p.effect_row();
            } else {
                p.parse_type();
                p.expect(K::KwIs, "`is`");
                if p.at(K::Name) {
                    p.type_atom_chain();
                } else {
                    p.error("expected a trait name after `is`");
                }
            }
        });
    }

    // -- nominal bodies ----------------------------------------------------

    /// `{ field* deinit_clause? }` — fields are `var? name : type ;`.
    pub fn record_fields(&mut self) {
        self.expect(K::LBrace, "`{`");
        while !self.at(K::RBrace) && !self.at_end() {
            let before = self.pos_marker();
            if self.at(K::KwDeinit) {
                self.deinit_clause();
            } else if self.at(K::Name) || self.at(K::KwVar) {
                self.field_def();
            } else {
                self.error("expected a field or `}`");
                self.bump();
            }
            if self.pos_marker() == before {
                self.bump();
            }
        }
        self.expect(K::RBrace, "`}`");
    }

    /// `{ field* }` — the body of a `view` type.
    pub fn view_fields_node(&mut self) {
        self.expect(K::LBrace, "`{`");
        while !self.at(K::RBrace) && !self.at_end() {
            let before = self.pos_marker();
            if self.at(K::Name) || self.at(K::KwVar) {
                self.field_def();
            } else {
                self.error("expected a field or `}`");
                self.bump();
            }
            if self.pos_marker() == before {
                self.bump();
            }
        }
        self.expect(K::RBrace, "`}`");
    }

    /// `var? name ":" type ";"` — the `;` is optional before `}`.
    fn field_def(&mut self) {
        self.node(K::FieldDef, |p| {
            if p.at(K::KwVar) {
                p.bump();
            }
            if p.at(K::Name) {
                p.bump();
            } else {
                p.error("expected a field name");
            }
            if p.expect(K::Colon, "`:`") {
                p.parse_type();
            }
            if !p.eat(K::Semi) && !p.at(K::RBrace) {
                p.error("expected `;` after the field");
            }
        });
    }

    /// `deinit (sink name) block`
    fn deinit_clause(&mut self) {
        self.node(K::DeinitClause, |p| {
            p.bump(); // `deinit`
            p.node(K::ParamList, |p| {
                p.expect(K::LParen, "`(`");
                if p.at(K::KwSink) {
                    p.bump();
                }
                if p.at(K::Name) {
                    p.bump();
                } else {
                    p.error("expected a name in `deinit`");
                }
                p.expect(K::RParen, "`)`");
            });
            p.block();
        });
    }

    /// Type parameters: `a`, `(comptime n : type)`.
    pub fn type_params(&mut self) {
        if !(self.at(K::Name) || self.at(K::LParen)) {
            return;
        }
        self.node(K::TypeParams, |p| {
            while p.at(K::Name) || p.at(K::LParen) {
                if p.at(K::Name) {
                    p.bump();
                } else {
                    p.node(K::Param, |p| {
                        p.expect(K::LParen, "`(`");
                        p.expect(K::KwComptime, "`comptime`");
                        if p.at(K::Name) {
                            p.bump();
                        } else {
                            p.error("expected a type parameter name");
                        }
                        if p.expect(K::Colon, "`:`") {
                            p.parse_type();
                        }
                        p.expect(K::RParen, "`)`");
                    });
                }
            }
        });
    }
}

fn starts_type_atom(kind: K) -> bool {
    matches!(
        kind,
        K::Name | K::KwDyn | K::KwView | K::LParen | K::LBracket | K::Amp
    )
}
