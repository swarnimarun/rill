//! Recursive-descent parser. Unsupported syntax is rejected rather than guessed.
use crate::ast::{
    BinaryOp, Expr, ExternConst, ExternFunction, ExternParam, ExternType, Function, FunctionOp,
    MatchArm, Param, ParamMode, Pattern, Program, TypeDecl, TypeExpr, VariantDecl,
};
use crate::lexer::{lex_lossless, Kind, Token};
use crate::syntax::{AstDeclaration, Builder, DiagnosticCode, Marker, SyntaxKind};
use std::cell::Cell;
use std::collections::HashSet;

pub fn parse(source: &str) -> Result<Program, String> {
    Parser::new(lex_lossless(source)?, false, None)
        .program()
        .map(|m| m.program)
}

/// Parse module syntax without resolving imports or checking visibility.
pub fn parse_module(source: &str) -> Result<ParsedModule, String> {
    Parser::new(lex_lossless(source)?, true, None).program()
}

pub(crate) fn source_tree(source: crate::source::SourceFile<'_>) -> crate::syntax::SyntaxTree<'_> {
    let mut parser = Parser::new(source.clone(), true, Some(Builder::default()));
    let module = parser
        .program()
        .expect("source recovery handles syntax failures");
    parser.syntax.take().unwrap().build(source, module)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedModule {
    pub program: Program,
    pub exports: Vec<Export>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportKind {
    Function,
    Constant,
    Type,
}

struct Parser {
    tokens: Vec<Token>,
    adjacent: Vec<bool>,
    pos: usize,
    depth: usize,
    module_mode: bool,
    module_aliases: HashSet<String>,
    block_id: usize,
    syntax: Option<Builder>,
    failure: Cell<Option<(usize, DiagnosticCode)>>,
}
const MAX_DEPTH: usize = 64;
impl Parser {
    fn new(
        source: crate::source::SourceFile<'_>,
        module_mode: bool,
        syntax: Option<Builder>,
    ) -> Self {
        let mut previous_end = None;
        let adjacent = source
            .lexemes()
            .iter()
            .filter_map(|piece| {
                if matches!(piece.kind(), crate::source::LexemeKind::Token(_)) {
                    let touches = previous_end == Some(piece.span().start.byte);
                    previous_end = Some(piece.span().end.byte);
                    Some(touches)
                } else {
                    None
                }
            })
            .collect();
        Self {
            tokens: source.into_tokens(),
            adjacent,
            pos: 0,
            depth: 0,
            module_mode,
            module_aliases: HashSet::new(),
            block_id: 0,
            syntax,
            failure: Cell::new(None),
        }
    }
    fn mark(&self) -> Option<Marker> {
        self.syntax.as_ref().map(|s| s.mark(self.pos))
    }
    fn finish(
        &mut self,
        marker: Option<Marker>,
        kind: SyntaxKind,
    ) -> Option<crate::syntax::NodeId> {
        marker.and_then(|m| self.syntax.as_mut().unwrap().finish(m, self.pos, kind))
    }
    fn node<T>(
        &mut self,
        kind: SyntaxKind,
        parse: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let marker = self.mark();
        let result = parse(self);
        self.finish(
            marker,
            if result.is_ok() {
                kind
            } else {
                SyntaxKind::Error
            },
        );
        result
    }
    fn error_at(&self, pos: usize, code: DiagnosticCode, message: String) -> String {
        self.failure.set(Some((pos, code)));
        message
    }

    fn guarded<T>(&mut self, parse: fn(&mut Self) -> Result<T, String>) -> Result<T, String> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("E-PARSE-DEPTH: syntax nesting exceeds 64"));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }
    fn peek(&self) -> &Kind {
        &self.tokens[self.pos].kind
    }
    fn take(&mut self) -> Kind {
        let k = self.peek().clone();
        if k != Kind::Eof {
            self.pos += 1;
        }
        k
    }
    fn error(&self, message: &str) -> String {
        let t = &self.tokens[self.pos];
        self.error_at(
            self.pos,
            if message.starts_with("E-PARSE-DEPTH:") {
                DiagnosticCode::Depth
            } else {
                DiagnosticCode::Syntax
            },
            format!("{}:{}: {} (found {:?})", t.line, t.column, message, t.kind),
        )
    }
    fn expect(&mut self, kind: Kind) -> Result<(), String> {
        if self.peek() == &kind {
            self.take();
            Ok(())
        } else {
            Err(self.error(&format!("expected {kind:?}")))
        }
    }
    fn name(&mut self) -> Result<String, String> {
        self.node(SyntaxKind::Name, Self::name_syntax)
    }
    fn name_syntax(&mut self) -> Result<String, String> {
        if let Kind::Ident(s) = self.peek().clone() {
            self.take();
            Ok(s)
        } else {
            Err(self.error("expected identifier"))
        }
    }
    fn program(&mut self) -> Result<ParsedModule, String> {
        let mut module = ParsedModule {
            program: Program {
                native: crate::c_import::Config::default(),
                constants: Vec::new(),
                allocation_types: None,
                types: Vec::new(),
                externs: Vec::new(),
                functions: Vec::new(),
            },
            exports: Vec::new(),
        };
        let mut declarations_started = false;
        while *self.peek() != Kind::Eof {
            let start = self.pos;
            let marker = self.mark();
            let kind = self.item_kind();
            let lengths = (
                module.program.native.includes.len(),
                module.program.native.links.len(),
                module.program.native.packages.len(),
            );
            match self.item(&mut module, &mut declarations_started) {
                Ok(declaration) => {
                    let node = self.finish(marker, kind);
                    if let (Some(builder), Some(node), Some(declaration)) =
                        (&mut self.syntax, node, declaration)
                    {
                        builder.map_declaration(declaration, node);
                    }
                }
                Err(message) => {
                    if self.syntax.is_none() {
                        return Err(message);
                    }
                    // A failed native directive must not poison later validation.
                    module.program.native.includes.truncate(lengths.0);
                    module.program.native.links.truncate(lengths.1);
                    module.program.native.packages.truncate(lengths.2);
                    let (token, code) = self
                        .failure
                        .take()
                        .unwrap_or((self.pos, DiagnosticCode::Syntax));
                    self.syntax
                        .as_mut()
                        .unwrap()
                        .diagnostic(token, code, message);
                    self.syntax.as_mut().unwrap().rewind(marker.unwrap());
                    self.recover_item(start, token);
                    self.finish(marker, SyntaxKind::Error);
                    if self.syntax.as_ref().unwrap().at_error_limit() && *self.peek() != Kind::Eof {
                        let rest = self.mark();
                        self.syntax.as_mut().unwrap().diagnostic(
                            self.pos,
                            DiagnosticCode::RecoveryLimit,
                            "E-PARSE-RECOVERY-LIMIT: stopped after 63 syntax errors".into(),
                        );
                        self.pos = self.tokens.len() - 1;
                        self.finish(rest, SyntaxKind::Error);
                    }
                }
            }
        }
        Ok(module)
    }
    fn item_kind(&self) -> SyntaxKind {
        let offset =
            usize::from(self.module_mode && matches!(self.peek(), Kind::Ident(n) if n == "export"));
        match &self.tokens[self.pos + offset].kind {
            Kind::At => SyntaxKind::NativeDirective,
            Kind::Ident(n) if n == "import" && self.module_mode => SyntaxKind::Import,
            Kind::Type => SyntaxKind::TypeDeclaration,
            Kind::Extern
                if self
                    .tokens
                    .get(self.pos + offset + 1)
                    .is_some_and(|t| t.kind == Kind::Const) =>
            {
                SyntaxKind::ExternConstant
            }
            Kind::Extern => SyntaxKind::ExternFunction,
            _ => SyntaxKind::Function,
        }
    }
    fn item(
        &mut self,
        module: &mut ParsedModule,
        declarations_started: &mut bool,
    ) -> Result<Option<AstDeclaration>, String> {
        let start = self.pos;
        if !*declarations_started {
            if *self.peek() == Kind::At {
                self.native_directive(&mut module.program.native)?;
                return Ok(None);
            }
            if self.module_mode && matches!(self.peek(), Kind::Ident(n) if n == "import") {
                self.take();
                let mut alias = self.name()?;
                while *self.peek() == Kind::Dot {
                    self.take();
                    alias = self.name()?;
                }
                if matches!(self.peek(), Kind::Ident(n) if n == "as") {
                    self.take();
                    alias = self.name()?;
                }
                self.expect(Kind::Semi)?;
                self.module_aliases.insert(alias);
                return Ok(None);
            }
        }
        *declarations_started = true;
        if *self.peek() == Kind::At {
            return Err(self.error("native directives must precede declarations"));
        }
        let exported =
            self.module_mode && matches!(self.peek(), Kind::Ident(name) if name == "export");
        if exported {
            self.take();
        }
        if *self.peek() == Kind::Extern {
            if self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.kind == Kind::Const)
            {
                self.take();
                self.take();
                let name = self.name()?;
                self.expect(Kind::Colon)?;
                let ty = self.extern_type()?;
                self.expect(Kind::Semi)?;
                if exported {
                    module.exports.push(Export {
                        name: name.clone(),
                        kind: ExportKind::Constant,
                    });
                }
                module.program.constants.push(ExternConst { name, ty });
            } else {
                let declaration = self.extern_decl()?;
                if exported {
                    module.exports.push(Export {
                        name: declaration.name.clone(),
                        kind: ExportKind::Function,
                    });
                }
                module.program.externs.push(declaration);
            }
            return Ok(Some(
                if self.tokens[if exported { start + 2 } else { start + 1 }].kind == Kind::Const {
                    AstDeclaration::Constant(module.program.constants.len() - 1)
                } else {
                    AstDeclaration::ExternFunction(module.program.externs.len() - 1)
                },
            ));
        }
        if *self.peek() == Kind::Type {
            let decl = self.type_decl()?;
            if exported {
                module.exports.push(Export {
                    name: decl.name.clone(),
                    kind: ExportKind::Type,
                });
            }
            module.program.types.push(decl);
            if *self.peek() == Kind::Semi {
                self.take();
            }
            return Ok(Some(AstDeclaration::Type(module.program.types.len() - 1)));
        }
        if *self.peek() == Kind::Fn {
            self.take();
            let name = self.name()?;
            let params = self.fn_params()?;
            let result_annotation = if *self.peek() == Kind::ThinArrow {
                self.take();
                Some(self.type_expr()?)
            } else {
                None
            };
            let body = self.block()?;
            self.check_expr_depth(&body)?;
            if *self.peek() == Kind::Semi {
                self.take();
            }
            if exported {
                module.exports.push(Export {
                    name: name.clone(),
                    kind: ExportKind::Function,
                });
            }
            module.program.functions.push(Function {
                name,
                params,
                result_annotation,
                body,
            });
            return Ok(Some(AstDeclaration::Function(
                module.program.functions.len() - 1,
            )));
        }
        self.expect(Kind::Let)?;
        let name = self.name()?;
        let mut params = Vec::new();
        let parameters = self.mark();
        while matches!(self.peek(), Kind::Ident(_) | Kind::LParen) {
            let parameter = self.mark();
            if *self.peek() == Kind::LParen {
                self.take();
                let mode = match self.peek() {
                    Kind::Inout => {
                        self.take();
                        ParamMode::Inout
                    }
                    Kind::Read => {
                        self.take();
                        ParamMode::Read
                    }
                    _ => ParamMode::Value,
                };
                let name = self.name()?;
                let annotation = if *self.peek() == Kind::Colon {
                    self.take();
                    Some(self.type_expr()?)
                } else {
                    if mode == ParamMode::Value {
                        return Err(
                            self.error("value parameter in parentheses requires a type annotation")
                        );
                    }
                    None
                };
                self.expect(Kind::RParen)?;
                params.push(Param {
                    name,
                    mode,
                    annotation,
                });
            } else {
                params.push(Param {
                    name: self.name()?,
                    mode: ParamMode::Value,
                    annotation: None,
                });
            }
            self.finish(parameter, SyntaxKind::Parameter);
        }
        self.finish(parameters, SyntaxKind::Parameters);
        self.expect(Kind::Equal)?;
        let body = self.expr()?;
        if *self.peek() == Kind::Semi {
            self.take();
        }
        if exported {
            module.exports.push(Export {
                name: name.clone(),
                kind: ExportKind::Function,
            });
        }
        module.program.functions.push(Function {
            name,
            params,
            result_annotation: None,
            body,
        });
        Ok(Some(AstDeclaration::Function(
            module.program.functions.len() - 1,
        )))
    }
    // Recovery is tooling-only. Delimiters give ordinary boundaries. A later
    // line at the item's indentation may recover an unambiguous declaration
    // even after its prefix was consumed by the failed production.
    fn recover_item(&mut self, start: usize, failure: usize) {
        let mut braces = 0usize;
        let mut parens = 0usize;
        for index in start..self.tokens.len() - 1 {
            let token = &self.tokens[index];
            let named_function =
                token.kind == Kind::Fn && matches!(self.tokens[index + 1].kind, Kind::Ident(_));
            let unambiguous = named_function
                || matches!(token.kind, Kind::Type | Kind::Extern)
                || matches!(&token.kind, Kind::Ident(n) if n == "export");
            let item = unambiguous
                || matches!(token.kind, Kind::Let | Kind::At)
                || matches!(&token.kind, Kind::Ident(n) if n == "import");
            if index > start
                && item
                && (((index >= failure || (named_function && index + 1 == failure))
                    && braces == 0
                    && parens == 0)
                    || (unambiguous
                        && token.line > self.tokens[start].line
                        && token.column <= self.tokens[start].column))
            {
                self.pos = index;
                return;
            }
            match token.kind {
                Kind::LBrace => braces += 1,
                Kind::RBrace => braces = braces.saturating_sub(1),
                Kind::LParen => parens += 1,
                Kind::RParen => parens = parens.saturating_sub(1),
                _ => {}
            }
        }
        self.pos = self.tokens.len() - 1;
    }
    fn fn_params(&mut self) -> Result<Vec<Param>, String> {
        self.node(SyntaxKind::Parameters, Self::fn_params_syntax)
    }
    fn fn_params_syntax(&mut self) -> Result<Vec<Param>, String> {
        self.expect(Kind::LParen)?;
        let mut params = Vec::new();
        while *self.peek() != Kind::RParen {
            let parameter = self.mark();
            let mode = match self.peek() {
                Kind::Read => {
                    self.take();
                    ParamMode::Read
                }
                Kind::Inout => {
                    self.take();
                    ParamMode::Inout
                }
                _ => ParamMode::Value,
            };
            let name = self.name()?;
            let annotation = if *self.peek() == Kind::Colon {
                self.take();
                Some(self.type_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                mode,
                annotation,
            });
            self.finish(parameter, SyntaxKind::Parameter);
            if *self.peek() != Kind::Comma {
                break;
            }
            self.take();
        }
        self.expect(Kind::RParen)?;
        Ok(params)
    }
    fn block(&mut self) -> Result<Expr, String> {
        self.node(SyntaxKind::Block, Self::block_syntax)
    }
    fn block_syntax(&mut self) -> Result<Expr, String> {
        if *self.peek() != Kind::LBrace {
            return Err(self.error("expected '{' to start block"));
        }
        self.take();
        let body = self.guarded(Self::block_body)?;
        if *self.peek() != Kind::RBrace {
            return Err(self.error("expected '}' to close block"));
        }
        self.take();
        Ok(body)
    }
    fn block_body(&mut self) -> Result<Expr, String> {
        if *self.peek() == Kind::RBrace {
            return Ok(Expr::Unit);
        }
        if *self.peek() == Kind::Eof {
            return Err(self.error("expected '}' to close block"));
        }
        let marker = self.mark();
        if *self.peek() == Kind::Let {
            self.take();
            let mutable = if *self.peek() == Kind::Mut {
                self.take();
                true
            } else {
                false
            };
            let name = self.name()?;
            self.expect(Kind::Equal)?;
            let value = self.expr()?;
            if *self.peek() != Kind::Semi {
                return Err(self.error("expected ';' after local binding"));
            }
            self.take();
            self.finish(marker, SyntaxKind::LocalBinding);
            let body = self.guarded(Self::block_body)?;
            return Ok(if mutable {
                Expr::LetMut(name, Box::new(value), Box::new(body))
            } else {
                Expr::Let(name, Box::new(value), Box::new(body))
            });
        }
        let value = self.expr()?;
        if *self.peek() == Kind::RBrace {
            return Ok(value);
        }
        if *self.peek() == Kind::Eof {
            return Err(self.error("expected '}' to close block"));
        }
        if *self.peek() != Kind::Semi {
            return Err(
                self.error("expected ';' between block statements or '}' after final expression")
            );
        }
        self.take();
        self.finish(marker, SyntaxKind::ExpressionStatement);
        // '$' is excluded from source identifiers, so this binding cannot shadow user code.
        let name = format!("$block{}", self.block_id);
        self.block_id += 1;
        let discard = format!("$discard{}", self.block_id);
        self.block_id += 1;
        let body = self.guarded(Self::block_body)?;
        let statement = Expr::Let(discard, Box::new(value), Box::new(Expr::Unit));
        Ok(Expr::Let(name, Box::new(statement), Box::new(body)))
    }
    fn qualified_name(&mut self, first: String) -> Result<String, String> {
        if self.module_mode && *self.peek() == Kind::Dot {
            self.take();
            Ok(format!("{first}.{}", self.name()?))
        } else {
            Ok(first)
        }
    }
    fn field_path(&mut self) -> Result<Vec<String>, String> {
        let mut path = Vec::new();
        while *self.peek() == Kind::Dot {
            if path.len() == 16 {
                return Err(self.error("E-PARSE-DEPTH: field path exceeds 16"));
            }
            self.take();
            path.push(self.name()?);
        }
        Ok(path)
    }
    fn extern_decl(&mut self) -> Result<ExternFunction, String> {
        self.expect(Kind::Extern)?;
        self.expect(Kind::Fn)?;
        let name = self.name()?;
        let parameters = self.mark();
        self.expect(Kind::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Kind::RParen {
            loop {
                let parameter = self.mark();
                let name = self.name()?;
                self.expect(Kind::Colon)?;
                let ty = self.extern_type()?;
                params.push(ExternParam { name, ty });
                self.finish(parameter, SyntaxKind::Parameter);
                if *self.peek() != Kind::Comma {
                    break;
                }
                self.take();
            }
        }
        self.expect(Kind::RParen)?;
        self.finish(parameters, SyntaxKind::Parameters);
        self.expect(Kind::ThinArrow)?;
        let result = self.extern_type()?;
        self.expect(Kind::Semi)?;
        Ok(ExternFunction {
            name,
            params,
            result,
        })
    }
    fn extern_type(&mut self) -> Result<ExternType, String> {
        self.node(SyntaxKind::ForeignType, Self::extern_type_syntax)
    }
    fn extern_type_syntax(&mut self) -> Result<ExternType, String> {
        let start = self.pos;
        let name = self.name()?;
        match name.as_str() {
            "i32" => Ok(ExternType::I32),
            "u8" => Ok(ExternType::U8),
            "u32" => Ok(ExternType::U32),
            "u64" => Ok(ExternType::U64),
            "f32" => Ok(ExternType::F32),
            "bool" => Ok(ExternType::Bool),
            "unit" => Ok(ExternType::Unit),
            "cstr" => Ok(ExternType::CStr),
            "ptr" | "const_ptr" => {
                self.expect(Kind::Lt)?;
                let pointee = self.name()?;
                self.expect(Kind::Gt)?;
                Ok(ExternType::Pointer {
                    pointee,
                    is_const: name == "const_ptr",
                })
            }
            _ => {
                let token = &self.tokens[start];
                Err(self.error_at(
                    start,
                    DiagnosticCode::ForeignType,
                    format!(
                        "{}:{}: E-FFI: unsupported foreign type {name:?}",
                        token.line, token.column
                    ),
                ))
            }
        }
    }
    fn native_directive(&mut self, native: &mut crate::c_import::Config) -> Result<(), String> {
        self.expect(Kind::At)?;
        let directive_start = self.pos;
        let name = self.name()?;
        if !matches!(name.as_str(), "include" | "link" | "pkg_config") {
            let token = &self.tokens[directive_start];
            return Err(self.error_at(
                directive_start,
                DiagnosticCode::NativeDirective,
                format!(
                    "{}:{}: unknown native directive @{name}",
                    token.line, token.column
                ),
            ));
        }
        self.expect(Kind::LParen)?;
        let start = self.pos;
        let Kind::Str(value) = self.peek().clone() else {
            return Err(self.error("native directive requires one ordinary string literal"));
        };
        self.take();
        self.expect(Kind::RParen)?;
        if *self.peek() != Kind::Semi {
            return Err(self.error("expected ';' after native directive"));
        }
        self.take();
        match name.as_str() {
            "include" => native.includes.push(value),
            "link" => native.links.push(value),
            "pkg_config" => native.packages.push(value),
            _ => unreachable!(),
        }
        native.validate().map_err(|message| {
            let token = &self.tokens[start];
            self.error_at(
                start,
                DiagnosticCode::NativeDirective,
                format!("{}:{}: {message}", token.line, token.column),
            )
        })
    }
    fn type_decl(&mut self) -> Result<TypeDecl, String> {
        self.expect(Kind::Type)?;
        let name = self.name()?;
        let mut params = Vec::new();
        if *self.peek() == Kind::Lt {
            let parameters = self.mark();
            self.take();
            loop {
                params.push(self.name()?);
                if *self.peek() != Kind::Comma {
                    break;
                }
                self.take();
            }
            self.expect(Kind::Gt)?;
            self.finish(parameters, SyntaxKind::TypeParameters);
        }
        self.expect(Kind::Equal)?;
        if *self.peek() == Kind::LBrace {
            self.take();
            let mut names = Vec::new();
            let mut fields = Vec::new();
            while *self.peek() != Kind::RBrace {
                let field = self.mark();
                names.push(self.name()?);
                self.expect(Kind::Colon)?;
                fields.push(self.type_expr()?);
                if *self.peek() == Kind::Semi {
                    self.take();
                } else if *self.peek() != Kind::RBrace {
                    return Err(self.error("expected semicolon between record fields"));
                }
                self.finish(field, SyntaxKind::RecordField);
            }
            self.expect(Kind::RBrace)?;
            return Ok(TypeDecl {
                name: name.clone(),
                params,
                variants: vec![VariantDecl { name, fields }],
                record_fields: Some(names),
            });
        }
        if *self.peek() == Kind::Pipe {
            self.take();
        }
        let mut variants = Vec::new();
        loop {
            let variant = self.mark();
            let name = self.name()?;
            let mut fields = Vec::new();
            if *self.peek() == Kind::LParen {
                self.take();
                if *self.peek() != Kind::RParen {
                    loop {
                        fields.push(self.type_expr()?);
                        if *self.peek() != Kind::Comma {
                            break;
                        }
                        self.take();
                    }
                }
                self.expect(Kind::RParen)?;
            }
            variants.push(VariantDecl { name, fields });
            self.finish(variant, SyntaxKind::Variant);
            if *self.peek() != Kind::Pipe {
                break;
            }
            self.take();
        }
        Ok(TypeDecl {
            name,
            params,
            variants,
            record_fields: None,
        })
    }
    fn type_expr(&mut self) -> Result<TypeExpr, String> {
        self.node(SyntaxKind::TypeExpression, Self::type_expr_syntax)
    }
    fn type_expr_syntax(&mut self) -> Result<TypeExpr, String> {
        self.guarded(Self::type_expr_inner)
    }
    fn type_expr_inner(&mut self) -> Result<TypeExpr, String> {
        let first = self.name()?;
        let name = self.qualified_name(first)?;
        let mut args = Vec::new();
        if *self.peek() == Kind::Lt {
            self.take();
            loop {
                args.push(self.type_expr()?);
                if *self.peek() != Kind::Comma {
                    break;
                }
                self.take();
            }
            self.expect(Kind::Gt)?;
        }
        Ok(TypeExpr { name, args })
    }
    fn expr(&mut self) -> Result<Expr, String> {
        let kind = match self.peek() {
            Kind::While => SyntaxKind::WhileExpression,
            Kind::Match => SyntaxKind::MatchExpression,
            Kind::Let => SyntaxKind::LetExpression,
            Kind::If => SyntaxKind::IfExpression,
            _ => SyntaxKind::Expression,
        };
        let expr = self.node(kind, |p| p.guarded(Self::expr_inner))?;
        self.check_expr_depth(&expr)?;
        Ok(expr)
    }
    fn expr_inner(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Kind::While => {
                self.take();
                let condition = self.expr()?;
                let body = if *self.peek() == Kind::LBrace {
                    self.block()?
                } else if *self.peek() == Kind::Do {
                    self.take();
                    self.expr()?
                } else {
                    return Err(self.error("expected '{' block or 'do' after while condition"));
                };
                Ok(Expr::While(Box::new(condition), Box::new(body)))
            }
            Kind::Match => {
                self.take();
                let value = self.expr()?;
                self.expect(Kind::LBrace)?;
                if *self.peek() == Kind::Pipe {
                    self.take();
                }
                let mut arms = Vec::new();
                loop {
                    let arm = self.mark();
                    let pattern = self.pattern()?;
                    self.expect(Kind::Arrow)?;
                    let body = self.expr()?;
                    arms.push(MatchArm { pattern, body });
                    self.finish(arm, SyntaxKind::MatchArm);
                    if *self.peek() != Kind::Pipe {
                        break;
                    }
                    self.take();
                }
                self.expect(Kind::RBrace)?;
                Ok(Expr::Match(Box::new(value), arms))
            }
            Kind::Let => {
                self.take();
                let mutable = if *self.peek() == Kind::Mut {
                    self.take();
                    true
                } else {
                    false
                };
                let name = self.name()?;
                self.expect(Kind::Equal)?;
                let value = self.expr()?;
                self.expect(Kind::In)?;
                let body = self.expr()?;
                if mutable {
                    Ok(Expr::LetMut(name, Box::new(value), Box::new(body)))
                } else {
                    Ok(Expr::Let(name, Box::new(value), Box::new(body)))
                }
            }
            Kind::If => {
                self.take();
                let cond = self.expr()?;
                let braced = *self.peek() == Kind::LBrace;
                let yes = if braced {
                    self.block()?
                } else {
                    self.expect(Kind::Then)?;
                    self.expr()?
                };
                let no = if braced && *self.peek() != Kind::Else {
                    Expr::Unit
                } else {
                    self.expect(Kind::Else)?;
                    if braced {
                        self.block()?
                    } else {
                        self.expr()?
                    }
                };
                Ok(Expr::If(Box::new(cond), Box::new(yes), Box::new(no)))
            }
            _ => {
                let marker = self.mark();
                let left = self.binary(0)?;
                if *self.peek() == Kind::Assign {
                    self.take();
                    let result = match left {
                        Expr::Var(name) => Ok(Expr::Assign(name, Box::new(self.expr()?))),
                        Expr::Field(root, field) => {
                            Ok(Expr::FieldAssign(root, field, Box::new(self.expr()?)))
                        }
                        _ => Err(self.error("assignment target must be a local name or field")),
                    };
                    self.finish(marker, SyntaxKind::AssignmentExpression);
                    result
                } else {
                    Ok(left)
                }
            }
        }
    }
    fn pattern(&mut self) -> Result<Pattern, String> {
        self.node(SyntaxKind::Pattern, Self::pattern_syntax)
    }
    fn pattern_syntax(&mut self) -> Result<Pattern, String> {
        self.guarded(Self::pattern_inner)
    }
    fn pattern_inner(&mut self) -> Result<Pattern, String> {
        match self.take() {
            Kind::Ident(name) if name == "_" => Ok(Pattern::Wildcard),
            Kind::Ident(name) => {
                let name = self.qualified_name(name)?;
                if *self.peek() == Kind::LParen {
                    self.take();
                    let mut fields = Vec::new();
                    if *self.peek() != Kind::RParen {
                        loop {
                            fields.push(self.pattern()?);
                            if *self.peek() != Kind::Comma {
                                break;
                            }
                            self.take();
                        }
                    }
                    self.expect(Kind::RParen)?;
                    Ok(Pattern::Constructor(name, fields))
                } else if name.contains('.') || name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    Ok(Pattern::Constructor(name, Vec::new()))
                } else {
                    Ok(Pattern::Binding(name))
                }
            }
            Kind::True => Ok(Pattern::Bool(true)),
            Kind::False => Ok(Pattern::Bool(false)),
            Kind::Int(n) => i32::try_from(n)
                .map(Pattern::Int)
                .map_err(|_| self.error("integer pattern out of i32 range")),
            Kind::Minus => match self.take() {
                Kind::Int(n) if n <= 2147483648 => Ok(Pattern::Int((-n) as i32)),
                _ => Err(self.error("expected negative i32 pattern")),
            },
            Kind::LParen => {
                if *self.peek() == Kind::RParen {
                    self.take();
                    Ok(Pattern::Unit)
                } else {
                    let pattern = self.pattern()?;
                    self.expect(Kind::RParen)?;
                    Ok(pattern)
                }
            }
            other => Err(self.error(&format!("expected pattern, found {other:?}"))),
        }
    }
    fn binary(&mut self, min: u8) -> Result<Expr, String> {
        let marker = self.mark();
        let mut left = self.application()?;
        let mut operators = 0;
        loop {
            // Keeping composition as two adjacent angle tokens also preserves
            // separate generic closing brackets in nested type annotations.
            let composition = match self.peek() {
                Kind::Gt if self.adjacent_pair(&Kind::Gt) => Some(FunctionOp::ComposeForward),
                Kind::Lt if self.adjacent_pair(&Kind::Lt) => Some(FunctionOp::ComposeBackward),
                _ => None,
            };
            let (precedence, binary, function) = match self.peek() {
                Kind::PipeForward => (1, None, Some(FunctionOp::PipeForward)),
                Kind::PipeBackward => (1, None, Some(FunctionOp::PipeBackward)),
                _ if composition.is_some() => (4, None, composition),
                Kind::Or => (2, Some(BinaryOp::Or), None),
                Kind::And => (3, Some(BinaryOp::And), None),
                Kind::EqEq => (5, Some(BinaryOp::Eq), None),
                Kind::Ne => (5, Some(BinaryOp::Ne), None),
                Kind::Lt => (6, Some(BinaryOp::Lt), None),
                Kind::Le => (6, Some(BinaryOp::Le), None),
                Kind::Gt => (6, Some(BinaryOp::Gt), None),
                Kind::Ge => (6, Some(BinaryOp::Ge), None),
                Kind::Plus => (7, Some(BinaryOp::Add), None),
                Kind::Minus => (7, Some(BinaryOp::Sub), None),
                Kind::Star => (8, Some(BinaryOp::Mul), None),
                Kind::Slash => (8, Some(BinaryOp::Div), None),
                Kind::Percent => (8, Some(BinaryOp::Rem), None),
                _ => break,
            };
            if precedence < min {
                break;
            }
            operators += 1;
            if operators > MAX_DEPTH {
                return Err(self.error("E-PARSE-DEPTH: operator chain exceeds 64"));
            }
            self.take();
            if composition.is_some() {
                self.take();
            }
            let next_precedence = if function == Some(FunctionOp::PipeBackward) {
                precedence
            } else {
                precedence + 1
            };
            // Right-associated backward application can recurse at the same
            // precedence, unlike the bounded precedence ladder above.
            if self.depth >= MAX_DEPTH {
                return Err(self.error("E-PARSE-DEPTH: syntax nesting exceeds 64"));
            }
            self.depth += 1;
            let right = self.binary(next_precedence);
            self.depth -= 1;
            let right = right?;
            let kind = if let Some(op) = binary {
                left = Expr::Binary(op, Box::new(left), Box::new(right));
                SyntaxKind::BinaryExpression
            } else {
                left = Expr::FunctionOp(function.unwrap(), Box::new(left), Box::new(right));
                SyntaxKind::FunctionOperatorExpression
            };
            self.finish(marker, kind);
        }
        Ok(left)
    }
    fn adjacent_pair(&self, kind: &Kind) -> bool {
        let first = &self.tokens[self.pos];
        self.tokens.get(self.pos + 1).is_some_and(|second| {
            second.kind == *kind && second.line == first.line && second.column == first.column + 1
        })
    }
    fn application(&mut self) -> Result<Expr, String> {
        let marker = self.mark();
        let callee = self.atom()?;
        let mut args = Vec::new();
        while !self.exported_item_start()
            && (matches!(
                self.peek(),
                Kind::Ident(_)
                    | Kind::Int(_)
                    | Kind::Str(_)
                    | Kind::CStr(_)
                    | Kind::True
                    | Kind::False
                    | Kind::LParen
                    | Kind::Move
                    | Kind::Record
            ) || (*self.peek() == Kind::Fn
                && self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == Kind::LParen)))
        {
            if args.len() == MAX_DEPTH {
                return Err(self.error("E-PARSE-DEPTH: application exceeds 64 arguments"));
            }
            let argument = self.mark();
            args.push(self.atom_with_postfix(true)?);
            self.finish(argument, SyntaxKind::Argument);
        }
        if args.is_empty() {
            return Ok(callee);
        }
        let result = if let Expr::Var(name) = callee {
            Expr::Call(name, args)
        } else {
            Expr::Apply(Box::new(callee), args)
        };
        self.finish(marker, SyntaxKind::ApplyExpression);
        Ok(result)
    }
    fn exported_item_start(&self) -> bool {
        self.module_mode
            && matches!(self.peek(), Kind::Ident(name) if name == "export")
            && self.tokens.get(self.pos + 1).is_some_and(|next| {
                matches!(next.kind, Kind::Let | Kind::Type | Kind::Extern)
                    || (next.kind == Kind::Fn
                        && self
                            .tokens
                            .get(self.pos + 2)
                            .is_some_and(|name| matches!(name.kind, Kind::Ident(_))))
            })
    }
    fn atom(&mut self) -> Result<Expr, String> {
        self.atom_with_postfix(false)
    }
    fn atom_with_postfix(&mut self, adjacent_only: bool) -> Result<Expr, String> {
        let marker = self.mark();
        let first = self.peek().clone();
        let unit = first == Kind::LParen
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == Kind::RParen);
        let mut result = self.guarded(if adjacent_only {
            Self::argument_atom_inner
        } else {
            Self::atom_inner
        });
        let kind = match (&first, &result) {
            (_, Err(_)) => SyntaxKind::Error,
            (Kind::LBrace, _) => SyntaxKind::Block,
            (Kind::LParen, _) if unit => SyntaxKind::UnitExpression,
            (Kind::LParen, _) => SyntaxKind::ParenthesizedExpression,
            (Kind::Minus | Kind::Not, _) => SyntaxKind::UnaryExpression,
            (Kind::Fn, _) => SyntaxKind::LambdaExpression,
            (_, Ok(Expr::Call(..))) => SyntaxKind::CallExpression,
            (_, Ok(Expr::Field(..))) => SyntaxKind::FieldExpression,
            (_, Ok(Expr::Var(..))) => SyntaxKind::NameExpression,
            (_, Ok(Expr::Record(..))) => SyntaxKind::RecordExpression,
            (_, Ok(Expr::Move(..))) => SyntaxKind::MoveExpression,
            _ => SyntaxKind::LiteralExpression,
        };
        if first != Kind::LBrace {
            self.finish(marker, kind);
        }
        let mut calls = 0;
        while result.is_ok()
            && *self.peek() == Kind::LParen
            && (!adjacent_only || self.adjacent[self.pos])
        {
            calls += 1;
            if calls > MAX_DEPTH {
                return Err(self.error("E-PARSE-DEPTH: application chain exceeds 64"));
            }
            let args = self.call_args()?;
            result = result.map(|callee| Expr::Apply(Box::new(callee), args));
            self.finish(marker, SyntaxKind::ApplyExpression);
        }
        result
    }
    fn atom_inner(&mut self) -> Result<Expr, String> {
        self.atom_inner_with_postfix(false)
    }
    fn argument_atom_inner(&mut self) -> Result<Expr, String> {
        self.atom_inner_with_postfix(true)
    }
    fn atom_inner_with_postfix(&mut self, adjacent_only: bool) -> Result<Expr, String> {
        if *self.peek() == Kind::LBrace {
            return self.block();
        }
        let start = self.pos;
        match self.take() {
            Kind::Fn => {
                let params = self.fn_params()?;
                let body = self.block()?;
                Ok(Expr::Lambda(params, Box::new(body)))
            }
            Kind::Move => Ok(Expr::Move(self.name()?)),
            Kind::Record => {
                let first = self.name()?;
                let name = self.qualified_name(first)?;
                self.expect(Kind::LBrace)?;
                let mut fields = Vec::new();
                while *self.peek() != Kind::RBrace {
                    let marker = self.mark();
                    let field = self.name()?;
                    self.expect(Kind::Equal)?;
                    fields.push((field, self.expr()?));
                    if *self.peek() == Kind::Semi {
                        self.take();
                    } else if *self.peek() != Kind::RBrace {
                        return Err(self.error("expected semicolon between record fields"));
                    }
                    self.finish(marker, SyntaxKind::RecordField);
                }
                self.expect(Kind::RBrace)?;
                Ok(Expr::Record(name, fields))
            }
            Kind::Str(value) => Ok(Expr::Str(value)),
            Kind::CStr(value) => Ok(Expr::CStr(value)),
            Kind::Int(n) => i32::try_from(n)
                .map(Expr::Int)
                .map_err(|_| self.error("integer literal out of i32 range")),
            Kind::True => Ok(Expr::Bool(true)),
            Kind::False => Ok(Expr::Bool(false)),
            Kind::Not => Ok(Expr::Binary(
                BinaryOp::Eq,
                Box::new(self.atom()?),
                Box::new(Expr::Bool(false)),
            )),
            Kind::Minus => {
                if *self.peek() == Kind::Int(2147483648) {
                    self.take();
                    Ok(Expr::Int(i32::MIN))
                } else {
                    Ok(Expr::Binary(
                        BinaryOp::Sub,
                        Box::new(Expr::Int(0)),
                        Box::new(self.atom()?),
                    ))
                }
            }
            Kind::Ident(name) => {
                let path = self.field_path()?;
                let name = if !path.is_empty() {
                    if self.module_mode
                        && self.module_aliases.contains(&name)
                        && path.len() == 1
                        && *self.peek() == Kind::LParen
                        && (!adjacent_only || self.adjacent[self.pos])
                    {
                        format!("{name}.{}", path[0])
                    } else {
                        return Ok(Expr::Field(name, path));
                    }
                } else {
                    name
                };
                if *self.peek() != Kind::LParen || (adjacent_only && !self.adjacent[self.pos]) {
                    return Ok(Expr::Var(name));
                }
                let args = self.call_args()?;
                Ok(Expr::Call(name, args))
            }
            Kind::LParen => {
                if *self.peek() == Kind::RParen {
                    self.take();
                    return Ok(Expr::Unit);
                }
                let e = self.expr()?;
                self.expect(Kind::RParen)?;
                Ok(e)
            }
            other => {
                let token = &self.tokens[start];
                Err(self.error_at(
                    start,
                    DiagnosticCode::Syntax,
                    format!(
                        "{}:{}: expected expression (found {other:?})",
                        token.line, token.column
                    ),
                ))
            }
        }
    }

    fn call_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(Kind::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != Kind::RParen {
            loop {
                let argument = self.mark();
                if *self.peek() == Kind::Inout {
                    self.take();
                    let root = self.name()?;
                    let path = self.field_path()?;
                    args.push(if path.is_empty() {
                        Expr::Inout(root)
                    } else {
                        Expr::InoutField(root, path)
                    });
                } else {
                    args.push(self.expr()?);
                }
                self.finish(argument, SyntaxKind::Argument);
                if *self.peek() != Kind::Comma {
                    break;
                }
                self.take();
            }
        }
        self.expect(Kind::RParen)?;
        Ok(args)
    }

    fn check_expr_depth(&self, expr: &Expr) -> Result<(), String> {
        let mut work = vec![(expr, 1)];
        while let Some((node, depth)) = work.pop() {
            if depth > MAX_DEPTH {
                return Err(self.error("E-PARSE-DEPTH: expression tree exceeds 64"));
            }
            let next = depth + 1;
            match node {
                Expr::Binary(_, a, b) | Expr::FunctionOp(_, a, b) | Expr::While(a, b) => {
                    work.push((a, next));
                    work.push((b, next));
                }
                Expr::Let(_, a, b) | Expr::LetMut(_, a, b) => {
                    work.push((a, next));
                    work.push((b, next));
                }
                Expr::If(a, b, c) => {
                    work.push((a, next));
                    work.push((b, next));
                    work.push((c, next));
                }
                Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => work.push((value, next)),
                Expr::Record(_, fields) => {
                    work.extend(fields.iter().map(|(_, value)| (value, next)))
                }
                Expr::Call(_, args) => work.extend(args.iter().map(|arg| (arg, next))),
                Expr::Apply(callee, args) => {
                    work.push((callee, next));
                    work.extend(args.iter().map(|arg| (arg, next)));
                }
                Expr::Lambda(_, body) => work.push((body, next)),
                Expr::Match(value, arms) => {
                    work.push((value, next));
                    work.extend(arms.iter().map(|arm| (&arm.body, next)));
                }
                Expr::Int(_)
                | Expr::Str(_)
                | Expr::CStr(_)
                | Expr::Bool(_)
                | Expr::Unit
                | Expr::Var(_)
                | Expr::Move(_)
                | Expr::Field(_, _)
                | Expr::InoutField(_, _)
                | Expr::Inout(_) => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_directives_are_stored_without_resolving_packages() {
        let source = r#"@include("SDL3/SDL.h"); @link("SDL3"); @pkg_config("nonexistent-rill-parser-test");
            extern const SDL_INIT_VIDEO: u32;
            extern fn SDL_CreateWindow(title: cstr, width: i32, height: i32, flags: u64) -> ptr<SDL_Window>;
            extern fn inspect(window: const_ptr<SDL_Window>, byte: u8, scale: f32) -> unit;
            fn main() { 0 }"#;
        let p = parse(source).unwrap();
        assert_eq!(p.native.includes, ["SDL3/SDL.h"]);
        assert_eq!(p.native.links, ["SDL3"]);
        assert_eq!(p.native.packages, ["nonexistent-rill-parser-test"]);
        assert_eq!(
            p.constants,
            [ExternConst {
                name: "SDL_INIT_VIDEO".into(),
                ty: ExternType::U32
            }]
        );
        assert_eq!(p.externs[0].params[3].ty, ExternType::U64);
        assert_eq!(
            p.externs[0].result,
            ExternType::Pointer {
                pointee: "SDL_Window".into(),
                is_const: false
            }
        );
        assert_eq!(
            p.externs[1].params[0].ty,
            ExternType::Pointer {
                pointee: "SDL_Window".into(),
                is_const: true
            }
        );
        assert_eq!(p.externs[1].params[1].ty, ExternType::U8);
        assert_eq!(p.externs[1].params[2].ty, ExternType::F32);
        let module = parse_module(
            r#"@link("SDL3"); import helper as h; @include("SDL3/SDL.h"); fn main() { 0 }"#,
        )
        .unwrap();
        assert_eq!(module.program.native.links, ["SDL3"]);
        assert_eq!(module.program.native.includes, ["SDL3/SDL.h"]);
    }
    #[test]
    fn native_syntax_rejects_malformed_directives_and_pointer_types() {
        for (source, location, message) in [
            (r#"@unknown("x");"#, "1:2:", "unknown native directive"),
            (r#"@link("x") fn main() {}"#, "1:12:", "expected ';'"),
            (r#"@link(c"x");"#, "1:7:", "ordinary string literal"),
            (r#"@link();"#, "1:7:", "ordinary string literal"),
            (r#"@link("a", "b");"#, "1:10:", "expected RParen"),
            (r#"@include("x>\n#error bad");"#, "1:10:", "invalid include"),
            (
                r#"fn main() {} @link("x");"#,
                "1:14:",
                "must precede declarations",
            ),
            ("extern const FLAG: u32", "1:23:", "expected Semi"),
            (
                "extern fn f(x: ptr<SDL_Window*>)->unit;",
                "1:30:",
                "expected Gt",
            ),
            (
                "extern fn f(x: ptr<SDL_Window.Other>)->unit;",
                "1:30:",
                "expected Gt",
            ),
        ] {
            let error = parse(source).unwrap_err();
            assert!(error.starts_with(location), "{error}");
            assert!(error.contains(message), "{error}");
        }
        assert!(parse("extern fn f(x: ptr<ptr<SDL_Window>>)->unit;").is_err());
        assert!(parse("extern const FLAG u32;").is_err());
    }
    #[test]
    fn precedence_and_calls() {
        let p = parse("let add x y = x + y\nlet main = add(1, 2 * 3)").unwrap();
        assert_eq!(p.functions.len(), 2);
        assert!(
            matches!(&p.functions[1].body, Expr::Call(name, args) if name == "add" && matches!(args[1], Expr::Binary(BinaryOp::Mul, _, _)))
        );
    }
    #[test]
    fn parses_module_exports_and_qualified_names_only_in_module_mode() {
        let src = "import helper as h;\nexport type Wrap<a> = Wrap(h.Option<a>)\nexport let get x = match x { h.Some(v) => h.id(v) | h.None => 0 }";
        let parsed = parse_module(src).unwrap();
        assert_eq!(
            parsed.exports,
            vec![
                Export {
                    name: "Wrap".into(),
                    kind: ExportKind::Type
                },
                Export {
                    name: "get".into(),
                    kind: ExportKind::Function
                },
            ]
        );
        assert_eq!(
            parsed.program.types[0].variants[0].fields[0].name,
            "h.Option"
        );
        assert!(
            matches!(&parsed.program.functions[0].body, Expr::Match(_, arms)
            if matches!(&arms[0].pattern, Pattern::Constructor(name, _) if name == "h.Some")
            && matches!(&arms[0].body, Expr::Call(name, _) if name == "h.id")
            && matches!(&arms[1].pattern, Pattern::Constructor(name, fields) if name == "h.None" && fields.is_empty()))
        );
        assert!(parse(src).is_err());
        assert!(matches!(
            &parse("let main = h.id(1)").unwrap().functions[0].body,
            Expr::Apply(callee, args) if matches!(callee.as_ref(), Expr::Field(root, path)
                if root == "h" && path == &["id"]) && args == &[Expr::Int(1)]
        ));
        let foreign =
            parse_module("export extern fn x() -> i32; export extern const FLAG: u32;").unwrap();
        assert_eq!(
            foreign.exports,
            vec![
                Export {
                    name: "x".into(),
                    kind: ExportKind::Function
                },
                Export {
                    name: "FLAG".into(),
                    kind: ExportKind::Constant
                },
            ]
        );
        assert!(parse("export extern fn x() -> i32;").is_err());
        assert!(parse_module("import h let main = 1").is_err());
    }
    #[test]
    fn legacy_bodies_preserve_following_exports_and_contextual_names() {
        let parsed = parse_module(
            "export let first value = let outcome = value in move outcome\n\
             export let second value = value\n\
             export fn third() { identity export }",
        )
        .unwrap();
        assert_eq!(
            parsed
                .exports
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
        assert_eq!(parsed.program.functions[1].body, Expr::Var("value".into()));
        assert_eq!(
            parsed.program.functions[2].body,
            Expr::Call("identity".into(), vec![Expr::Var("export".into())])
        );
    }
    #[test]
    fn lets_and_conditionals() {
        assert!(parse("let main = let x = 1 in if x == 1 then 2 else 3").is_ok());
    }
    #[test]
    fn parses_while_expression_and_sequence() {
        let p = parse("let main = let done = while false do () in 42").unwrap();
        assert!(matches!(&p.functions[0].body, Expr::Let(_, value, _) if
            matches!(value.as_ref(), Expr::While(condition, body) if **condition == Expr::Bool(false) && **body == Expr::Unit)));
        assert!(parse("let main = while true ()").is_err());
        assert!(parse("let main = while false do").is_err());
    }
    #[test]
    fn parses_mutable_locals_and_assignment() {
        let p = parse("let main = let mut x = 1 in let done = x <- x + 2 in x").unwrap();
        assert!(
            matches!(&p.functions[0].body, Expr::LetMut(name, init, body)
            if name == "x" && **init == Expr::Int(1)
                && matches!(body.as_ref(), Expr::Let(_, value, _)
                    if matches!(value.as_ref(), Expr::Assign(target, _) if target == "x")))
        );
        assert!(parse("let main = let mut x = 0 in while x < 2 do x <- x + 1").is_ok());
        assert!(parse("let main = (1 + 2) <- 3").is_err());
        assert!(parse("let mut x = 0").is_err());
    }
    #[test]
    fn parses_explicit_root_move() {
        let p = parse("let main = let x = 1 in let y = move x in y").unwrap();
        assert!(matches!(&p.functions[0].body, Expr::Let(_, _, body)
            if matches!(body.as_ref(), Expr::Let(_, value, _) if matches!(value.as_ref(), Expr::Move(name) if name == "x"))));
        assert!(parse("let main = move").is_err());
        assert!(parse("let main = move x.y").is_err());
        assert!(parse("let main = move (x)").is_err());
    }
    #[test]
    fn parses_nominal_records_fields_and_annotated_parameters() {
        let src = "type Point = { x: i32; y: i32 }\nlet set (inout p: Point) (n: i32) = p.x <- n\nlet main = let mut p = record Point { y = 2; x = 1 } in p.x";
        let p = parse(src).unwrap();
        assert_eq!(p.types[0].record_fields.as_ref().unwrap(), &["x", "y"]);
        assert_eq!(p.types[0].variants[0].fields[0].name, "i32");
        assert_eq!(p.functions[0].params[0].mode, ParamMode::Inout);
        assert_eq!(
            p.functions[0].params[0].annotation.as_ref().unwrap().name,
            "Point"
        );
        assert_eq!(p.functions[0].params[1].mode, ParamMode::Value);
        assert!(
            matches!(&p.functions[0].body, Expr::FieldAssign(root, field, _) if root == "p" && field.as_slice() == ["x"])
        );
        assert!(matches!(&p.functions[1].body, Expr::LetMut(_, value, body)
            if matches!(value.as_ref(), Expr::Record(name, fields) if name == "Point" && fields[0].0 == "y")
            && matches!(body.as_ref(), Expr::Field(root, field) if root == "p" && field.as_slice() == ["x"])));
        assert!(parse("type P = {x: i32 y: i32}").is_err());
        assert!(parse("let main = record P {x = 1 y = 2}").is_err());
        assert!(parse("let f (x) = x").is_err());
        assert!(parse("let main = let mut p = record P {x = 1} in p.x.y <- 2").is_ok());
    }
    #[test]
    fn parses_qualified_record_type_and_unresolved_field_access() {
        let p = parse_module("import shapes as s; let f (p: s.Point) = p.x\nlet main = s.None")
            .unwrap();
        assert_eq!(
            p.program.functions[0].params[0]
                .annotation
                .as_ref()
                .unwrap()
                .name,
            "s.Point"
        );
        assert!(
            matches!(&p.program.functions[0].body, Expr::Field(root,field) if root == "p" && field.as_slice() == ["x"])
        );
        assert!(
            matches!(&p.program.functions[1].body, Expr::Field(root,field) if root == "s" && field.as_slice() == ["None"])
        );
    }
    #[test]
    fn parses_bounded_field_inout_paths() {
        let p = parse("let main = f(inout item.x)").unwrap();
        assert!(matches!(&p.functions[0].body, Expr::Call(_, args)
            if matches!(&args[0], Expr::InoutField(root, field) if root == "item" && field.as_slice() == ["x"])));
        assert!(parse("let main = f(inout item.x.y)").is_ok());
        assert!(parse(&format!("let main = f(inout item{})", ".x".repeat(17))).is_err());
        assert!(parse(&format!("let main = item{}", ".x".repeat(17))).is_err());
        assert!(parse("let main = f(inout (item.x))").is_err());
    }
    #[test]
    fn parses_explicit_inout_roots() {
        let p = parse(
            "let bump (inout x) n = x <- x + n\nlet main = let mut x = 1 in bump(inout x, 2)",
        )
        .unwrap();
        assert_eq!(p.functions[0].params[0].mode, ParamMode::Inout);
        assert_eq!(p.functions[0].params[1].mode, ParamMode::Value);
        assert!(matches!(&p.functions[1].body, Expr::LetMut(_, _, body)
            if matches!(body.as_ref(), Expr::Call(_, args) if matches!(&args[0], Expr::Inout(x) if x == "x"))));
        assert!(parse("let f (inout x = x").is_err());
        assert!(parse("let main = inout x").is_err());
        assert!(parse("let main = f(inout 1)").is_err());
        assert!(parse("let main = f(inout x + 1)").is_err());
    }
    #[test]
    fn parses_read_parameters_and_ordinary_arguments() {
        let p = parse("let inspect (read x) (inout y) z = x\nlet main = inspect(a, inout b, c)")
            .unwrap();
        assert_eq!(p.functions[0].params[0].mode, ParamMode::Read);
        assert_eq!(p.functions[0].params[1].mode, ParamMode::Inout);
        assert_eq!(p.functions[0].params[2].mode, ParamMode::Value);
        assert!(matches!(&p.functions[1].body, Expr::Call(_, args)
            if matches!(&args[0], Expr::Var(name) if name == "a")));
        assert!(parse("let f (read) = 1").is_err());
        assert!(parse("let f (value x) = 1").is_err());
        assert!(parse("let main = f(read x)").is_err());
    }
    #[test]
    fn parses_explicit_c_scalar_declarations() {
        let p = parse("extern fn sum(x: i32, y: i32) -> i32; extern fn log(x: i32) -> unit; let main = sum(1, 2)").unwrap();
        assert_eq!(p.externs.len(), 2);
        assert_eq!(p.externs[0].params[1].ty, ExternType::I32);
        assert_eq!(p.externs[1].result, ExternType::Unit);
        assert!(parse("extern fn f(x: i64) -> i32; let main = 0").is_err());
        assert!(parse("extern fn f(x: i32) -> i32 let main = 0").is_err());
        assert!(parse("extern fn f(x: i32) -> i32; let main = 1 - 2").is_ok());
    }
    #[test]
    fn c_strings_decode_escapes_and_reject_nul() {
        let p = parse("extern fn use_text(s: cstr) -> unit; let main = use_text(c\"hi \\tκόσμος \\\"ok\\\"\")").unwrap();
        assert_eq!(p.externs[0].params[0].ty, ExternType::CStr);
        assert!(matches!(&p.functions[0].body, Expr::Call(_, args)
            if args[0] == Expr::CStr("hi \tκόσμος \"ok\"".into())));
        assert!(parse("let main = c\"abc\0def\"").is_err());
        assert!(parse("let main = c\"abc\\0def\"").is_err());
        assert!(parse("let main = c\"a\\qb\"").is_err());
        assert!(parse("let main = c\"line\nbreak\"").is_err());
        assert!(parse("let main = c\"unfinished").is_err());
    }
    #[test]
    fn static_strings_decode_utf8_and_embedded_nul() {
        let p = parse("let main = byte_at(\"é\\0a\\n\\\"\\\\\", 2)").unwrap();
        assert!(matches!(&p.functions[0].body, Expr::Call(name, args)
            if name == "byte_at" && args[0] == Expr::Str("é\0a\n\"\\".into())));
        assert!(parse("let main = \"\"").is_ok());
        assert!(parse("let main = \"a\\qb\"").is_err());
        assert!(parse("let main = \"abc").is_err());
        assert!(parse("let main = \"a\nb\"").is_err());
    }
    #[test]
    fn generic_types_and_match_patterns() {
        let program = parse(
            "type Option<a> = None | Some(a)\n\
            type Pair<a, b> = Pair(a, Option<b>)\n\
            let main = match Some(1) { Some(x) => x | None => 0 }",
        )
        .unwrap();
        assert_eq!(program.types[0].params, vec!["a"]);
        assert_eq!(program.types[1].variants[0].fields[1].args[0].name, "b");
        assert!(matches!(&program.functions[0].body, Expr::Match(_, arms)
            if matches!(&arms[0].pattern, Pattern::Constructor(n, fields)
                if n == "Some" && matches!(&fields[0], Pattern::Binding(x) if x == "x"))));
        let p = parse("let main = match 1 { -2147483648 => 1 | _ => 0 }").unwrap();
        assert!(matches!(&p.functions[0].body, Expr::Match(_, arms)
            if arms[0].pattern == Pattern::Int(i32::MIN)));
        assert!(parse("type T = C( ) |\nlet main = 1").is_err());
        assert!(parse("let main = match true { true => 1 | }").is_err());
    }
    #[test]
    fn rejects_invalid_input() {
        assert!(parse("let main = 1 +").is_err());
        assert!(parse("let main = @").unwrap_err().contains("1:12"));
        assert!(parse("let main = 2147483648").is_err());
        assert!(parse("let main = -2147483648").is_ok());
    }
}
