//! CST -> AST lowering (PLAN.md §7.2, first stage).
//!
//! Lowering is total: a malformed or unexpected CST shape produces an
//! `Error`/`Item::Error` node plus a diagnostic instead of panicking, so the
//! pipeline stays alive on incomplete code.

use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_span::{Interner, SourceMap, Span, Symbol};
use rill_syntax::{Cst, Element, SyntaxKind as K, SyntaxNode, SyntaxToken};

use crate::ast::*;
use crate::desugar;

/// Output of lowering: the module, diagnostics, and the interner that owns
/// every symbol referenced by the AST.
pub struct Lowered {
    pub module: Module,
    pub diagnostics: Diagnostics,
    pub interner: Interner,
}

impl Lowered {
    /// Resolves an identifier to its text.
    pub fn text(&self, ident: Ident) -> &str {
        self.interner.get(ident.sym)
    }
}

/// Lowers a CST to a desugared AST.
pub fn lower_module(source_map: &SourceMap, cst: &Cst) -> Lowered {
    let mut lower = Lower::new(source_map);
    let mut module = lower.module(&cst.root);
    let mut diagnostics = std::mem::take(&mut lower.diags);
    let mut interner = lower.interner;
    let extra = desugar::run(&mut module, &mut interner);
    diagnostics.extend(extra);
    Lowered {
        module,
        diagnostics,
        interner,
    }
}

struct Lower<'a> {
    sm: &'a SourceMap,
    interner: Interner,
    diags: Diagnostics,
}

impl<'a> Lower<'a> {
    fn new(sm: &'a SourceMap) -> Self {
        Lower {
            sm,
            interner: Interner::new(),
            diags: Diagnostics::new(),
        }
    }

