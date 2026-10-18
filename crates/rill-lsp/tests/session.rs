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
        raw_message(
            r#"{"jsonrpc":"2.0","id":11,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"URI"}}}"#,
            &[("URI", "file:///h.rill")],
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
    let tokens = seen
        .iter()
        .find(|body| body.contains(r#""id":11"#))
        .unwrap_or_else(|| panic!("no semantic tokens answer: {all}"));
    assert!(
        tokens.contains("\"data\":["),
        "semantic tokens should carry a data array: {tokens}"
    );
    assert!(
        !tokens.contains("\"data\":[]"),
        "the highlight should not be empty: {tokens}"
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

#[test]
fn hover_on_a_var_name_gives_its_type_not_unit() {
    // A local's name is not an expression, so an expression-only search answers with
    // the enclosing block's type - `()`. The name must come from the locals table.
    let source = "let main () -> i32 <computes> = {\n    var counter = 42;\n    counter\n};\n";
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///v.rill"), ("TEXT", &json_string(source))],
        ),
        // Inside `counter` in its declaration.
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[("URI", "file:///v.rill"), ("LINE", "1"), ("CHAR", "10")],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/inlayHint","params":{"textDocument":{"uri":"URI"},"range":{"start":{"line":0,"character":0},"end":{"line":99,"character":0}}}}"#,
            &[("URI", "file:///v.rill")],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let hints = output
        .iter()
        .find(|body| body.contains(r#""id":8"#))
        .unwrap_or_else(|| panic!("no inlay answer: {output:?}"));
    assert!(
        hints.contains(": i32"),
        "the binding should get a type hint: {hints}"
    );
    let hover = output
        .iter()
        .find(|body| body.contains(r#""id":9"#))
        .unwrap_or_else(|| panic!("no hover answer: {output:?}"));
    assert!(
        hover.contains("i32"),
        "a var's name should answer its type, not the block's unit: {hover}"
    );
    assert!(
        !hover.contains("\"value\":\"()\""),
        "the block's unit type leaked: {hover}"
    );
}

/// Runs a scripted session against the freshly built server, which is fast enough to
/// use in tests (no install step).
fn run_session(messages: &[String]) -> Vec<String> {
    use std::io::{Read, Write};
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rill-lsp"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the server");
    let mut stdin = child.stdin.take().expect("stdin");
    for message in messages {
        stdin.write_all(frame(message).as_bytes()).expect("write");
    }
    stdin.flush().expect("flush");
    drop(stdin);
    let mut stdout = child.stdout.take().expect("stdout");
    let mut out = Vec::new();
    while let Some(body) = read_frame(&mut stdout) {
        out.push(body);
    }
    let _ = child.wait();
    out
}

#[test]
fn inlay_hints_sit_after_an_identifier() {
    // The report was hints landing mid-word (`first_po : [i32]sitive`) and stacked
    // after a brace. The placement rule is therefore checked structurally: every hint
    // must directly follow an identifier character, which is what "after the name"
    // means.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/all.rill");
    let source = std::fs::read_to_string(&path).expect("examples/all.rill");
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///a.rill"), ("TEXT", &json_string(&source))],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/inlayHint","params":{"textDocument":{"uri":"URI"},"range":{"start":{"line":0,"character":0},"end":{"line":99,"character":0}}}}"#,
            &[("URI", "file:///a.rill")],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let answer = output
        .iter()
        .find(|body| body.contains(r#""id":8"#))
        .unwrap_or_else(|| panic!("no inlay answer: {output:?}"));
    let lines = source.lines().collect::<Vec<_>>();
    let mut checked = 0;
    // Each hint carries `"line": N, … "character": M`. Walking those pairs avoids
    // parsing JSON in a test whose subject is placement, not the protocol.
    let mut search_from = 0;
    while let Some(found) = answer[search_from..].find("\"character\":") {
        let at = search_from + found;
        let line_at = answer[..at].rfind("\"line\":").expect("a line precedes");
        let line = leading_number(&answer[line_at + 7..]);
        let col = leading_number(&answer[at + 12..]);
        if let Some(text) = lines.get(line) {
            let before: Vec<char> = text.chars().take(col).collect();
            let previous = before.last().copied().unwrap_or(' ');
            assert!(
                previous.is_alphanumeric() || previous == '_',
                "a hint sits after {previous:?} at {line}:{col} in `{text}`"
            );
        }
        checked += 1;
        search_from = at + 12;
    }
    assert!(checked > 0, "no hints were produced for all.rill");
}

/// The number at the start of `text`, with its index-sign ignored.
fn leading_number(text: &str) -> usize {
    text.chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}
