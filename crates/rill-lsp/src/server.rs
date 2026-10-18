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
            "textDocument/inlayHint" => {
                let uri = params
                    .get("textDocument")
                    .and_then(|doc| doc.get("uri"))
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                let start = params
                    .get("range")
                    .and_then(|r| r.get("start"))
                    .and_then(|s| s.get("line"))
                    .and_then(Json::as_i64)
                    .unwrap_or(0);
                let end = params
                    .get("range")
                    .and_then(|r| r.get("end"))
                    .and_then(|s| s.get("line"))
                    .and_then(Json::as_i64)
                    .unwrap_or(i64::MAX);
                self.inlay_hints(id, &uri, start as u32, end as u32);
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
        // The same pipeline a build runs: parse and lower, resolve, check. The
        // document's own directory is searched first for `@include` headers.
        let (mut frontend, source) = rill_ast::parse_and_lower_text(&uri, &text);
        let mut options = rill_hir::Options::default();
        if let Some(dir) = uri
            .strip_prefix("file://")
            .and_then(|path| std::path::Path::new(path).parent())
        {
            options.include_dirs.push(dir.to_path_buf());
        }
        let hir = rill_hir::lower_with(&mut frontend, &options);
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
        // A local's name is not an expression, so containment would fall through to
        // the enclosing block and answer `()` - the bug this replaces. Names are
        // answered from the locals table, which is where `var x = 1;` keeps `x: i32`.
        // The initializer's effects ride along, so the hover says exactly what the
        // inlay hint at the same declaration says.
        let initializers = local_initializers(&document.hir);
        if let Some((local, ty)) = document
            .hir
            .locals()
            .iter()
            .find(|local| {
                local.owner.is_some() && local.span.start <= offset && offset <= local.span.end
            })
            .and_then(|local| {
                document
                    .checked
                    .local_ty(local.id)
                    .map(|ty| (local.id, ty))
            })
        {
            let text = type_with_effects(
                &document.checked,
                &document.frontend,
                ty,
                initializers
                    .get(&local)
                    .map(|init| document.checked.expr_effects(*init))
                    .unwrap_or_default(),
            );
            return self.respond(
                id,
                Json::obj(&[(
                    "contents",
                    Json::obj(&[
                        ("kind", Json::Str("plaintext".into())),
                        ("value", Json::Str(text)),
                    ]),
                )]),
            );
        }
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
                    let performed = document.checked.expr_effects(expr.id);
                    let text = type_with_effects(
                        &document.checked,
                        &document.frontend,
                        ty,
                        performed,
                    );
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
                                    start.line as i64 - 1,
                                    start.col as i64 - 1,
                                    end.line as i64 - 1,
                                    end.col as i64 - 1,
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
                // `line_col` is 1-based; semantic tokens count from zero.
                let line = start.line as i64 - 1;
                let character = start.col as i64 - 1;
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

    /// Inlay hints: the type of every local at its declaration.
    ///
    /// Not advertised yet, and that is deliberate: `LocalInfo::span` is the whole
    /// statement rather than the name, so the hints land at the end of a declaration
    /// instead of after the binding it describes. A cross-check against hover on all
    /// 30 examples is what showed it. The position has to come from the name token in
    /// the concrete tree before a client should see these.
    fn inlay_hints(&mut self, id: Option<Json>, uri: &str, from_line: u32, to_line: u32) {
        let mut hints = Vec::new();
        if let Some(document) = self.documents.get(uri) {
            // A hint says what the source does not. A parameter is declared with its
            // type (`x: i32`), and so is any binding that writes one (`var x: i32 =
            // ...`), so those are left out; the type is already on the screen.
            let annotated = annotated_locals(&document.hir);
            let initializers = local_initializers(&document.hir);
            for local in document.hir.locals() {
                if local.owner.is_none() || local.span.is_dummy() {
                    continue;
                }
                if local.kind == rill_hir::LocalKind::Param || annotated.contains(&local.id) {
                    continue;
                }
                // The hint goes right after the binding's name. `LocalInfo::span` is
                // the identifier, set by the resolver where the local is declared -
                // so it is the position, with no scanning of the tree for a
                // neighbouring name (which could pick a different identifier).
                if debug() {
                    let snippet = document.source.snippet(local.span);
                    let name = document.frontend.text(rill_ast::Ident::new(local.name, local.span));
                    eprintln!(
                        "lsp: local `{name}` kind {:?} span {}..{} covers {snippet:?}",
                        local.kind,
                        local.span.start.to_usize(),
                        local.span.end.to_usize()
                    );
                }
                // Only trust the span when the source it covers *is* the name.
                // Some locals (parameters among them) resolve to the span of a *use*
                // rather than their declaration, and a hint at that position is worse
                // than no hint: it lands mid-expression and describes another binding.
                let name = document.frontend.text(rill_ast::Ident::new(local.name, local.span));
                let covered = document.source.snippet(local.span);
                if covered != name {
                    if debug() {
                        eprintln!(
                            "lsp: skipping hint for `{name}`: span covers {covered:?}"
                        );
                    }
                    continue;
                }
                let at = document.source.line_col(local.span.end);
                if debug() {
                    eprintln!(
                        "lsp: hint for local `{}` at {}:{} (span {}..{})",
                        document.frontend.text(rill_ast::Ident::new(local.name, local.span)),
                        at.line,
                        at.col,
                        local.span.start.to_usize(),
                        local.span.end.to_usize()
                    );
                }
                // `at` is 1-based, the request's range is not.
                if at.line.saturating_sub(1) < from_line || at.line.saturating_sub(1) > to_line {

                    continue;
                }
                let Some(ty) = document.checked.local_ty(local.id) else {
                    continue;
                };
                if matches!(ty, rill_ty::Ty::Unit | rill_ty::Ty::Error) {
                    continue;
                }
                let text = type_with_effects(
                    &document.checked,
                    &document.frontend,
                    ty,
                    initializers
                        .get(&local.id)
                        .map(|init| document.checked.expr_effects(*init))
                        .unwrap_or_default(),
                );
                hints.push(Json::obj(&[
                    (
                        "position",
                        Json::obj(&[
                            ("line", Json::Num(at.line as f64 - 1.0)),
                            ("character", Json::Num(at.col as f64 - 1.0)),
                        ]),
                    ),
                    ("label", Json::Str(format!(": {text}"))),
                    ("kind", Json::Num(1.0)),
                    ("paddingLeft", Json::Bool(true)),
                ]));
            }
        }
        let _ = self.respond(id, Json::Arr(hints));
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
                // `line_col` is 1-based; LSP counts from zero.
                let (start, end) = (
                    (start_pos.line as i64 - 1, start_pos.col as i64 - 1),
                    (end_pos.line as i64 - 1, end_pos.col as i64 - 1),
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
        // Advertised because an editor only asks for what the server claims: the
        // handlers existed and were tested while no client could reach them, since
        // hover and references were never declared here.
        ("hoverProvider", Json::Bool(true)),
        ("referencesProvider", Json::Bool(true)),
        ("inlayHintProvider", Json::Bool(true)),
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

/// Every expression under `expr`, outermost first. Every shape is walked: an
/// expression the tool cannot reach is an expression hover cannot answer for.
fn collect_expr(expr: &rill_hir::Expr, out: &mut Vec<rill_hir::Expr>) {
    out.push(expr.clone());
    match &expr.kind {
        rill_hir::ExprKind::Block(block) => collect_block(block, out),
        rill_hir::ExprKind::Lambda { body, .. } => collect_block(body, out),
        rill_hir::ExprKind::Tuple(elems) | rill_hir::ExprKind::Array(elems) => {
            for elem in elems {
                collect_expr(elem, out);
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
        rill_hir::ExprKind::Cast { expr, .. } | rill_hir::ExprKind::Is { expr, .. } => {
            collect_expr(expr, out)
        }
        rill_hir::ExprKind::Match { scrutinee, arms } => {
            collect_expr(scrutinee, out);
            for arm in arms {
                collect_expr(&arm.body, out);
                if let Some(guard) = &arm.guard {
                    collect_expr(guard, out);
                }
            }
        }
        rill_hir::ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_cond(cond, out);
            collect_block(then_branch, out);
            match else_branch {
                Some(rill_hir::ElseBranch::Block(block)) => collect_block(block, out),
                Some(rill_hir::ElseBranch::If(expr)) => collect_expr(expr, out),
                None => {}
            }
        }
        rill_hir::ExprKind::Comptime { body }
        | rill_hir::ExprKind::Unsafe { body }
        | rill_hir::ExprKind::Handler { body, .. } => collect_block(body, out),
        rill_hir::ExprKind::Concurrent { arms, .. } => {
            for arm in arms {
                collect_expr(arm, out);
            }
        }
        rill_hir::ExprKind::Literal { .. } | rill_hir::ExprKind::Path { .. } | rill_hir::ExprKind::Error => {}
    }
}

/// The expressions inside a statement list and its tail.
fn collect_block(block: &rill_hir::Block, out: &mut Vec<rill_hir::Expr>) {
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
            rill_hir::Stmt::Bind { value, .. } => collect_expr(value, out),
            rill_hir::Stmt::When { cond, .. } => collect_cond(cond, out),
            rill_hir::Stmt::For { iter, body, .. } => {
                collect_expr(iter, out);
                collect_block(body, out);
            }
            rill_hir::Stmt::While { cond, body, .. } => {
                collect_expr(cond, out);
                collect_block(body, out);
            }
            rill_hir::Stmt::Let { .. }
            | rill_hir::Stmt::Break { .. }
            | rill_hir::Stmt::Error(_) => {}
        }
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

fn collect_cond(cond: &rill_hir::Cond, out: &mut Vec<rill_hir::Expr>) {
    match cond {
        rill_hir::Cond::Expr(expr) => collect_expr(expr, out),
        rill_hir::Cond::Bind { value, .. } => collect_expr(value, out),
    }
}

/// The expression that initialises a local, when its binding wrote one. Inlay
/// hints show the effects of that expression next to the binding's type, which
/// is where a reader looks for what the binding costs.
fn local_initializers(hir: &rill_hir::Hir) -> HashMap<rill_hir::LocalId, rill_hir::ExprId> {
    let mut found = HashMap::new();
    for item in hir.items() {
        let Some(body) = item_body(item) else {
            continue;
        };
        let mut exprs = Vec::new();
        collect_expr(body, &mut exprs);
        for expr in &exprs {
            let rill_hir::ExprKind::Block(block) = &expr.kind else {
                continue;
            };
            for stmt in &block.stmts {
                match stmt {
                    rill_hir::Stmt::Var { local, init, .. } => {
                        found.insert(*local, init.id);
                    }
                    rill_hir::Stmt::Bind {
                        pattern: rill_hir::Pattern::Binding { local, .. },
                        value,
                        ..
                    }
                    | rill_hir::Stmt::TryBind { local, value, .. } => {
                        found.insert(*local, value.id);
                    }
                    _ => {}
                }
            }
        }
    }
    found
}

/// A type plus the effects evaluating the expression performs. A function type's
/// own row already answers what calling it does, so performed effects are only
/// appended when they say something the row does not.
fn type_with_effects(
    checked: &rill_ty::Checked,
    frontend: &rill_ast::Frontend,
    ty: &rill_ty::Ty,
    effects: &[rill_ty::Effect],
) -> String {
    let mut text = checked.display_ty(frontend, ty);
    let covered = match ty {
        rill_ty::Ty::Fn { effects: row, .. } => effects.iter().all(|effect| row.contains(effect)),
        _ => effects.is_empty(),
    };
    if !covered {
        text.push_str(&format!(
            " <{}>",
            checked.display_effects(frontend, effects)
        ));
    }
    text
}

/// Locals whose declaration writes a type, so a hint would only repeat the source.
///
/// The walk reuses `collect_expr`: a block *is* an expression, so collecting every
/// expression reaches every statement list, and the annotations are read off those
/// statements. Nothing is inferred from the text.
fn annotated_locals(hir: &rill_hir::Hir) -> Vec<rill_hir::LocalId> {
    let mut found = Vec::new();
    for item in hir.items() {
        let Some(body) = item_body(item) else {
            continue;
        };
        let mut exprs = Vec::new();
        collect_expr(body, &mut exprs);
        for expr in &exprs {
            let rill_hir::ExprKind::Block(block) = &expr.kind else {
                continue;
            };
            for stmt in &block.stmts {
                if let rill_hir::Stmt::Var {
                    local, ty: Some(_), ..
                } = stmt
                {
                    found.push(*local);
                }
            }
        }
    }
    found
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
        // LSP lines are zero-based; this scan's index is too, so it matches directly.
        if index as u32 == line {
            let at = (offset + character as usize).min(file.text.len());
            return rill_span::BytePos(at as u32);
        }
        offset += text.len();
    }
    rill_span::BytePos(file.text.len() as u32)
}