    fn err(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(span, message));
    }

    /// Reports a malformed construct and yields an error node. Every path that
    /// cannot represent the CST goes through one of these, so a lowering gap
    /// surfaces as a diagnostic instead of silently degrading the AST.
    fn bad_type(&mut self, span: Span, what: &str) -> Type {
        self.err(span, format!("malformed type: {what}"));
        Type::Error(span)
    }

    fn bad_expr(&mut self, span: Span, what: &str) -> Expr {
        self.err(span, format!("malformed expression: {what}"));
        Expr::error(span)
    }

    fn bad_pattern(&mut self, span: Span, what: &str) -> Pattern {
        self.err(span, format!("malformed pattern: {what}"));
        Pattern::Error(span)
    }

    fn text(&self, tok: SyntaxToken) -> &str {
        self.sm.snippet(tok.span)
    }

    fn ident(&mut self, tok: SyntaxToken) -> Ident {
        let text = self.text(tok).to_string();
        Ident::new(self.interner.intern(&text), tok.span)
    }

    // -- module & items ----------------------------------------------------

    fn module(&mut self, root: &SyntaxNode) -> Module {
        Module {
            items: root
                .child_list()
                .into_iter()
                .map(|n| self.item(n))
                .collect(),
        }
    }

    fn item(&mut self, node: &SyntaxNode) -> Item {
        match node.kind {
            K::UseDecl => Item::Use(self.use_decl(node)),
            K::TypeDecl => Item::Type(self.type_decl(node)),
            K::TraitDecl => Item::Trait(self.trait_decl(node)),
            K::ImplDecl => Item::Impl(self.impl_decl(node)),
            K::BuilderDecl => Item::Builder(self.builder_decl(node)),
            K::ExternDecl => Item::Extern(self.extern_decl(node)),
            K::LetDecl => Item::Fn(self.fn_decl(node)),
            K::ProjectDecl => Item::Project(self.project_decl(node)),
            K::SignatureDecl => Item::Signature(self.signature_decl(node)),
            k if is_stmt_kind(k) => Item::Stmt(self.stmt(node)),
            k if is_expr_kind(k) => Item::Stmt(Stmt::Expr(self.expr(node))),
            _ => {
                self.err(
                    node.span,
                    format!("unexpected {:?} at item position", node.kind),
                );
                Item::Error(node.span)
            }
        }
    }

    fn use_decl(&mut self, node: &SyntaxNode) -> UseDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwUse);
        let path = if let Some(text_tok) = cur.eat_token(K::Text) {
            UsePath::Text {
                text: self.text(text_tok).to_string(),
            }
        } else {
            match cur.expect_node(K::UsePath) {
                Some(path_node) => {
                    let mut pc = Cursor::new(path_node);
                    let mut segments = Vec::new();
                    while let Some(tok) = pc.name() {
                        segments.push(self.ident(tok));
                    }
                    if let Some(group) = cur.expect_node(K::UseGroup) {
                        let mut gc = Cursor::new(group);
                        let mut names = Vec::new();
                        while let Some(tok) = gc.name() {
                            names.push(self.ident(tok));
                            gc.eat(K::Comma);
                        }
                        UsePath::Group {
                            prefix: segments,
                            names,
                        }
                    } else {
                        UsePath::Plain(segments)
                    }
                }
                None => {
                    self.err(node.span, "expected a `use` path");
                    UsePath::Plain(Vec::new())
                }
            }
        };
        UseDecl {
            path,
            span: node.span,
        }
    }

    fn signature_decl(&mut self, node: &SyntaxNode) -> SignatureDecl {
        let mut cur = Cursor::new(node);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => return self.bad_signature(node.span),
        };
        let ty = if cur.eat(K::Colon).is_some() {
            self.type_child(&mut cur, node.span)
        } else {
            self.err(node.span, "expected `:` in a signature");
            Type::Error(node.span)
        };
        let constraints = self.constraints_opt(&mut cur);
        let effects = self.effects_opt(&mut cur);
        SignatureDecl {
            name,
            ty,
            constraints,
            effects,
            span: node.span,
        }
    }

    fn bad_signature(&mut self, span: Span) -> SignatureDecl {
        self.err(span, "malformed signature");
        SignatureDecl {
            name: Ident::new(Symbol::EMPTY, span),
            ty: Type::Error(span),
            constraints: Vec::new(),
            effects: EffectRow::empty(),
            span,
        }
    }

    fn fn_decl(&mut self, node: &SyntaxNode) -> FnDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwLet);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => {
                self.err(node.span, "expected a name after `let`");
                Ident::new(Symbol::EMPTY, node.span)
            }
        };
        let params = self.param_groups(&mut cur);
        let ret = self.result_type(&mut cur);
        let constraints = self.constraints_opt(&mut cur);
        let effects = self.effects_opt(&mut cur);
        let body = if cur.eat(K::Assign).is_some() {
            Some(self.expr_child(&mut cur, node.span))
        } else {
            None
        };
        FnDecl {
            name,
            params,
            ret,
            constraints,
            effects,
            body,
            span: node.span,
        }
    }

    fn project_decl(&mut self, node: &SyntaxNode) -> ProjectDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwProject);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => {
                self.err(node.span, "expected a projection name");
                Ident::new(Symbol::EMPTY, node.span)
            }
        };
        let params = self.param_groups(&mut cur);
        let ret = self.result_type(&mut cur);
        let constraints = self.constraints_opt(&mut cur);
        let effects = self.effects_opt(&mut cur);
        let body = if cur.eat(K::Assign).is_some() {
            self.expr_child(&mut cur, node.span)
        } else {
            self.err(node.span, "a projection needs `= body`");
            Expr::error(node.span)
        };
        ProjectDecl {
            name,
            params,
            ret,
            constraints,
            effects,
            body,
            span: node.span,
        }
    }

    fn type_decl(&mut self, node: &SyntaxNode) -> TypeDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwType);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => {
                self.err(node.span, "expected a type name");
                Ident::new(Symbol::EMPTY, node.span)
            }
        };
        let params = self.type_params(&mut cur);
        cur.eat(K::Assign);
        let body = match cur.peek_kind() {
            Some(K::RecordDef) => {
                let record_node = cur.node().expect("peeked record");
                TypeBody::Record(self.record_def(record_node, node.span))
            }
            Some(K::TypeView) => {
                let view_node = cur.node().expect("peeked node");
                let record = self.record_def(view_node, node.span);
                TypeBody::View(record)
            }
            Some(K::VariantList) => {
                let list = cur.node().expect("peeked node");
                TypeBody::Variants(self.variants(list))
            }
            Some(k) if is_type_kind(k) => TypeBody::Alias(self.type_child(&mut cur, node.span)),
            _ => {
                self.err(node.span, "expected a type body");
                TypeBody::Error(node.span)
            }
        };
        TypeDecl {
            name,
            params,
            body,
            span: node.span,
        }
    }

    fn trait_decl(&mut self, node: &SyntaxNode) -> TraitDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwTrait);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => Ident::new(Symbol::EMPTY, node.span),
        };
        let params = self.type_params(&mut cur);
        let constraints = self.constraints_opt(&mut cur);
        let members = self.members(&mut cur);
        TraitDecl {
            name,
            params,
            constraints,
            members,
            span: node.span,
        }
    }

    fn impl_decl(&mut self, node: &SyntaxNode) -> ImplDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwImpl);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => Ident::new(Symbol::EMPTY, node.span),
        };
        let params = self.type_params(&mut cur);
        let constraints = self.constraints_opt(&mut cur);
        let members = self.members(&mut cur);
        ImplDecl {
            name,
            params,
            constraints,
            members,
            span: node.span,
        }
    }

    fn builder_decl(&mut self, node: &SyntaxNode) -> BuilderDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwBuilder);
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => {
                self.err(node.span, "expected a builder name");
                Ident::new(Symbol::EMPTY, node.span)
            }
        };
        let params = self.type_params(&mut cur);
        let members = self.members(&mut cur);
        BuilderDecl {
            name,
            params,
            members,
            span: node.span,
        }
    }

    fn extern_decl(&mut self, node: &SyntaxNode) -> ExternDecl {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwExtern);
        let abi = cur.eat_token(K::Text).map(|t| self.text(t).to_string());
        let inner = match cur.node() {
            Some(inner_node) => Box::new(self.item(inner_node)),
            None => {
                self.err(node.span, "expected a declaration after `extern`");
                Box::new(Item::Error(node.span))
            }
        };
        ExternDecl {
            abi,
            inner,
            span: node.span,
        }
    }

    fn members(&mut self, cur: &mut Cursor<'_>) -> Vec<Item> {
        match cur.node() {
            Some(body) => body
                .child_list()
                .into_iter()
                .map(|n| self.item(n))
                .collect(),
            None => Vec::new(),
        }
    }

    fn param_groups(&mut self, cur: &mut Cursor<'_>) -> Vec<ParamGroup> {
        let mut groups = Vec::new();
        while let Some(node) = cur.expect_node(K::ParamList) {
            groups.push(self.param_group(node));
        }
        groups
    }

    fn param_group(&mut self, node: &SyntaxNode) -> ParamGroup {
        let mut cur = Cursor::new(node);
        cur.eat(K::LParen);
        let mut params = Vec::new();
        while cur.peek_kind() == Some(K::Param) {
            let param = cur.node().expect("peeked param");
            params.push(self.param(param));
            cur.eat(K::Comma);
        }
        ParamGroup {
            params,
            span: node.span,
        }
    }

    fn param(&mut self, node: &SyntaxNode) -> Param {
        let mut cur = Cursor::new(node);
        let comptime = cur.eat(K::KwComptime).is_some();
        let name = cur.name().map(|tok| self.ident(tok));
        let mut convention = None;
        let mut ty = None;
        if cur.eat(K::Colon).is_some() {
            convention = self.convention(&mut cur);
            ty = Some(self.type_child(&mut cur, node.span));
        }
        Param {
            comptime,
            name,
            convention,
            ty,
            span: node.span,
        }
    }

    /// `-> convention? type` of a declaration.
    fn result_type(&mut self, cur: &mut Cursor<'_>) -> Option<ParamSlot> {
        cur.eat(K::Arrow)?;
        let start = cur.peek().map(|e| e.span()).unwrap_or(Span::DUMMY);
        let convention = self.convention(cur);
        let ty = self.type_child(cur, start);
        let span = start.merge(ty.span());
        Some(ParamSlot {
            convention,
            ty,
            span,
        })
    }

    fn convention(&mut self, cur: &mut Cursor<'_>) -> Option<Convention> {
        match cur.peek_kind() {
            Some(K::KwLet) => {
                cur.bump();
                Some(Convention::Let)
            }
            Some(K::KwInout) => {
                cur.bump();
                Some(Convention::Inout)
            }
            Some(K::KwSink) => {
                cur.bump();
                Some(Convention::Sink)
            }
            Some(K::KwSet) => {
                cur.bump();
                Some(Convention::Set)
            }
            _ => None,
        }
    }

    fn type_params(&mut self, cur: &mut Cursor<'_>) -> Vec<TypeParam> {
        let Some(node) = cur.expect_node(K::TypeParams) else {
            return Vec::new();
        };
        let mut tc = Cursor::new(node);
        let mut params = Vec::new();
        loop {
            match tc.peek_kind() {
                Some(K::Name) => {
                    let tok = tc.name().expect("peeked name");
                    let name = self.ident(tok);
                    params.push(TypeParam {
                        name,
                        comptime_ty: None,
                        span: tok.span,
                    });
                }
                Some(K::Param) => {
                    let param_node = tc.node().expect("peeked param");
                    let mut pc = Cursor::new(param_node);
                    pc.eat(K::LParen);
                    pc.eat(K::KwComptime);
                    let name = match pc.name() {
                        Some(tok) => self.ident(tok),
                        None => Ident::new(Symbol::EMPTY, param_node.span),
                    };
                    let comptime_ty = if pc.eat(K::Colon).is_some() {
                        Some(self.type_child(&mut pc, param_node.span))
                    } else {
                        None
                    };
                    params.push(TypeParam {
                        name,
                        comptime_ty,
                        span: param_node.span,
                    });
                }
                _ => break,
            }
        }
        params
    }

    fn constraints_opt(&mut self, cur: &mut Cursor<'_>) -> Vec<Constraint> {
        let Some(node) = cur.expect_node(K::ConstraintList) else {
            return Vec::new();
        };
        node.child_nodes(K::Constraint)
            .map(|n| self.constraint(n))
            .collect()
    }

    fn constraint(&mut self, node: &SyntaxNode) -> Constraint {
        let mut cur = Cursor::new(node);
        if cur.peek_kind() == Some(K::Name) && cur.peek_nth_kind(1) == Some(K::Colon) {
            let name = cur
                .name()
                .map(|t| self.ident(t))
                .unwrap_or(Ident::new(Symbol::EMPTY, node.span));
            cur.eat(K::Colon);
            let effects = self.effects_opt(&mut cur);
            Constraint::Effect {
                name,
                effects,
                span: node.span,
            }
        } else {
            let ty = self.type_child(&mut cur, node.span);
            cur.eat(K::KwIs);
            let mut trait_path = Vec::new();
            let mut args = Vec::new();
            while let Some(tok) = cur.name() {
                trait_path.push(self.ident(tok));
                if cur.peek_kind() == Some(K::ColonColon) {
                    cur.bump();
                }
            }
            while starts_type_element(&cur) {
                args.push(self.type_child(&mut cur, node.span));
            }
            Constraint::Is {
                ty,
                trait_path,
                args,
                span: node.span,
            }
        }
    }

    fn effects_opt(&mut self, cur: &mut Cursor<'_>) -> EffectRow {
        let Some(node) = cur.expect_node(K::EffectRow) else {
            return EffectRow::empty();
        };
        let mut items = Vec::new();
        for item_node in node.child_nodes(K::EffectItem) {
            let mut ic = Cursor::new(item_node);
            let name = match ic.name() {
                Some(tok) => self.ident(tok),
                None => Ident::new(Symbol::EMPTY, item_node.span),
            };
            let mut args = Vec::new();
            while starts_type_element(&ic) {
                args.push(self.type_child(&mut ic, item_node.span));
            }
            items.push(EffectItem {
                name,
                args,
                span: item_node.span,
            });
        }
        EffectRow {
            items,
            span: node.span,
        }
    }

    // -- nominal bodies ----------------------------------------------------

    fn record_def(&mut self, node: &SyntaxNode, fallback: Span) -> RecordDef {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwRecord);
        cur.eat(K::KwView);
        cur.eat(K::LBrace);
        let mut fields = Vec::new();
        let mut deinit = None;
        loop {
            match cur.peek_kind() {
                Some(K::FieldDef) => {
                    let field = cur.node().expect("peeked field");
                    fields.push(self.field_def(field));
                    cur.eat(K::Semi);
                }
                Some(K::DeinitClause) => {
                    let clause = cur.node().expect("peeked deinit");
                    deinit = Some(self.deinit_clause(clause));
                }
                _ => break,
            }
        }
        RecordDef {
            fields,
            deinit,
            span: node.span.merge(fallback),
        }
    }

    fn field_def(&mut self, node: &SyntaxNode) -> FieldDef {
        let mut cur = Cursor::new(node);
        let mutable = cur.eat(K::KwVar).is_some();
        let name = match cur.name() {
            Some(tok) => self.ident(tok),
            None => Ident::new(Symbol::EMPTY, node.span),
        };
        cur.eat(K::Colon);
        let ty = self.type_child(&mut cur, node.span);
        FieldDef {
            mutable,
            name,
            ty,
            span: node.span,
        }
    }

    fn deinit_clause(&mut self, node: &SyntaxNode) -> DeinitClause {
        let mut cur = Cursor::new(node);
        cur.eat(K::KwDeinit);
        let mut binding = None;
        if let Some(params) = cur.expect_node(K::ParamList) {
            let mut pc = Cursor::new(params);
            pc.eat(K::LParen);
            pc.eat(K::KwSink);
            binding = pc.name().map(|tok| self.ident(tok));
        }
        let body = match cur.expect_node(K::Block) {
            Some(block) => self.block(block),
            None => Block::empty(node.span),
        };
        DeinitClause {
            binding,
            body,
            span: node.span,
        }
    }

    fn variants(&mut self, node: &SyntaxNode) -> Vec<Variant> {
        let mut out = Vec::new();
        for variant in node.child_nodes(K::Variant) {
            let mut cur = Cursor::new(variant);
            let name = match cur.name() {
                Some(tok) => self.ident(tok),
                None => Ident::new(Symbol::EMPTY, variant.span),
            };
            let mut payload = Vec::new();
            while starts_type_element(&cur) {
                payload.push(self.type_child(&mut cur, variant.span));
            }
            out.push(Variant {
                name,
                payload,
                span: variant.span,
            });
        }
        out
    }

    // -- types -------------------------------------------------------------

    fn type_child(&mut self, cur: &mut Cursor<'_>, fallback: Span) -> Type {
        match cur.node() {
            Some(node) => self.type_of(node),
            None => {
                self.err(fallback, "expected a type");
                Type::Error(fallback)
            }
        }
    }

    fn type_of(&mut self, node: &SyntaxNode) -> Type {
        match node.kind {
            K::TypePath => {
                let mut cur = Cursor::new(node);
                let mut segments = Vec::new();
                while let Some(tok) = cur.name() {
                    segments.push(self.ident(tok));
                    cur.eat(K::ColonColon);
                }
                Type::Path {
                    segments,
                    span: node.span,
                }
            }
            K::TypeApp => {
                let mut cur = Cursor::new(node);
                let Some(first) = cur.node() else {
                    return self.bad_type(node.span, "type application without a constructor");
                };
                let ctor = self.type_of(first);
                let mut args = Vec::new();
                while starts_type_element(&cur) {
                    args.push(self.type_child(&mut cur, node.span));
                }
                Type::App {
                    ctor: Box::new(ctor),
                    args,
                    span: node.span,
                }
            }
            K::TypeTuple => {
                let mut cur = Cursor::new(node);
                cur.eat(K::LParen);
                let mut elems = Vec::new();
                loop {
                    match cur.peek_kind() {
                        None | Some(K::RParen) => break,
                        Some(K::Comma) => {
                            cur.bump();
                        }
                        Some(_) => match cur.node() {
                            Some(atom) => elems.push(self.type_of(atom)),
                            None => {
                                let span = cur.span_or(node.span);
                                self.err(span, "unexpected token in a tuple type");
                                cur.bump();
                            }
                        },
                    }
                }
                if elems.len() == 1 {
                    // `(T)` is a parenthesised group, not a one-element tuple.
                    elems.pop().expect("checked length")
                } else {
                    Type::Tuple {
                        elems,
                        span: node.span,
                    }
                }
            }
            K::TypeArray => {
                let mut cur = Cursor::new(node);
                cur.eat(K::LBracket);
                let elem = self.type_child(&mut cur, node.span);
                let len = if cur.eat(K::Semi).is_some() {
                    Some(Box::new(self.expr_child(&mut cur, node.span)))
                } else {
                    None
                };
                Type::Array {
                    elem: Box::new(elem),
                    len,
                    span: node.span,
                }
            }
            K::TypePtr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::Amp);
                cur.eat(K::Name); // `raw`
                let mutable = cur.peek_kind() == Some(K::Name) && self.text_of(&cur) == "mut";
                if mutable {
                    cur.bump();
                }
                let pointee = self.type_child(&mut cur, node.span);
                Type::Ptr {
                    mutable,
                    pointee: Box::new(pointee),
                    span: node.span,
                }
            }
            K::TypeDyn => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwDyn);
                let inner = self.type_child(&mut cur, node.span);
                Type::Dyn {
                    inner: Box::new(inner),
                    span: node.span,
                }
            }
            K::TypeRef => {
                let mut cur = Cursor::new(node);
                let has_arrow = cur.items.iter().any(|e| e.kind() == K::Arrow);
                if !has_arrow {
                    return self.type_child(&mut cur, node.span);
                }
                let mut params = Vec::new();
                let ret;
                let mut effects = EffectRow::empty();
                loop {
                    let start = cur.peek().map(|e| e.span()).unwrap_or(node.span);
                    let convention = self.convention(&mut cur);
                    let ty = self.type_child(&mut cur, node.span);
                    if cur.eat(K::Arrow).is_some() {
                        params.push(ParamSlot {
                            convention,
                            ty,
                            span: start,
                        });
                    } else {
                        if cur.peek_kind() == Some(K::EffectRow) {
                            effects = self.effects_opt(&mut cur);
                        }
                        ret = Some(Box::new(ty));
                        break;
                    }
                }
                Type::Fn {
                    params,
                    ret,
                    effects,
                    span: node.span,
                }
            }
            _ => {
                self.err(
                    node.span,
                    format!("unsupported type syntax {:?}", node.kind),
                );
                Type::Error(node.span)
            }
        }
    }

    fn text_of(&self, cur: &Cursor<'a>) -> String {
        cur.peek()
            .map(|e| self.sm.snippet(e.span()).to_string())
            .unwrap_or_default()
    }

    // -- statements --------------------------------------------------------

    fn stmt(&mut self, node: &SyntaxNode) -> Stmt {
        match node.kind {
            K::VarStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwVar);
                let name = match cur.name() {
                    Some(tok) => self.ident(tok),
                    None => Ident::new(Symbol::EMPTY, node.span),
                };
                let ty = if cur.eat(K::Colon).is_some() {
                    Some(self.type_child(&mut cur, node.span))
                } else {
                    None
                };
                cur.eat(K::Assign);
                let init = self.expr_child(&mut cur, node.span);
                Stmt::Var {
                    name,
                    ty,
                    init,
                    span: node.span,
                }
            }
            K::SetStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwSet);
                let place = self.expr_child(&mut cur, node.span);
                cur.eat(K::Assign);
                let value = self.expr_child(&mut cur, node.span);
                Stmt::Set {
                    place,
                    value,
                    span: node.span,
                }
            }
            K::YieldStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwYield);
                let convention = self.convention(&mut cur);
                let value = self.expr_child(&mut cur, node.span);
                Stmt::Yield {
                    convention,
                    value,
                    span: node.span,
                }
            }
            K::BindStmt => {
                let (pattern, op, value) = self.bind_parts(node);
                Stmt::Bind {
                    name: pattern,
                    op,
                    value,
                    span: node.span,
                }
            }
            K::TryBindStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwLet);
                cur.eat(K::Question);
                let name = match cur.name() {
                    Some(tok) => self.ident(tok),
                    None => Ident::new(Symbol::EMPTY, node.span),
                };
                cur.eat(K::Assign);
                let value = self.expr_child(&mut cur, node.span);
                Stmt::TryBind {
                    name,
                    value,
                    span: node.span,
                }
            }
            K::WhenStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwWhen);
                cur.eat(K::LParen);
                let cond = self.cond(&mut cur, node.span);
                cur.eat(K::RParen);
                Stmt::When {
                    cond,
                    span: node.span,
                }
            }
            K::ForStmt => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwFor);
                cur.eat(K::LParen);
                let (pattern, _op, iter) = match cur.expect_node(K::BindStmt) {
                    Some(bind) => self.bind_parts(bind),
                    None => (
                        Pattern::Error(node.span),
                        BindOp::ColonEq,
                        Expr::error(node.span),
                    ),
                };
                cur.eat(K::RParen);
                let body = match cur.peek_kind() {
                    Some(K::Block) => {
                        let block_node = cur.node().expect("peeked block");
                        self.block(block_node)
                    }
                    Some(k) if is_stmt_kind(k) || is_expr_kind(k) => {
                        let single = self.stmt_or_expr_inline(cur.bump().expect("peeked"));
                        Block {
                            stmts: vec![single],
                            tail: None,
                            span: node.span,
                        }
                    }
                    _ => {
                        self.err(node.span, "expected a loop body");
                        Block::empty(node.span)
                    }
                };
                Stmt::For {
                    pattern,
                    iter,
                    body,
                    span: node.span,
                }
            }
            K::LetDecl => Stmt::Let(self.fn_decl(node)),
            K::ExprStmt => {
                let mut cur = Cursor::new(node);
                match cur.node() {
                    Some(e) => Stmt::Expr(self.expr(e)),
                    None => Stmt::Error(node.span),
                }
            }
            k if is_expr_kind(k) => Stmt::Expr(self.expr(node)),
            _ => {
                self.err(node.span, format!("unexpected statement {:?}", node.kind));
                Stmt::Error(node.span)
            }
        }
    }

    /// A statement that is not wrapped in its own node (loop bodies).
    fn stmt_or_expr_inline(&mut self, element: &Element) -> Stmt {
        match element {
            Element::Node(node) => self.stmt(node),
            Element::Token(tok) => {
                self.err(tok.span, "expected a statement");
                Stmt::Error(tok.span)
            }
        }
    }

    fn bind_parts(&mut self, node: &SyntaxNode) -> (Pattern, BindOp, Expr) {
        let mut cur = Cursor::new(node);
        let pattern = match cur.peek_kind() {
            Some(K::Underscore) => {
                let tok = cur.bump().expect("peeked underscore");
                Pattern::Wildcard(tok.span())
            }
            Some(K::Name) => {
                let tok = cur.name().expect("peeked name");
                let ident = self.ident(tok);
                Pattern::Binding {
                    name: ident,
                    span: tok.span,
                }
            }
            _ => self.bad_pattern(node.span, "expected a binding name"),
        };
        let op = match cur.peek_kind() {
            Some(K::Colon) => {
                cur.bump();
                BindOp::Colon
            }
            _ => {
                cur.eat(K::ColonEq);
                BindOp::ColonEq
            }
        };
        let value = self.expr_child(&mut cur, node.span);
        (pattern, op, value)
    }

    fn cond(&mut self, cur: &mut Cursor<'_>, fallback: Span) -> Cond {
        match cur.peek_kind() {
            Some(K::BindStmt) => {
                let node = cur.node().expect("peeked bind");
                let (pattern, _op, value) = self.bind_parts(node);
                Cond::Bind {
                    pattern,
                    value: Box::new(value),
                    span: node.span,
                }
            }
            _ => Cond::Expr(Box::new(self.expr_child(cur, fallback))),
        }
    }

    fn block(&mut self, node: &SyntaxNode) -> Block {
        let mut stmts = Vec::new();
        let mut tail = None;
        let children = node.child_list();
        let last = children.len().saturating_sub(1);
        for (i, child) in children.into_iter().enumerate() {
            if is_expr_kind(child.kind) && i == last {
                tail = Some(Box::new(self.expr(child)));
            } else {
                stmts.push(self.stmt(child));
            }
        }
        Block {
            stmts,
            tail,
            span: node.span,
        }
    }

    // -- expressions -------------------------------------------------------

    fn expr_error(&mut self, node: &SyntaxNode, message: &str) -> Expr {
        self.err(node.span, message);
        Expr::error(node.span)
    }

    fn expr_child(&mut self, cur: &mut Cursor<'_>, fallback: Span) -> Expr {
        match cur.node() {
            Some(node) => self.expr(node),
            None => {
                self.err(fallback, "expected an expression");
                Expr::error(fallback)
            }
        }
    }

    fn expr(&mut self, node: &SyntaxNode) -> Expr {
        match node.kind {
            K::LiteralExpr => {
                let tok = node.tokens().next().copied();
                match tok {
                    Some(tok) => {
                        let kind = match tok.kind {
                            K::IntLiteral => LiteralKind::Int,
                            K::FloatLiteral => LiteralKind::Float,
                            K::Text => LiteralKind::Text,
                            K::RawText => LiteralKind::RawText,
                            K::CharLiteral => LiteralKind::Char,
                            _ => LiteralKind::Bool,
                        };
                        let text = self.text(tok).to_string();
                        Expr::Literal {
                            kind,
                            text,
                            span: node.span,
                        }
                    }
                    None => self.bad_expr(node.span, "literal without a token"),
                }
            }
            K::PathExpr => {
                let mut cur = Cursor::new(node);
                let mut segments = Vec::new();
                while let Some(tok) = cur.name() {
                    segments.push(self.ident(tok));
                    cur.eat(K::ColonColon);
                }
                Expr::Path {
                    segments,
                    span: node.span,
                }
            }
            K::ParenExpr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::LParen);
                match cur.node() {
                    Some(inner) => self.expr(inner),
                    // `()` — the unit value.
                    None => Expr::Tuple {
                        elems: Vec::new(),
                        span: node.span,
                    },
                }
            }
            K::TupleExpr => {
                let mut elems = Vec::new();
                for child in node.child_list() {
                    if is_expr_kind(child.kind) {
                        elems.push(self.expr(child));
                    }
                }
                Expr::Tuple {
                    elems,
                    span: node.span,
                }
            }
            K::ArrayExpr => {
                let mut elems = Vec::new();
                if let Some(list) = node.child_node(K::ArgList) {
                    for arg in list.child_nodes(K::Arg) {
                        elems.push(self.arg(arg).value);
                    }
                }
                Expr::Array {
                    elems,
                    span: node.span,
                }
            }
            K::LambdaExpr => {
                let mut params = Vec::new();
                let mut body = Block::empty(node.span);
                for child in node.child_list() {
                    match child.kind {
                        K::LambdaParams => {
                            let mut cur = Cursor::new(child);
                            cur.eat(K::LBrace);
                            loop {
                                match cur.peek_kind() {
                                    Some(K::Name) => {
                                        let tok = cur.name().expect("peeked name");
                                        let ident = self.ident(tok);
                                        params.push(Pattern::Binding {
                                            name: ident,
                                            span: tok.span,
                                        });
                                    }
                                    Some(K::Underscore) => {
                                        let tok = cur.bump().expect("peeked underscore");
                                        params.push(Pattern::Wildcard(tok.span()));
                                    }
                                    _ => break,
                                }
                                if cur.eat(K::Comma).is_none() {
                                    break;
                                }
                            }
                        }
                        K::Block => body = self.block(child),
                        _ => {}
                    }
                }
                Expr::Lambda {
                    params,
                    body,
                    span: node.span,
                }
            }
            K::CallExpr => {
                let mut cur = Cursor::new(node);
                let Some(callee_node) = cur.node() else {
                    return self.bad_expr(node.span, "call without a callee");
                };
                let callee = self.expr(callee_node);
                let mut args = Vec::new();
                while let Some(element) = cur.peek() {
                    match element {
                        Element::Node(n) if n.kind == K::Arg => {
                            let arg_node = cur.node().expect("peeked arg");
                            args.push(self.arg(arg_node));
                        }
                        // A parenthesised argument list splices into the call.
                        Element::Node(n) if n.kind == K::ArgList => {
                            let list = cur.node().expect("peeked list");
                            self.append_args(list, &mut args);
                        }
                        _ => break,
                    }
                }
                Expr::Call {
                    callee: Box::new(callee),
                    args,
                    span: node.span,
                }
            }
            K::IndexExpr => {
                let mut cur = Cursor::new(node);
                let Some(base_node) = cur.node() else {
                    return self.bad_expr(node.span, "index without a target");
                };
                let base = self.expr(base_node);
                let mut args = Vec::new();
                if let Some(list) = cur.expect_node(K::ArgList) {
                    self.append_args(list, &mut args);
                } else {
                    self.err(node.span, "expected `[` arguments");
                }
                Expr::Index {
                    base: Box::new(base),
                    args,
                    span: node.span,
                }
            }
            K::TryExpr => {
                let mut cur = Cursor::new(node);
                let base = self.expr_child(&mut cur, node.span);
                Expr::Try {
                    base: Box::new(base),
                    span: node.span,
                }
            }
            K::UnaryExpr => {
                let mut cur = Cursor::new(node);
                let op = match cur.peek_kind() {
                    Some(K::Minus) => UnaryOp::Neg,
                    Some(K::Star) => UnaryOp::Deref,
                    Some(K::KwSize) => UnaryOp::Size,
                    Some(K::KwShapeof) => UnaryOp::Shapeof,
                    Some(K::KwReshape) => UnaryOp::Reshape,
                    _ => UnaryOp::Not,
                };
                cur.bump();
                let operand = self.expr_child(&mut cur, node.span);
                Expr::Unary {
                    op,
                    operand: Box::new(operand),
                    span: node.span,
                }
            }
            K::BinaryExpr => {
                let mut cur = Cursor::new(node);
                let lhs = self.expr_child(&mut cur, node.span);
                // `as` / `is` carry a type on the right-hand side.
                if matches!(cur.peek_kind(), Some(K::KwAs) | Some(K::KwIs)) {
                    let is_cast = cur.peek_kind() == Some(K::KwAs);
                    cur.bump();
                    let ty = if matches!(cur.peek_kind(), Some(k) if is_type_kind(k)) {
                        self.type_child(&mut cur, node.span)
                    } else {
                        self.err(node.span, "expected a type after `as`/`is`");
                        Type::Error(node.span)
                    };
                    return if is_cast {
                        Expr::Cast {
                            expr: Box::new(lhs),
                            ty,
                            span: node.span,
                        }
                    } else {
                        Expr::Is {
                            expr: Box::new(lhs),
                            ty,
                            span: node.span,
                        }
                    };
                }
                let op = match cur.peek_kind() {
                    Some(K::Name) => match self.text_of(&cur).as_str() {
                        "and" => BinaryOp::And,
                        "or" => BinaryOp::Or,
                        other => {
                            self.err(node.span, format!("unknown binary operator `{other}`"));
                            return Expr::error(node.span);
                        }
                    },
                    Some(k) => match binary_op(k) {
                        Some(op) => op,
                        None => {
                            self.err(node.span, "unknown binary operator");
                            return Expr::error(node.span);
                        }
                    },
                    None => return Expr::error(node.span),
                };
                cur.bump();
                let rhs = self.expr_child(&mut cur, node.span);
                Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span: node.span,
                }
            }
            K::IfExpr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwIf);
                cur.eat(K::LParen);
                let cond = self.cond(&mut cur, node.span);
                cur.eat(K::RParen);
                let then_branch = match cur.expect_node(K::Block) {
                    Some(b) => self.block(b),
                    None => {
                        self.err(
                            cur.span_or(node.span),
                            "expected the `then` block of an `if`",
                        );
                        Block::empty(node.span)
                    }
                };
                let else_branch = if cur.eat(K::KwElse).is_some() {
                    match cur.peek_kind() {
                        Some(K::Block) => {
                            let b = cur.node().expect("peeked block");
                            Some(ElseBranch::Block(self.block(b)))
                        }
                        Some(K::IfExpr) => {
                            let e = cur.node().expect("peeked if");
                            Some(ElseBranch::If(Box::new(self.expr(e))))
                        }
                        _ => {
                            self.err(cur.span_or(node.span), "expected `else` body");
                            None
                        }
                    }
                } else {
                    None
                };
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                    span: node.span,
                }
            }
            K::MatchExpr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwMatch);
                let scrutinee = self.expr_child(&mut cur, node.span);
                let mut arms = Vec::new();
                while let Some(arm) = cur.expect_node(K::MatchArm) {
                    arms.push(self.match_arm(arm));
                }
                Expr::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    span: node.span,
                }
            }
            K::MatchArm => self.expr_error(node, "match arms are lowered through their parent"),
            K::ComptimeExpr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwComptime);
                let body = match cur.expect_node(K::Block) {
                    Some(b) => self.block(b),
                    None => {
                        self.err(cur.span_or(node.span), "expected a `comptime` block");
                        Block::empty(node.span)
                    }
                };
                Expr::Comptime {
                    body,
                    span: node.span,
                }
            }
            K::UnsafeExpr => {
                let mut cur = Cursor::new(node);
                cur.eat(K::KwUnsafe);
                let body = match cur.expect_node(K::Block) {
                    Some(b) => self.block(b),
                    None => {
                        self.err(cur.span_or(node.span), "expected an `unsafe` block");
                        Block::empty(node.span)
                    }
                };
                Expr::Unsafe {
                    body,
                    span: node.span,
                }
            }
            K::Block => Expr::Block(self.block(node)),
            _ => {
                self.err(node.span, format!("unsupported expression {:?}", node.kind));
                Expr::error(node.span)
            }
        }
    }

    fn append_args(&mut self, list: &SyntaxNode, args: &mut Vec<Arg>) {
        for arg in list.child_nodes(K::Arg) {
            args.push(self.arg(arg));
        }
    }

    fn arg(&mut self, node: &SyntaxNode) -> Arg {
        let mut cur = Cursor::new(node);
        cur.eat(K::LParen);
        let convention = self.convention(&mut cur);
        let value = self.expr_child(&mut cur, node.span);
        Arg {
            convention,
            value,
            span: node.span,
        }
    }

    fn match_arm(&mut self, node: &SyntaxNode) -> MatchArm {
        let mut cur = Cursor::new(node);
        cur.eat(K::Pipe);
        let pattern = match cur.expect_node(K::MatchPat) {
            Some(p) => self.pattern(p),
            None => Pattern::Error(node.span),
        };
        let guard = if cur.eat(K::KwIf).is_some() {
            Some(self.expr_child(&mut cur, node.span))
        } else {
            None
        };
        cur.eat(K::Arrow);
        let body = self.expr_child(&mut cur, node.span);
        MatchArm {
            pattern,
            guard,
            body,
            span: node.span,
        }
    }

    fn pattern(&mut self, node: &SyntaxNode) -> Pattern {
        let mut cur = Cursor::new(node);
        match cur.peek_kind() {
            Some(K::Underscore) => {
                let tok = cur.bump().expect("peeked underscore");
                Pattern::Wildcard(tok.span())
            }
            Some(K::Name) => {
                let mut path = Vec::new();
                let first = cur.name().expect("peeked name");
                let name = self.ident(first);
                path.push(name);
                while cur.peek_kind() == Some(K::ColonColon) {
                    cur.bump();
                    if let Some(tok) = cur.name() {
                        path.push(self.ident(tok));
                    } else {
                        break;
                    }
                }
                // Constructor arguments, if any.
                let mut args = Vec::new();
                while let Some(next) = cur.peek_kind() {
                    match next {
                        K::MatchPat => {
                            let arg = cur.node().expect("peeked pattern");
                            args.push(self.pattern(arg));
                        }
                        K::Name
                        | K::Underscore
                        | K::IntLiteral
                        | K::FloatLiteral
                        | K::Text
                        | K::CharLiteral
                        | K::KwTrue
                        | K::KwFalse => {
                            let tok = cur.bump_token().expect("peeked token");
                            match next {
                                K::Underscore => args.push(Pattern::Wildcard(tok.span())),
                                K::Name => args.push(Pattern::Binding {
                                    name: self.ident(tok),
                                    span: tok.span(),
                                }),
                                _ => args.push(Pattern::Literal {
                                    text: self.text(tok).to_string(),
                                    span: tok.span(),
                                }),
                            }
                        }
                        _ => break,
                    }
                }
                if args.is_empty() && path.len() == 1 {
                    Pattern::Binding {
                        name,
                        span: node.span,
                    }
                } else {
                    Pattern::Constructor {
                        path,
                        args,
                        span: node.span,
                    }
                }
            }
            Some(K::LParen) => {
                cur.bump();
                let mut elems = Vec::new();
                while let Some(kind) = cur.peek_kind() {
                    if kind == K::RParen {
                        break;
                    }
                    if kind == K::MatchPat {
                        let arg = cur.node().expect("peeked pattern");
                        elems.push(self.pattern(arg));
                    } else {
                        elems.push(Pattern::Error(cur.span_or(node.span)));
                        cur.bump();
                    }
                    cur.eat(K::Comma);
                }
                Pattern::Tuple {
                    elems,
                    span: node.span,
                }
            }
            Some(k) if k.is_literal() => {
                let tok = cur.bump_token().expect("peeked literal");
                Pattern::Literal {
                    text: self.text(tok).to_string(),
                    span: tok.span(),
                }
            }
            _ => self.bad_pattern(node.span, "unsupported pattern"),
        }
    }
}

