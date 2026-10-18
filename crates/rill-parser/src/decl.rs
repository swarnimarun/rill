//! Declarations: `use`, `type`, `trait`, `impl`, `builder`, `extern`, `let`,
//! `project` and bare signatures (PLAN.md §2.2, §2.4).

use crate::Parser;
use rill_syntax::SyntaxKind as K;

impl<'a> Parser<'a> {
    /// `@name ("(" "text" ")")? ;` — a module-level directive.
    pub fn attribute_decl(&mut self) {
        self.node(K::AttributeDecl, |p| {
            p.bump(); // `@`
            p.expect(K::Name, "an attribute name");
            if p.at(K::LParen) {
                p.bump();
                if p.at(K::Text) {
                    p.bump();
                } else {
                    p.error("expected a text literal");
                }
                p.expect(K::RParen, "`)`");
            }
            p.semi_or_block_end();
        });
    }

    /// `use path (:: name)* | "text" ;`
    pub fn use_decl(&mut self) {
        self.node(K::UseDecl, |p| {
            p.bump(); // `use`
            if p.at(K::Text) {
                p.bump();
            } else {
                p.node(K::UsePath, |p| {
                    p.expect(K::Name, "a module name");
                    while p.at(K::ColonColon) {
                        if p.nth(1) == K::LBrace {
                            p.bump(); // `::`
                            p.use_group();
                            break;
                        }
                        if p.nth(1) == K::Star {
                            p.bump(); // `::`
                            p.bump(); // `*`
                            break;
                        }
                        p.bump();
                        if p.at(K::Name) {
                            p.bump();
                        } else {
                            p.error("expected a module name after `::`");
                            break;
                        }
                    }
                });
            }
            p.semi_or_block_end();
        });
    }

    /// `{ name ("," name)* }` in a `use` path.
    fn use_group(&mut self) {
        self.node(K::UseGroup, |p| {
            p.expect(K::LBrace, "`{`");
            if !p.at(K::RBrace) {
                p.expect(K::Name, "an imported name");
                while p.eat(K::Comma) {
                    if p.at(K::RBrace) {
                        break;
                    }
                    p.expect(K::Name, "an imported name");
                }
            }
            p.expect(K::RBrace, "`}`");
        });
    }

    /// `type Name type_params = record {...} | view {...} | | V | V t | Type ;`
    pub fn type_decl(&mut self) {
        self.node(K::TypeDecl, |p| {
            p.bump(); // `type`
            p.expect(K::Name, "a type name");
            p.type_params();
            p.expect(K::Assign, "`=`");
            match p.kind() {
                K::KwRecord => {
                    p.node(K::RecordDef, |p| {
                        p.bump(); // `record`
                        p.record_fields();
                    });
                }
                K::KwView => {
                    p.node(K::TypeView, |p| {
                        p.bump(); // `view`
                        p.view_fields_node();
                    });
                }
                K::At => p.include_body(),
                _ if p.variants_ahead() => p.variant_list(),
                _ => p.parse_type(),
            }
            p.semi_or_block_end();
        });
    }

    /// `@include("text")` — an external source file in a type body.
    fn include_body(&mut self) {
        self.node(K::Include, |p| {
            p.bump(); // `@`
            p.expect(K::Name, "`include`");
            p.expect(K::LParen, "`(`");
            if p.at(K::Text) || p.at(K::RawText) {
                p.bump();
            } else {
                p.error("expected a text literal");
            }
            p.expect(K::RParen, "`)`");
        });
    }

    /// Lookahead for algebraic variants: a leading `|` or a `|` before the
    /// statement's `;`. Aliases (`type X = Array i32`) have neither.
    fn variants_ahead(&self) -> bool {
        if self.at(K::Pipe) {
            return true;
        }
        let mut i = 0usize;
        loop {
            match self.nth(i) {
                K::Pipe => return true,
                K::Semi | K::Eof | K::RBrace | K::ColonEq => return false,
                _ => i += 1,
            }
            if i > 256 {
                return false;
            }
        }
    }

    /// `|? Variant ("|" Variant)*` — `Variant = Name type_atom*`.
    fn variant_list(&mut self) {
        self.node(K::VariantList, |p| {
            p.eat(K::Pipe);
            loop {
                let before = p.pos_marker();
                p.node(K::Variant, |p| {
                    p.expect(K::Name, "a variant name");
                    while matches!(
                        p.kind(),
                        K::Name | K::LParen | K::LBracket | K::KwDyn | K::KwView
                    ) {
                        p.type_atom();
                    }
                });
                if p.pos_marker() == before {
                    break;
                }
                if !p.eat(K::Pipe) {
                    break;
                }
            }
        });
    }

