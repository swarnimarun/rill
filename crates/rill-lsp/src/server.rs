//! The document store and the protocol loop.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::json::Json;

/// One open document: its text and the analysis last computed for it.
pub struct Document {
    pub text: String,
    pub version: i64,
    pub source: rill_span::SourceMap,
    pub frontend: rill_ast::Frontend,
    pub hir: rill_hir::Hir,
    pub checked: rill_ty::Checked,
    pub diagnostics: rill_diagnostics::Diagnostics,
}

pub struct Server {
    documents: HashMap<String, Document>,
    shutdown: bool,
}

impl Server {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            shutdown: false,
        }
    }

    /// Reads messages until `exit`. Returns whether the stream ended cleanly.
    pub fn run(&mut self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let Some(message) = read_message(&mut reader)? else {
                return Ok(());
            };
            let Some(value) = crate::json::parse(&message) else {
                continue;
            };
            if self.handle(&value)? {
                return Ok(());
            }
        }
    }

    /// Handles one message. Returns whether the server should stop.
    fn handle(&mut self, message: &Json) -> std::io::Result<bool> {
        let method = message.get("method").and_then(Json::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Json::Null);
        match method {
            "initialize" => {
                self.respond(
                    id,
                    Json::obj(&[
                        ("capabilities", capabilities()),
                        (
                            "serverInfo",
                            Json::obj(&[
                                ("name", Json::Str("rill-lsp".into())),
                                ("version", Json::Str("0.1.0".into())),
                            ]),
                        ),
                    ]),
                )?;
            }
            "initialized" => {}
            "shutdown" => {
                self.shutdown = true;
                self.respond(id, Json::Null)?;
            }
            "exit" => return Ok(true),
            "textDocument/didOpen" => {
                let Some(document) = params.get("textDocument") else {
                    return Ok(false);
                };
                let uri = document
                    .get("uri")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                let text = document
                    .get("text")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                let version = document.get("version").and_then(Json::as_i64).unwrap_or(0);
                self.open(uri, text, version)?;
            }
            "textDocument/didChange" => {
                let Some(document) = params.get("textDocument") else {
                    return Ok(false);
                };
                let uri = document
                    .get("uri")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                let version = document.get("version").and_then(Json::as_i64).unwrap_or(0);
                // Full sync: the change carries the whole text, which keeps the
                // server honest without an incremental tree.
                let text = params
                    .get("contentChanges")
                    .and_then(|changes| match changes {
                        Json::Arr(items) => items.last(),
                        _ => None,
                    })
                    .and_then(|change| change.get("text"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                self.open(uri, text, version)?;
            }
            "textDocument/hover" => {
                let line = params
                    .get("position")
                    .and_then(|p| p.get("line"))
                    .and_then(Json::as_i64)
                    .unwrap_or(0) as u32;
                let character = params
                    .get("position")
                    .and_then(|p| p.get("character"))
                    .and_then(Json::as_i64)
                    .unwrap_or(0) as u32;
                let uri = params
                    .get("textDocument")
                    .and_then(|doc| doc.get("uri"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                if debug() {
                    eprintln!("lsp: hover {uri} at {line}:{character}");
                }
                self.hover(id, &uri, line, character)?;
            }
            "textDocument/semanticTokens/full" => {
                let uri = params
                    .get("textDocument")
                    .and_then(|doc| doc.get("uri"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                self.semantic_tokens(id, &uri);
            }
            "textDocument/references" => {
                let line = params
                    .get("position")
                    .and_then(|p| p.get("line"))
                    .and_then(Json::as_i64)
                    .unwrap_or(0) as u32;
                let character = params
                    .get("position")
                    .and_then(|p| p.get("character"))
                    .and_then(Json::as_i64)
                    .unwrap_or(0) as u32;
                let uri = params
                    .get("textDocument")
                    .and_then(|doc| doc.get("uri"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                self.references(id, &uri, line, character);
            }
            "textDocument/formatting" => {
                let uri = params
                    .get("textDocument")
                    .and_then(|doc| doc.get("uri"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                self.format(id, &uri)?;
            }
            _ => {
                if id.is_some() {
                    self.respond(id, Json::Null)?;
                }
            }
        }
        Ok(false)
    }

    fn open(&mut self, uri: String, text: String, version: i64) -> std::io::Result<()> {
        // The same pipeline a build runs: parse and lower, resolve, check.
        let (frontend, source) = rill_ast::parse_and_lower_text(&uri, &text);
        let hir = rill_hir::lower(&frontend);
        let checked = rill_ty::check(&frontend, &hir);
        let mut diagnostics = rill_diagnostics::Diagnostics::default();
        diagnostics.extend(frontend.diagnostics.clone());
        diagnostics.extend(checked.diagnostics.clone());
        if debug() {
            eprintln!("lsp: analysed {uri} ({} diagnostics)", diagnostics.len());
        }
        let document = Document {
            text,
            version,
            source,
            frontend,
            hir,
            checked,
            diagnostics,
        };
        self.documents.insert(uri.clone(), document);
        self.publish(&uri)
    }

    fn format(&mut self, id: Option<Json>, uri: &str) -> std::io::Result<()> {
        let Some(document) = self.documents.get(uri) else {
            return self.respond(id, Json::Null);
        };
        let formatted = rill_fmt::format_text(&document.text);
        // LSP wants edits; replacing the whole document with one edit is valid and
        // is what a formatter that rewrites line breaks should do.
        let end_line = document.text.lines().count() as i64;
        let edit = Json::obj(&[
            ("range", range(0, 0, end_line + 1, 0)),
            ("newText", Json::Str(formatted)),
        ]);
        self.respond(id, Json::Arr(vec![edit]))
    }

    /// Hover: the type of the smallest expression containing the cursor.
    fn hover(
        &mut self,
        id: Option<Json>,
        uri: &str,
        line: u32,
        character: u32,
    ) -> std::io::Result<()> {
        let Some(document) = self.documents.get(uri) else {
            if debug() {
                eprintln!("lsp: hover for an unknown document");
            }
            return self.respond(id, Json::Null);
        };
        let offset = position_to_offset(&document.source, line, character);
        if debug() {
            let items = document.hir.items().len();
            let bodies = document
                .hir
                .items()
                .iter()
                .filter(|item| item_body(item).is_some())
                .count();
            eprintln!(
                "lsp: hover offset={} items={items} with-body={bodies} files={}",
                offset.to_usize(),
                document.source.files().len()
            );
        }
        let mut best: Option<(u32, String)> = None;
        let mut considered = 0usize;
        for item in document.hir.items() {
            let mut exprs = Vec::new();
            if let Some(body) = item_body(item) {
                collect_expr(body, &mut exprs);
            }
            for expr in exprs {
                let span = expr.span;
                if span.is_dummy() || span.is_empty() {
                    continue;
                }
                if span.start <= offset && offset <= span.end {
                    considered += 1;
                    let Some(ty) = document.checked.expr_ty(expr.id) else {
                        continue;
                    };
                    let text = document.checked.display_ty(&document.frontend, ty);
                    let width = span.end.to_usize().saturating_sub(span.start.to_usize()) as u32;
                    if best.as_ref().map(|(best, _)| width < *best).unwrap_or(true) {
                        best = Some((width, text));
                    }
                }
            }
        }
        if debug() {
            eprintln!("lsp: hover considered {considered} expressions, answer: {best:?}");
        }
        match best {
            Some((_, text)) => self.respond(
                id,
                Json::obj(&[(
                    "contents",
                    Json::obj(&[
                        ("kind", Json::Str("plaintext".into())),
                        ("value", Json::Str(text)),
                    ]),
                )]),
            ),
            None => self.respond(id, Json::Null),
        }
    }

    /// References: every expression that resolves to the same thing as the one
    /// under the cursor. Resolution is the linker's answer, so this is exact rather
    /// than a name match - a shadowed local does not collect the outer one's uses.
    fn references(&mut self, id: Option<Json>, uri: &str, line: u32, character: u32) {
        let mut locations = Vec::new();
        if let Some(document) = self.documents.get(uri) {
            let offset = position_to_offset(&document.source, line, character);
            let at = expr_at(document, offset);
            let wanted = at.as_ref().and_then(|expr| match &expr.kind {
                rill_hir::ExprKind::Path { res, .. } => Some(res.clone()),
                // A call's callee often carries an empty span, so the resolution is
                // taken from the call when that is the innermost expression.
                rill_hir::ExprKind::Call { callee, .. } => {
                    callee.path_res().map(|(_, res)| res.clone())
                }
                _ => None,
            });
            if debug() {
                eprintln!(
                    "lsp: references at {}:{} -> {wanted:?} (expr span {:?})",
                    line,
                    character,
                    at.as_ref()
                        .map(|expr| (expr.span.start.to_usize(), expr.span.end.to_usize()))
                );
            }
            if let Some(wanted) = wanted {
                for item in document.hir.items() {
                    let mut exprs = Vec::new();
                    if let Some(body) = item_body(item) {
                        collect_expr(body, &mut exprs);
                    }
                    for expr in exprs {
                        let rill_hir::ExprKind::Path { res, .. } = &expr.kind else {
                            continue;
                        };
                        if *res != wanted || expr.span.is_dummy() {
                            continue;
                        }
                        let start = document.source.line_col(expr.span.start);
                        let end = document.source.line_col(expr.span.end);
                        locations.push(Json::obj(&[
                            ("uri", Json::Str(uri.to_string())),
                            (
                                "range",
                                range(
                                    start.line as i64,
                                    start.col as i64,
                                    end.line as i64,
                                    end.col as i64,
                                ),
                            ),
                        ]));
                    }
                }
            }
        }
        let _ = self.respond(id, Json::Arr(locations));
    }

    /// Semantic tokens: what the token stream itself knows - keywords, literals,
    /// comments and operators - with the same legend the handshake declared.
    ///
    /// Names are deliberately *not* classified: telling a type from a function from a
    /// local needs the resolver, and the tree the server holds here is the concrete
    /// one. A wrong colour is worse than a plain one, and names would be wrong
    /// whenever a local shadows an item, which is exactly when a reader wants the
    /// colour to be right.
    fn semantic_tokens(&mut self, id: Option<Json>, uri: &str) {
        let mut data: Vec<i64> = Vec::new();
        if let Some(document) = self.documents.get(uri) {
            let (parse, _map) = rill_parser::parse_text(uri, &document.text);
            let mut tokens = Vec::new();
            collect_tokens(&parse.cst.root, &mut tokens);
            let mut previous_line = 0i64;
            let mut previous_start = 0i64;
            let mut marked: Vec<(i64, i64, i64, i64)> = Vec::new();
            for (span, kind) in tokens {
                let Some(index) = token_type(kind) else {
                    continue;
                };
                let start = document.source.line_col(span.start);
                let end = document.source.line_col(span.end);
                if start.line != end.line {
                    continue;
                }
                let line = start.line as i64;
                let character = start.col as i64;
                let length = (end.col as i64 - character).max(0);
                if length == 0 {
                    continue;
                }
                marked.push((line, character, length, index));
            }
            marked.sort();
            for (line, character, length, index) in marked {
                let delta_line = line - previous_line;
                let delta_start = if delta_line == 0 {
                    character - previous_start
                } else {
                    character
                };
                data.extend([delta_line, delta_start, length, index, 0]);
                previous_line = line;
                previous_start = character;
            }
        }
        self.respond(
            id,
            Json::obj(&[(
                "data",
                Json::Arr(data.into_iter().map(|v| Json::Num(v as f64)).collect()),
            )]),
        )
        .ok();
    }

    fn publish(&mut self, uri: &str) -> std::io::Result<()> {
        let Some(document) = self.documents.get(uri) else {
            return Ok(());
        };
        let items: Vec<Json> = document
            .diagnostics
            .items()
            .iter()
            .map(|diagnostic| {
                // A diagnostic's first label carries the span an editor underlines.
                let span = diagnostic
                    .labels
                    .first()
                    .map(|label| label.span)
                    .unwrap_or(rill_span::Span::DUMMY);
                let start_pos = document.source.line_col(span.start);
                let end_pos = document.source.line_col(span.end);
                let (start, end) = (
                    (start_pos.line as i64, start_pos.col as i64),
                    (end_pos.line as i64, end_pos.col as i64),
                );
                Json::obj(&[
                    ("range", range(start.0, start.1, end.0, end.1)),
                    (
                        "severity",
                        Json::Num(match diagnostic.severity {
                            rill_diagnostics::Severity::Error => 1.0,
                            rill_diagnostics::Severity::Warning => 2.0,
                            _ => 3.0,
                        }),
                    ),
                    ("source", Json::Str("rill".into())),
                    ("message", Json::Str(diagnostic.message.clone())),
                ])
            })
            .collect();
        self.notify(
            "textDocument/publishDiagnostics",
            Json::obj(&[
                ("uri", Json::Str(uri.to_string())),
                // The version is what lets a client discard diagnostics for a
                // stale edit, which is why the protocol asks for it.
                ("version", Json::Num(document.version as f64)),
                ("diagnostics", Json::Arr(items)),
            ]),
        )
    }

    fn respond(&mut self, id: Option<Json>, result: Json) -> std::io::Result<()> {
        let message = Json::obj(&[
            ("jsonrpc", Json::Str("2.0".into())),
            ("id", id.unwrap_or(Json::Null)),
            ("result", result),
        ]);
        write_message(&message.render())
    }

    fn notify(&mut self, method: &str, params: Json) -> std::io::Result<()> {
        let message = Json::obj(&[
            ("jsonrpc", Json::Str("2.0".into())),
            ("method", Json::Str(method.to_string())),
            ("params", params),
        ]);
        write_message(&message.render())
    }
}

fn range(start_line: i64, start_col: i64, end_line: i64, end_col: i64) -> Json {
    Json::obj(&[
        (
            "start",
            Json::obj(&[
                ("line", Json::Num(start_line as f64)),
                ("character", Json::Num(start_col as f64)),
            ]),
        ),
        (
            "end",
            Json::obj(&[
                ("line", Json::Num(end_line as f64)),
                ("character", Json::Num(end_col as f64)),
            ]),
        ),
    ])
}

/// The token types the legend declares, in the order `token_type` indexes them.
fn token_legend() -> Vec<Json> {
    ["keyword", "number", "string", "comment", "operator"]
        .iter()
        .map(|name| Json::Str(name.to_string()))
        .collect()
}

fn token_type(kind: rill_syntax::SyntaxKind) -> Option<i64> {
    use rill_syntax::SyntaxKind;
    Some(match kind {
        SyntaxKind::IntLiteral | SyntaxKind::FloatLiteral => 1,
        SyntaxKind::Text | SyntaxKind::RawText | SyntaxKind::CharLiteral => 2,
        SyntaxKind::LineComment | SyntaxKind::BlockComment => 3,
        SyntaxKind::Plus
        | SyntaxKind::Minus
        | SyntaxKind::Star
        | SyntaxKind::Slash
        | SyntaxKind::Percent
        | SyntaxKind::Assign
        | SyntaxKind::EqEq
        | SyntaxKind::BangEq
        | SyntaxKind::Lt
        | SyntaxKind::Gt
        | SyntaxKind::LtEq
        | SyntaxKind::GtEq
        | SyntaxKind::Arrow
        | SyntaxKind::PlusPlus
        | SyntaxKind::ColonEq => 4,
        _ => {
            // Everything else is a keyword only if the syntax kind says so, which is
            // what `is_keyword` answers.
            if is_keyword(kind) {
                0
            } else {
                return None;
            }
        }
    })
}

fn is_keyword(kind: rill_syntax::SyntaxKind) -> bool {
    // The syntax kind has the answer; prefixing the debug name would guess at it.
    kind.is_keyword()
}

fn collect_tokens(
    node: &rill_syntax::SyntaxNode,
    out: &mut Vec<(rill_span::Span, rill_syntax::SyntaxKind)>,
) {
    for child in &node.children {
        match child {
            rill_syntax::Element::Node(node) => collect_tokens(node, out),
            rill_syntax::Element::Token(token) => {
                if token.kind != rill_syntax::SyntaxKind::Whitespace {
                    out.push((token.span, token.kind));
                }
            }
        }
    }
}

fn capabilities() -> Json {
    Json::obj(&[
        ("textDocumentSync", Json::Num(1.0)),
        ("documentFormattingProvider", Json::Bool(true)),
        (
            "semanticTokensProvider",
            Json::obj(&[
                (
                    "legend",
                    Json::obj(&[
                        ("tokenTypes", Json::Arr(token_legend())),
                        ("tokenModifiers", Json::Arr(Vec::new())),
                    ]),
                ),
                ("full", Json::Bool(true)),
            ]),
        ),
    ])
}

fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let Some(length) = length else {
        return Ok(None);
    };
    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer)?;
    Ok(Some(String::from_utf8_lossy(&buffer).to_string()))
}

fn write_message(body: &str) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// The smallest expression containing an offset, which is what a position query
/// means in practice.
fn expr_at(document: &Document, offset: rill_span::BytePos) -> Option<rill_hir::Expr> {
    let mut best: Option<(usize, rill_hir::Expr)> = None;
    for item in document.hir.items() {
        let mut exprs = Vec::new();
        if let Some(body) = item_body(item) {
            collect_expr(body, &mut exprs);
        }
        for expr in exprs {
            let span = expr.span;
            if span.is_dummy() || span.is_empty() {
                continue;
            }
            if span.start <= offset && offset <= span.end {
                let width = span.end.to_usize().saturating_sub(span.start.to_usize());
                if best.as_ref().map(|(best, _)| width < *best).unwrap_or(true) {
                    best = Some((width, expr));
                }
            }
        }
    }
    best.map(|(_, expr)| expr)
}

/// Whether the server prints what it is doing, which is how a request that answers
/// nothing gets diagnosed instead of guessed at.
fn debug() -> bool {
    std::env::var_os("RILL_DBG_LSP").is_some()
}

/// An item's body expression, for tools that walk program text.
fn item_body(item: &rill_hir::Item) -> Option<&rill_hir::Expr> {
    match &item.kind {
        rill_hir::ItemKind::Fn(def) => def.body.as_ref(),
        rill_hir::ItemKind::Project(def) => Some(&def.body),
        _ => None,
    }
}

/// Every expression under `expr`, outermost first. The walk covers the shapes that
/// carry types in practice; a shape it does not reach yields no answer rather than a
/// wrong one.
fn collect_expr(expr: &rill_hir::Expr, out: &mut Vec<rill_hir::Expr>) {
    out.push(expr.clone());
    match &expr.kind {
        rill_hir::ExprKind::Block(block) => {
            for stmt in &block.stmts {
                match stmt {
                    rill_hir::Stmt::Expr(expr) => collect_expr(expr, out),
                    rill_hir::Stmt::Set { place, value, .. } => {
                        collect_expr(place, out);
                        collect_expr(value, out);
                    }
                    rill_hir::Stmt::Var { init, .. } => collect_expr(init, out),
                    rill_hir::Stmt::Yield { value, .. } => collect_expr(value, out),
                    rill_hir::Stmt::TryBind { value, .. } => collect_expr(value, out),
                    _ => {}
                }
            }
            if let Some(tail) = &block.tail {
                collect_expr(tail, out);
            }
        }
        rill_hir::ExprKind::Call { callee, args } => {
            collect_expr(callee, out);
            for arg in args {
                collect_expr(&arg.value, out);
            }
        }
        rill_hir::ExprKind::Index { base, args } => {
            collect_expr(base, out);
            for arg in args {
                collect_expr(&arg.value, out);
            }
        }
        rill_hir::ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, out);
            collect_expr(rhs, out);
        }
        rill_hir::ExprKind::Unary { operand, .. } => collect_expr(operand, out),
        rill_hir::ExprKind::Try { base } => collect_expr(base, out),
        rill_hir::ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_expr_block(then_branch, out);
            if let Some(rill_hir::ElseBranch::Block(block)) = else_branch {
                collect_expr_block(block, out);
            }
        }
        _ => {}
    }
}

fn collect_expr_block(block: &rill_hir::Block, out: &mut Vec<rill_hir::Expr>) {
    for stmt in &block.stmts {
        if let rill_hir::Stmt::Expr(expr) = stmt {
            collect_expr(expr, out);
        }
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

/// `(line, character)` as an absolute offset. LSP counts lines from zero; these
/// sources are ASCII in practice, so a byte column is the same number, and the
/// difference is documented rather than assumed away.
fn position_to_offset(
    source: &rill_span::SourceMap,
    line: u32,
    character: u32,
) -> rill_span::BytePos {
    let Some(file) = source.files().first() else {
        return rill_span::BytePos(0);
    };
    let mut offset = file.start.to_usize();
    for (index, text) in file.text.split_inclusive('\n').enumerate() {
        if index as u32 == line {
            let at = (offset + character as usize).min(file.text.len());
            return rill_span::BytePos(at as u32);
        }
        offset += text.len();
    }
    rill_span::BytePos(file.text.len() as u32)
}