fn binary_op(kind: K) -> Option<BinaryOp> {
    Some(match kind {
        K::Pipe => BinaryOp::Choice,
        K::DotDot => BinaryOp::RangeInclusive,
        K::DotDotLt => BinaryOp::RangeHalfOpen,
        K::Assign => BinaryOp::Eq,
        K::EqEq => BinaryOp::EqEq,
        K::BangEq => BinaryOp::NotEq,
        K::Lt => BinaryOp::Lt,
        K::Gt => BinaryOp::Gt,
        K::LtEq => BinaryOp::LtEq,
        K::GtEq => BinaryOp::GtEq,
        K::Dot => BinaryOp::Flow,
        K::PlusPlus => BinaryOp::Concat,
        K::Plus => BinaryOp::Add,
        K::Minus => BinaryOp::Sub,
        K::Caret => BinaryOp::BitXor,
        K::PipeAmp => BinaryOp::BitOr,
        K::Star => BinaryOp::Mul,
        K::Slash => BinaryOp::Div,
        K::Percent => BinaryOp::Rem,
        K::Shl => BinaryOp::Shl,
        K::Shr => BinaryOp::Shr,
        K::Amp => BinaryOp::BitAnd,
        _ => return None,
    })
}

fn is_expr_kind(kind: K) -> bool {
    matches!(
        kind,
        K::LiteralExpr
            | K::PathExpr
            | K::ParenExpr
            | K::TupleExpr
            | K::ArrayExpr
            | K::LambdaExpr
            | K::CallExpr
            | K::IndexExpr
            | K::TryExpr
            | K::UnaryExpr
            | K::BinaryExpr
            | K::IfExpr
            | K::MatchExpr
            | K::ComptimeExpr
            | K::UnsafeExpr
            | K::Block
    )
}