    /// `trait Name type_params where? =? { members } ;?`
    pub fn trait_decl(&mut self) {
        self.node(K::TraitDecl, |p| {
            p.bump(); // `trait`
            p.expect(K::Name, "a trait name");
            p.type_params();
            if p.at(K::KwWhere) {
                p.constraints();
            }
            p.eat(K::Assign);
            if p.at(K::LBrace) {
                p.member_list(K::TraitBody);
            }
            p.semi_optional();
        });
    }

    /// `impl Name type_params where? =? { members } ;?`
    ///
    /// The plan leaves `impl` heads unspecified; the head is preserved as a
    /// name plus optional type parameters, and members are declared like trait
    /// members (`let` or bare signatures).
    pub fn impl_decl(&mut self) {
        self.node(K::ImplDecl, |p| {
            p.bump(); // `impl`
            if p.at(K::Name) {
                p.bump();
            } else {
                p.error("expected a trait or type name after `impl`");
            }
            p.type_params();
            if p.at(K::KwWhere) {
                p.constraints();
            }
            p.eat(K::Assign);
            if p.at(K::LBrace) {
                p.member_list(K::ImplBody);
            }
            p.semi_optional();
        });
    }

    /// `builder Name type_params = { members } ;`
    pub fn builder_decl(&mut self) {
        self.node(K::BuilderDecl, |p| {
            p.bump(); // `builder`
            p.expect(K::Name, "a builder name");
            p.type_params();
            p.expect(K::Assign, "`=`");
            if p.at(K::LBrace) {
                p.member_list(K::BuilderBody);
            } else {
                p.error("expected `{` with builder members");
            }
            p.semi_or_block_end();
        });
    }

    /// `extern "text"? (let_decl | signature)` — FFI declarations.
    pub fn extern_decl(&mut self) {
        self.node(K::ExternDecl, |p| {
            p.bump(); // `extern`
            p.eat(K::Text);
            if p.at(K::KwLet) {
                p.let_decl();
            } else if p.at(K::Name) {
                p.signature_decl();
            } else {
                p.error("expected `let` or a signature after `extern`");
            }
        });
    }

    /// `{ (let_decl | signature)* }`
    fn member_list(&mut self, body_kind: K) {
        self.node(body_kind, |p| {
            p.expect(K::LBrace, "`{`");
            while !p.at(K::RBrace) && !p.at_end() {
                let before = p.pos_marker();
                match p.kind() {
                    K::KwLet => p.let_decl(),
                    K::Name if p.nth(1) == K::Colon => p.signature_decl(),
                    _ => {
                        p.error("expected a member declaration");
                        p.bump();
                    }
                }
                if p.pos_marker() == before {
                    p.bump();
                }
            }
            p.expect(K::RBrace, "`}`");
        });
    }

    /// `let name params* ("->" convention? type)? where? effects? ("=" expr)? ;`
    pub fn let_decl(&mut self) {
        self.node(K::LetDecl, |p| {
            p.bump(); // `let`
            p.expect(K::Name, "a function name");
            p.decl_signature_tail();
            if p.eat(K::Assign) {
                p.expr();
            }
            p.semi_or_block_end();
        });
    }

    /// `project name params+ ("->" convention? type)? where? effects? = expr ;`
    pub fn project_decl(&mut self) {
        self.node(K::ProjectDecl, |p| {
            p.bump(); // `project`
            p.expect(K::Name, "a projection name");
            if !p.at(K::LParen) {
                p.error("a projection needs a parameter list");
            }
            p.decl_signature_tail();
            p.expect(K::Assign, "`=`");
            p.expr();
            p.semi_or_block_end();
        });
    }

    /// `name ":" type where? effects? ;` — a bare signature (trait members,
    /// `extern` bindings).
    pub fn signature_decl(&mut self) {
        self.node(K::SignatureDecl, |p| {
            p.expect(K::Name, "a signature name");
            if p.expect(K::Colon, "`:`") {
                p.parse_type();
            }
            p.decl_effects_tail();
            p.semi_or_block_end();
        });
    }

    /// Parameter groups, optional result type, constraints and effect row.
    fn decl_signature_tail(&mut self) {
        while self.at(K::LParen) {
            self.param_list();
        }
        if self.eat(K::Arrow) {
            self.convention();
            self.parse_type();
        }
        self.decl_effects_tail();
    }

    fn decl_effects_tail(&mut self) {
        if self.at(K::KwWhere) {
            self.constraints();
        }
        if self.at(K::Lt) {
            self.effect_row();
        }
    }

    /// Trailing `;` of a block-like declaration (optional).
    fn semi_optional(&mut self) {
        if self.at(K::Semi) {
            self.bump();
        }
    }
}
