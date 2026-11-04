//! Synchronous stdio language server using the compiler's existing analysis.
pub mod analysis;
mod protocol;
pub mod text;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

pub(super) const MAX_DOCUMENT: usize = 1024 * 1024;
const MAX_OPEN_DOCUMENTS: usize = 128;
const MAX_OPEN_BYTES: usize = 16 * MAX_DOCUMENT;

struct Document {
    text: String,
    version: i64,
}

#[derive(Default)]
struct Server {
    initialized: bool,
    shutdown: bool,
    documents: BTreeMap<String, Document>,
}

pub fn run_stdio() -> Result<(), String> {
    let input = io::stdin();
    let output = io::stdout();
    if serve(&mut input.lock(), &mut output.lock())? {
        Ok(())
    } else {
        Err("rill lsp: exited without shutdown".into())
    }
}

fn serve(reader: &mut impl BufRead, writer: &mut impl Write) -> Result<bool, String> {
    let mut server = Server::default();
    while let Some(body) = protocol::read(reader).map_err(|e| format!("rill lsp: {e}"))? {
        let message: Value = match serde_json::from_slice(&body) {
            Ok(message) => message,
            Err(_) => {
                send_error(writer, Value::Null, -32700, "invalid JSON")?;
                continue;
            }
        };
        let id = message.get("id").cloned();
        let valid_id = id
            .as_ref()
            .is_none_or(|v| v.is_null() || v.is_string() || v.is_i64() || v.is_u64());
        if !message.is_object()
            || message["jsonrpc"] != "2.0"
            || !valid_id
            || !message["method"].is_string()
        {
            send_error(writer, Value::Null, -32600, "invalid JSON-RPC request")?;
            continue;
        }
        let method = message["method"].as_str().unwrap();
        if method == "exit" && id.is_none() {
            return Ok(server.shutdown);
        }
        let params = message.get("params").unwrap_or(&Value::Null);
        if let Some(id) = id {
            let result = server.request(method, params);
            match result {
                Ok(result) => send(writer, &json!({"jsonrpc":"2.0","id":id,"result":result}))?,
                Err((code, message)) => send_error(writer, id, code, &message)?,
            }
        } else if server.initialized && !server.shutdown {
            match server.notification(method, params) {
                Ok(true) => server.publish(writer)?,
                Ok(false) => {}
                Err(error) => eprintln!("rill lsp: {error}"),
            }
            if method == "textDocument/didClose" {
                if let Some(uri) = params["textDocument"]["uri"].as_str() {
                    send(
                        writer,
                        &json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[]}}),
                    )?;
                }
            }
        }
    }
    Ok(server.shutdown)
}

fn send(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    protocol::write(writer, value).map_err(|e| format!("rill lsp: {e}"))
}

fn send_error(writer: &mut impl Write, id: Value, code: i32, message: &str) -> Result<(), String> {
    send(
        writer,
        &json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
    )
}

type Response = Result<Value, (i32, String)>;
fn invalid(message: &str) -> (i32, String) {
    (-32602, message.into())
}

impl Server {
    fn overlays(&self) -> BTreeMap<String, String> {
        self.documents
            .iter()
            .map(|(uri, doc)| (uri.clone(), doc.text.clone()))
            .collect()
    }