fn is_stmt_kind(kind: K) -> bool {
    matches!(
        kind,
        K::VarStmt
            | K::SetStmt
            | K::YieldStmt
            | K::BindStmt
            | K::TryBindStmt
            | K::WhenStmt
            | K::ForStmt
            | K::LetDecl
            | K::ExprStmt
    )
}

/// True when the cursor sits on a type node.
fn starts_type_element(cur: &Cursor<'_>) -> bool {
    matches!(cur.peek_kind(), Some(kind) if is_type_kind(kind))
}

fn is_type_kind(kind: K) -> bool {
    matches!(
        kind,
        K::TypeRef
            | K::TypePath
            | K::TypeApp
            | K::TypeTuple
            | K::TypeArray
            | K::TypePtr
            | K::TypeDyn
    )
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// Ordered iteration over a node's non-trivia children (tokens and nodes).
struct Cursor<'a> {
    items: Vec<&'a Element>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(node: &'a SyntaxNode) -> Self {
        let items = node
            .children
            .iter()
            .filter(|e| !e.kind().is_trivia())
            .collect();
        Cursor { items, pos: 0 }
    }

    fn peek(&self) -> Option<&'a Element> {
        self.items.get(self.pos).copied()
    }

    fn peek_kind(&self) -> Option<K> {
        self.peek().map(|e| e.kind())
    }

    fn peek_nth_kind(&self, n: usize) -> Option<K> {
        self.items.get(self.pos + n).map(|e| e.kind())
    }

    fn bump(&mut self) -> Option<&'a Element> {
        let item = self.peek()?;
        self.pos += 1;
        Some(item)
    }

    fn eat(&mut self, kind: K) -> Option<&'a Element> {
        if self.peek_kind() == Some(kind) {
            self.bump()
        } else {
            None
        }
    }

    /// Consumes a child node of the given kind, if present.
    fn expect_node(&mut self, kind: K) -> Option<&'a SyntaxNode> {
        match self.peek() {
            Some(Element::Node(node)) if node.kind == kind => {
                self.pos += 1;
                Some(node)
            }
            _ => None,
        }
    }

    /// Consumes the next child if it is a node.
    fn node(&mut self) -> Option<&'a SyntaxNode> {
        match self.peek() {
            Some(Element::Node(node)) => {
                self.pos += 1;
                Some(node)
            }
            _ => None,
        }
    }

    /// Consumes the next child if it is a token of `kind`.
    fn eat_token(&mut self, kind: K) -> Option<SyntaxToken> {
        match self.peek() {
            Some(Element::Token(tok)) if tok.kind == kind => {
                self.pos += 1;
                Some(*tok)
            }
            _ => None,
        }
    }

    /// Consumes the next child if it is a token, of whatever kind.
    fn bump_token(&mut self) -> Option<SyntaxToken> {
        match self.peek() {
            Some(Element::Token(tok)) => {
                self.pos += 1;
                Some(*tok)
            }
            _ => None,
        }
    }

    /// Consumes the next child if it is a `Name`/`Underscore` token.
    fn name(&mut self) -> Option<SyntaxToken> {
        match self.peek() {
            Some(Element::Token(tok)) if tok.kind == K::Name || tok.kind == K::Underscore => {
                self.pos += 1;
                Some(*tok)
            }
            _ => None,
        }
    }

    fn span_or(&self, fallback: Span) -> Span {
        self.peek().map(|e| e.span()).unwrap_or(fallback)
    }
}
