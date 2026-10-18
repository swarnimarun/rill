//! Drives the server the way an editor does: framed JSON-RPC over stdio.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

fn read_frame(reader: &mut impl Read) -> Option<String> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        if reader.read_exact(&mut byte).is_err() {
            return None;
        }
        header.push(byte[0]);
    }
    let header = String::from_utf8_lossy(&header);
    let length: usize = header
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .and_then(|value| value.trim().parse().ok())?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).to_string())
}

#[test]
fn a_session_initialises_reports_diagnostics_and_shuts_down() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rill-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the server");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    let bad_source = "let main () -> i32 <computes> = \"text\";\n";
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///t.rill","languageId":"rill","version":1,"text":{}}}}}}}"#,
            rill_lsp_json_string(bad_source)
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[
                ("URI", "file:///h.rill"),
                (
                    "TEXT",
                    &json_string(
                        "let one () -> i32 <computes> = 1;\nlet main () -> i32 <computes> = one ();\n",
                    ),
                ),
            ],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            // Character 36 is inside `one ()` - the call - not the type
            // annotation, which is where an off-by-a-few first pointed.
            &[("URI", "file:///h.rill"), ("LINE", "1"), ("CHAR", "36")],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":10,"method":"textDocument/references","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[("URI", "file:///h.rill"), ("LINE", "1"), ("CHAR", "36")],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    for message in &messages {
        stdin.write_all(frame(message).as_bytes()).expect("write");
    }
    stdin.flush().expect("flush");
    drop(stdin);

    let mut seen = Vec::new();
    while let Some(body) = read_frame(&mut stdout) {
        seen.push(body);
    }
    let _ = child.wait();

    let all = seen.join("\n");
    assert!(
        all.contains("\"documentFormattingProvider\":true"),
        "the initialize result should advertise formatting: {all}"
    );
    let refs = seen
        .iter()
        .find(|body| body.contains(r#""id":10"#))
        .unwrap_or_else(|| panic!("no references answer: {all}"));
    assert!(
        refs.contains("file:///h.rill") && refs.contains("range"),
        "references should point at the use of `one`: {refs}"
    );
    let diagnostics = seen
        .iter()
        .find(|body| body.contains("publishDiagnostics"))
        .unwrap_or_else(|| panic!("no diagnostics were published: {all}"));
    assert!(
        diagnostics.contains("\"severity\":1"),
        "the type error should be an error: {diagnostics}"
    );
    assert!(
        diagnostics.contains("file:///t.rill"),
        "diagnostics are published for the document: {diagnostics}"
    );
}

/// Renders a Rust string as a JSON string literal, so the test does not hand-escape.
fn rill_lsp_json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Substitutes placeholders in a raw JSON string, so its braces stay literal -
/// hand-escaping them produced two unparseable messages before this test worked.
fn raw_message(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (key, value) in values {
        text = text.replace(key, value);
    }
    text
}

/// A Rust string as a JSON string literal.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
