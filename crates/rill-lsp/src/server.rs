//! The document store and the protocol loop.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::json::Json;

/// One open document: its text and the analysis last computed for it.
pub struct Document {
    pub text: String,
    pub version: i64,
    pub source: rill_span::SourceMap,
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
        let document = Document {
            text,
            version,
            source,
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

fn capabilities() -> Json {
    Json::obj(&[
        ("textDocumentSync", Json::Num(1.0)),
        ("documentFormattingProvider", Json::Bool(true)),
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