    fn request(&mut self, method: &str, params: &Value) -> Response {
        if method == "initialize" {
            if self.initialized {
                return Err((-32600, "server is already initialized".into()));
            }
            if !params.is_object() {
                return Err(invalid("initialize params must be an object"));
            }
            self.initialized = true;
            return Ok(json!({
                "serverInfo":{"name":"rill","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{
                    "positionEncoding":"utf-16",
                    "textDocumentSync":{"openClose":true,"change":2,"save":{"includeText":false}},
                    "hoverProvider":true,"definitionProvider":true,"documentSymbolProvider":true,
                    "documentFormattingProvider":true,
                    "inlayHintProvider":true,
                    "signatureHelpProvider":{"triggerCharacters":["(",","],"retriggerCharacters":[","]},
                    "completionProvider":{"resolveProvider":false,"triggerCharacters":["."]}
                }
            }));
        }
        if !self.initialized {
            return Err((-32002, "server is not initialized".into()));
        }
        if self.shutdown {
            return Err((-32600, "server has shut down".into()));
        }
        if method == "shutdown" {
            self.shutdown = true;
            return Ok(Value::Null);
        }
        if !matches!(
            method,
            "textDocument/hover"
                | "textDocument/definition"
                | "textDocument/completion"
                | "textDocument/documentSymbol"
                | "textDocument/formatting"
                | "textDocument/inlayHint"
                | "textDocument/signatureHelp"
        ) {
            return Err((-32601, format!("method not found: {method}")));
        }
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or_else(|| invalid("missing document URI"))?;
        let document = self
            .documents
            .get(uri)
            .ok_or_else(|| invalid("document is not open"))?;
        if method == "textDocument/documentSymbol" {
            return Ok(Value::Array(analysis::document_symbols(&document.text)));
        }
        if method == "textDocument/formatting" {
            let formatted = crate::formatter::format(&document.text).map_err(|e| invalid(&e))?;
            if formatted == document.text {
                return Ok(json!([]));
            }
            return Ok(
                json!([{"range":text::range(&document.text,0..document.text.len()),"newText":formatted}]),
            );
        }
        if method == "textDocument/inlayHint" {
            let range = &params["range"];
            let start = text::offset(&document.text, &range["start"])
                .ok_or_else(|| invalid("invalid UTF-16 range start"))?;
            let end = text::offset(&document.text, &range["end"])
                .ok_or_else(|| invalid("invalid UTF-16 range end"))?;
            if start > end {
                return Err(invalid("reversed inlay hint range"));
            }
            return Ok(Value::Array(analysis::inlay_hints(
                uri,
                &document.text,
                start..end,
                &self.overlays(),
            )));
        }
        let offset = text::offset(&document.text, &params["position"])
            .ok_or_else(|| invalid("invalid UTF-16 position"))?;
        let overlays = self.overlays();
        match method {
            "textDocument/hover" => {
                Ok(analysis::hover(uri, &document.text, offset, &overlays).unwrap_or(Value::Null))
            }
            "textDocument/definition" => {
                Ok(analysis::definition(uri, &document.text, offset, &overlays)
                    .unwrap_or(Value::Null))
            }
            "textDocument/completion" => Ok(
                json!({"isIncomplete":false,"items":analysis::completion(uri, &document.text, offset, &overlays)}),
            ),
            "textDocument/signatureHelp" => {
                Ok(
                    analysis::signature_help(uri, &document.text, offset, &overlays)
                        .unwrap_or(Value::Null),
                )
            }
            _ => unreachable!(),
        }
    }

    fn notification(&mut self, method: &str, params: &Value) -> Result<bool, &'static str> {
        if matches!(method, "initialized" | "$/cancelRequest" | "$/setTrace") {
            return Ok(false);
        }
        if method == "workspace/didChangeWatchedFiles" {
            return Ok(true);
        }
        if !matches!(
            method,
            "textDocument/didOpen"
                | "textDocument/didChange"
                | "textDocument/didSave"
                | "textDocument/didClose"
        ) {
            return Ok(false);
        }
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or("missing document URI")?;
        if uri.is_empty() || uri.len() > 8192 {
            return Err("invalid document URI");
        }
        if method == "textDocument/didClose" {
            self.documents.remove(uri);
            return Ok(true);
        }
        if method == "textDocument/didSave" {
            return Ok(self.documents.contains_key(uri));
        }
        let version = params["textDocument"]["version"]
            .as_i64()
            .filter(|n| i32::try_from(*n).is_ok())
            .ok_or("invalid document version")?;
        let new_text = if method == "textDocument/didOpen" {
            if self.documents.contains_key(uri) {
                return Err("document is already open");
            }
            if self.documents.len() >= MAX_OPEN_DOCUMENTS {
                return Err("too many open documents");
            }
            params["textDocument"]["text"]
                .as_str()
                .ok_or("missing document text")?
                .to_owned()
        } else {
            let document = self.documents.get(uri).ok_or("document is not open")?;
            if version <= document.version {
                return Ok(false);
            }
            text::apply_changes(&document.text, &params["contentChanges"])?
        };
        if new_text.len() > MAX_DOCUMENT {
            return Err("document exceeds 1 MiB");
        }
        let other_bytes: usize = self
            .documents
            .iter()
            .filter(|(key, _)| key.as_str() != uri)
            .map(|(_, document)| document.text.len())
            .sum();
        if other_bytes + new_text.len() > MAX_OPEN_BYTES {
            return Err("open documents exceed 16 MiB");
        }
        self.documents.insert(
            uri.into(),
            Document {
                text: new_text,
                version,
            },
        );
        Ok(true)
    }

    fn publish(&self, writer: &mut impl Write) -> Result<(), String> {
        let overlays = self.overlays();
        for (uri, document) in &self.documents {
            let diagnostics = analysis::diagnostics(uri, &document.text, &overlays);
            send(
                writer,
                &json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"version":document.version,"diagnostics":diagnostics}}),
            )?;
        }
        Ok(())
    }
}
