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
