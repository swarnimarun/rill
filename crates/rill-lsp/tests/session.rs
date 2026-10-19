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

/// A completion request at a position, as an editor sends one.
fn completion_request(id: u32, uri: &str, line: usize, character: usize) -> String {
    raw_message(
        r#"{"jsonrpc":"2.0","id":ID,"method":"textDocument/completion","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
        &[
            ("ID", &id.to_string()),
            ("URI", uri),
            ("LINE", &line.to_string()),
            ("CHAR", &character.to_string()),
        ],
    )
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
    use std::io::Write;
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
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/all.rill");
    let source = std::fs::read_to_string(&path).expect("examples/all.rill");
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
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
        // A written type means no hint: a parameter is declared with its type, so the
        // only hints left in this file are the two `var` declarations.
        if let Some(text) = lines.get(line) {
            assert!(
                text.contains("var "),
                "a hint was emitted where the type is already written: `{text}`"
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

#[test]
fn hovering_fail_reports_the_bottom_type() {
    // `fail` never produces a value, so it must not be presented with whatever the
    // surrounding expression wanted - that would read as "failing yields an i32".
    let source = "let f (x: i32) -> i32 <decides> = { if (x > 0) { x } else { fail } };\n";
    let at = source.find("fail").expect("fail in the source");
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///c.rill"), ("TEXT", &json_string(source))],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":0,"character":COL}}}"#,
            &[("URI", "file:///c.rill"), ("COL", &(at + 1).to_string())],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let answer = output
        .iter()
        .find(|body| body.contains(r#""id":9"#))
        .unwrap_or_else(|| panic!("no hover answer: {output:?}"));
    assert!(
        answer.contains("never"),
        "hovering `fail` should report the bottom type: {answer}"
    );
    assert!(
        answer.contains("never <decides>"),
        "`fail` decides, so the hover must say so: {answer}"
    );
}

#[test]
fn hover_and_hints_carry_effects() {
    // The effect row is as much a part of what an expression is as its type: a
    // call answers with what it performs, and a binding's hint with what its
    // initializer costs. Neither is written in the source, so the server is the
    // only place they can come from.
    let source = "let f () -> i32 <computes> = { var b = buf_new (); buf_len (b) as i32 };\nlet main () -> i32 <io> = f ();\n";
    // The `(` inside `buf_new ()`: the call, not the callee path, which ends just
    // before it.
    let call = source.find("buf_new (").expect("the call") + "buf_new (".len() - 1;
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///e.rill"), ("TEXT", &json_string(source))],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":0,"character":COL}}}"#,
            &[("URI", "file:///e.rill"), ("COL", &call.to_string())],
        ),
        // A function value has no performed effects of its own to show; its type
        // carries the row instead.
        raw_message(
            r#"{"jsonrpc":"2.0","id":10,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":0,"character":COL}}}"#,
            &[
                ("URI", "file:///e.rill"),
                (
                    "COL",
                    &source.find("buf_new").expect("the callee").to_string(),
                ),
            ],
        ),
        // The binding's name answers what the hint next to it says.
        raw_message(
            r#"{"jsonrpc":"2.0","id":11,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":0,"character":COL}}}"#,
            &[
                ("URI", "file:///e.rill"),
                (
                    "COL",
                    &source.find("b = buf_new").expect("the binding").to_string(),
                ),
            ],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/inlayHint","params":{"textDocument":{"uri":"URI"},"range":{"start":{"line":0,"character":0},"end":{"line":99,"character":0}}}}"#,
            &[("URI", "file:///e.rill")],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let hover = output
        .iter()
        .find(|body| body.contains(r#""id":9"#))
        .unwrap_or_else(|| panic!("no hover answer: {output:?}"));
    assert!(
        hover.contains("Buf <alloc>"),
        "hovering the call should report its type and effect: {hover}"
    );
    let callee = output
        .iter()
        .find(|body| body.contains(r#""id":10"#))
        .unwrap_or_else(|| panic!("no callee hover answer: {output:?}"));
    assert!(
        callee.contains("() -> Buf <alloc>"),
        "a function value's hover should carry its effect row: {callee}"
    );
    let binding = output
        .iter()
        .find(|body| body.contains(r#""id":11"#))
        .unwrap_or_else(|| panic!("no binding hover answer: {output:?}"));
    assert!(
        binding.contains("Buf <alloc>"),
        "the name's hover must agree with its hint: {binding}"
    );
    let hints = output
        .iter()
        .find(|body| body.contains(r#""id":8"#))
        .unwrap_or_else(|| panic!("no inlay answer: {output:?}"));
    assert!(
        hints.contains(": Buf <alloc>"),
        "the hint should show the initializer's effect: {hints}"
    );
}

#[test]
fn undeclared_effects_mark_the_expression_that_performs_them() {
    // "performs alloc" without a place is a riddle in a body with several calls:
    // the diagnostic has to underline the call that performed the effect, not the
    // declaration it was written on.
    let source = "let f () -> i32 <computes> = { var b = buf_new (); buf_len (b) as i32 };\nlet main () -> i32 <io> = f ();\n";
    let at = source.find("buf_new ()").expect("the offending call");
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///x.rill"), ("TEXT", &json_string(source))],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let diagnostics = output
        .iter()
        .find(|body| body.contains("performs alloc"))
        .unwrap_or_else(|| panic!("no alloc diagnostic: {output:?}"));
    let expected = format!(
        r#""range":{{"start":{{"line":0,"character":{at}}},"end":{{"line":0,"character":{end}}}}}"#,
        end = at + "buf_new ()".len()
    );
    assert!(
        diagnostics.contains(&expected),
        "the diagnostic must underline `buf_new ()`: {diagnostics}"
    );
}

#[test]
fn discharged_decides_do_not_blame_the_handled_arm() {
    // The `let?` arm decides, but a fallback that cannot fail catches it - so the
    // only undischarged failure is the tail's. The diagnostic must underline the
    // tail, not the wider statement that was already handled: origins have to be
    // discharged with the effect, not just recorded as they are performed.
    let source = "let pick (x: i32) -> i32 <decides> = if (x > 0) { x } else { fail };\nlet f (x: i32) -> i32 <computes> = {\n    var a = { let? v = pick[x]; v } | 0;\n    pick[x]\n};\n";
    let first = source.find("pick[x]").expect("the arm's call");
    let handled = first
        + 1
        + source[first + 1..]
            .find("pick[x]")
            .expect("the tail's call");
    let line_start = source[..handled].rfind('\n').map(|at| at + 1).unwrap_or(0);
    let (line, character) = (
        source[..handled].matches('\n').count(),
        handled - line_start,
    );
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///m.rill"), ("TEXT", &json_string(source))],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let diagnostics = output
        .iter()
        .find(|body| body.contains("performs decides"))
        .unwrap_or_else(|| panic!("no decides diagnostic: {output:?}"));
    let expected = format!(
        r#""start":{{"line":{line},"character":{character}}}"#,
        line = line,
        character = character
    );
    assert!(
        diagnostics.contains(&expected),
        "the diagnostic must underline the tail's `pick[x]` at {line}:{character}: {diagnostics}"
    );
}

/// `(line, character)` for an offset, the way a request reports a position.
fn at(source: &str, offset: usize) -> (usize, usize) {
    let line = source[..offset].matches('\n').count();
    let line_start = source[..offset].rfind('\n').map(|at| at + 1).unwrap_or(0);
    (line, offset - line_start)
}

#[test]
fn completion_offers_the_names_in_scope_at_the_cursor() {
    // A name is offered from where it is bound: `count` and `total` are written
    // above the cursor, `later` only below it. `sum`, `Point` and the builtins
    // come from the module and the compiler's own table. A second request types a
    // prefix, which the server filters, and the edit range covers it.
    let source = "\
type Point = record { var x: i32; var y: i32; };
let sum (p: Point) -> i32 <computes> = p.x + p.y;
let main () -> i32 <computes> = {
    var count = 1;
    var total = 0;
    
    var later = 2;
    tot
    total
};
";
    let (line, character) = at(source, source.find("    \n").expect("the blank line") + 4);
    let (prefix_line, prefix_character) = at(
        source,
        source.find("    tot\n").expect("the partial name") + 7,
    );
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///n.rill"), ("TEXT", &json_string(source))],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/completion","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", "file:///n.rill"),
                ("LINE", &line.to_string()),
                ("CHAR", &character.to_string()),
            ],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":10,"method":"textDocument/completion","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", "file:///n.rill"),
                ("LINE", &prefix_line.to_string()),
                ("CHAR", &prefix_character.to_string()),
            ],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let init = output
        .iter()
        .find(|body| body.contains(r#""id":1"#))
        .unwrap_or_else(|| panic!("no initialize answer: {output:?}"));
    assert!(
        init.contains("completionProvider"),
        "completion must be advertised, or no client asks for it: {init}"
    );
    let answer = output
        .iter()
        .find(|body| body.contains(r#""id":9"#))
        .unwrap_or_else(|| panic!("no completion answer: {output:?}"));
    for expected in [
        r#""label":"count""#,
        r#""label":"total","kind":6"#,
        r#""label":"sum","kind":3"#,
        r#""label":"Point","kind":22"#,
        r#""label":"Point","kind":22,"detail":"record""#,
        r#""label":"buf_new","kind":3,"detail":"() -> Buf <alloc>""#,
    ] {
        assert!(
            answer.contains(expected),
            "{expected} missing from the answer: {answer}"
        );
    }
    assert!(
        !answer.contains(r#""label":"later""#),
        "a binding below the cursor is not in scope yet: {answer}"
    );
    // An empty prefix replaces the whole name being typed, which is nothing here.
    let expected_range = format!(
        r#""range":{{"start":{{"line":{line},"character":{character}}},"end":{{"line":{line},"character":{character}}}}}"#
    );
    assert!(
        answer.contains(&expected_range),
        "the edit range is the name being typed: {answer}"
    );

    let filtered = output
        .iter()
        .find(|body| body.contains(r#""id":10"#))
        .unwrap_or_else(|| panic!("no prefixed completion answer: {output:?}"));
    assert!(
        filtered.contains(r#""label":"total""#),
        "`tot` completes to `total`: {filtered}"
    );
    for missing in [r#""label":"count""#, r#""label":"later""#] {
        assert!(
            !filtered.contains(missing),
            "{missing} does not start with the typed prefix: {filtered}"
        );
    }
    let expected_range = format!(
        r#""range":{{"start":{{"line":{prefix_line},"character":{}}}"#,
        prefix_character - 3
    );
    assert!(
        filtered.contains(&expected_range),
        "the edit range covers the typed prefix: {filtered}"
    );
}

#[test]
fn completion_after_a_dot_offers_fields_and_methods() {
    // The fields come from the receiver's type; the methods from the callables in
    // scope that take it as a first argument, since `p.sum ()` is `sum (p)`. The
    // record's constructor is not a method - its first parameter is an `i32` - and
    // a chained access answers for the whole receiver text, not for its head.
    let source = "\
type Point = record { var x: i32; var y: i32; };
type Key = record { var scancode: i32; };
type Event = record { var key: Key; };
let sum (p: Point) -> i32 <computes> = p.x + p.y;
let main () -> i32 <computes> = {
    var p = Point (1, 2);
    var ev = Event (Key (5));
    var out = buf_new ();
    p.;
    ev.key.;
    out.;
    p.x
};
";
    let (line, character) = at(source, source.find("    p.;\n").expect("the access") + 6);
    let (key_line, key_character) = at(
        source,
        source.find("    ev.key.;\n").expect("the chain") + 11,
    );
    let (buf_line, buf_character) = at(source, source.find("    out.;\n").expect("the buffer") + 8);
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", "file:///d.rill"), ("TEXT", &json_string(source))],
        ),
        completion_request(9, "file:///d.rill", line, character),
        completion_request(10, "file:///d.rill", key_line, key_character),
        completion_request(11, "file:///d.rill", buf_line, buf_character),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let answer = |id: u32| -> String {
        output
            .iter()
            .find(|body| body.contains(&format!(r#""id":{id}"#)))
            .unwrap_or_else(|| panic!("no answer for {id}: {output:?}"))
            .to_string()
    };

    let fields = answer(9);
    assert!(
        fields.contains(r#""label":"x","kind":5,"detail":"i32""#),
        "the record's fields are offered with their types: {fields}"
    );
    assert!(
        fields.contains(r#""label":"y","kind":5"#),
        "every field, in declaration order: {fields}"
    );
    assert!(
        fields.contains(r#""label":"sum","kind":2,"detail":"(Point) -> i32 <computes>""#),
        "`sum` takes a `Point` first, so it is a method of one: {fields}"
    );
    assert!(
        !fields.contains(r#""label":"Point""#),
        "a constructor whose first parameter is not the receiver is not a method: {fields}"
    );

    // `scancode` is a field of the `Key` that `ev.key` holds; `key` is the field
    // of `Event` that was already written, so it must not be offered.
    let chained = answer(10);
    assert!(
        chained.contains(r#""label":"scancode","kind":5,"detail":"i32""#),
        "the innermost access is the receiver: {chained}"
    );
    assert!(
        !chained.contains(r#""label":"key""#),
        "the head of the chain is not: {chained}"
    );

    // The builtins are called the same way - `out.buf_push (b)` is
    // `buf_push (out, b)` - so they are offered after a `.` too, and a builtin
    // that takes no receiver (`buf_new`) is not.
    let methods = answer(11);
    assert!(
        methods.contains(r#""label":"buf_push","kind":2,"detail":"(Buf, u8) -> () <alloc>""#),
        "a builtin whose first parameter is the receiver is a method: {methods}"
    );
    assert!(
        methods.contains(r#""label":"buf_len","kind":2"#),
        "every builtin that takes one: {methods}"
    );
    assert!(
        !methods.contains(r#""label":"buf_new""#),
        "a builtin that takes no receiver is not: {methods}"
    );
}

#[test]
fn completion_after_a_module_path_lists_the_header_declarations() {
    // `M::name` reaches what the header contributed, so a `::` after a module
    // name offers exactly those declarations - with the same kinds and details a
    // bare name gets.
    let dir = std::env::temp_dir().join("rill-lsp-completion-module");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workdir");
    std::fs::write(
        dir.join("m.h"),
        "extern int triple(int n);\n#define TRIPLE 3\n",
    )
    .expect("write the header");
    let source = "type M = @include(\"m.h\");\nlet main () -> i32 <computes> = M::;\n";
    let (line, character) = at(source, source.find("M::").expect("the module path") + 3);
    let uri = format!("file://{}", dir.join("m.rill").display());
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", &uri), ("TEXT", &json_string(source))],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/completion","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", &uri),
                ("LINE", &line.to_string()),
                ("CHAR", &character.to_string()),
            ],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let answer = output
        .iter()
        .find(|body| body.contains(r#""id":9"#))
        .unwrap_or_else(|| panic!("no completion answer: {output:?}"));
    assert!(
        answer.contains(r#""label":"triple","kind":3,"detail":"(i32) -> i32 <io>""#),
        "the header's prototype, with the row the import gives it: {answer}"
    );
    assert!(
        answer.contains(r#""label":"TRIPLE","kind":21"#),
        "the header's `#define` is a constant: {answer}"
    );
    assert!(
        !answer.contains(r#""label":"main""#),
        "a module path lists the module's members, not the enclosing scope: {answer}"
    );
}

#[test]
fn documentation_reaches_completion_and_hover() {
    // `///` docs on a Rill declaration, and a header's Doxygen block on an
    // imported prototype: both should reach the editor through the same
    // `documentation` field completion carries and the Markdown hover.
    let dir = std::env::temp_dir().join("rill-lsp-docs");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workdir");
    std::fs::write(
        dir.join("h.h"),
        "/**\n * Triple a number.\n *\n * \\param n the number.\n * \\returns three times `n`.\n */\nextern int triple(int n);\n",
    )
    .expect("write the header");
    let source = "\
/// Doubles a number.
let double (x: i32) -> i32 <computes> = x * 2;
type M = @include(\"h.h\");
/// A point.
type Point = record {
    /// The x coordinate.
    var x: i32;
};
let main () -> i32 <computes> = {
    var p = Point (1);
    double (M::triple (p.x));
    p.;
};
";
    let uri = format!("file://{}", dir.join("d.rill").display());
    let (scope_line, scope_char) = at(
        source,
        source.find("    double (M::").expect("the body") + 4,
    );
    let (module_line, module_char) = at(
        source,
        source.find("M::triple").expect("the module path") + 3,
    );
    let (use_line, use_char) = at(source, source.find("    double (M::").expect("the use") + 6);
    let triple_start = source.find("M::triple").expect("the callee") + 3;
    let (triple_line, triple_char) = at(source, triple_start + 2);
    let (decl_line, decl_char) = at(
        source,
        source.find("double (").expect("the declaration") + 2,
    );
    let (field_line, field_char) = at(
        source,
        source.find("    p.;").expect("the field access") + 6,
    );
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", &uri), ("TEXT", &json_string(source))],
        ),
        completion_request(9, &uri, scope_line, scope_char),
        completion_request(10, &uri, module_line, module_char),
        completion_request(14, &uri, field_line, field_char),
        raw_message(
            r#"{"jsonrpc":"2.0","id":11,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", &uri),
                ("LINE", &use_line.to_string()),
                ("CHAR", &use_char.to_string()),
            ],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":12,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", &uri),
                ("LINE", &triple_line.to_string()),
                ("CHAR", &triple_char.to_string()),
            ],
        ),
        raw_message(
            r#"{"jsonrpc":"2.0","id":13,"method":"textDocument/hover","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("URI", &uri),
                ("LINE", &decl_line.to_string()),
                ("CHAR", &decl_char.to_string()),
            ],
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let output = run_session(&messages);
    let answer = |id: u32| -> String {
        output
            .iter()
            .find(|body| body.contains(&format!(r#""id":{id}"#)))
            .unwrap_or_else(|| panic!("no answer for {id}: {output:?}"))
            .to_string()
    };

    let scope = answer(9);
    assert!(
        scope.contains(r#""label":"double""#)
            && scope.contains(r#""documentation":{"kind":"markdown","value":"Doubles a number."}"#),
        "the Rill doc comment should ride on the completion: {scope}"
    );

    let module = answer(10);
    assert!(
        module.contains(r#""label":"triple""#)
            && module.contains(r#""documentation":{"kind":"markdown","value":"Triple a number.\n\n- **n**: the number.\n**Returns:** three times `n`.""#),
        "the header's Doxygen block should ride on the module completion: {module}"
    );

    let fields = answer(14);
    assert!(
        fields.contains(r#""label":"x","kind":5,"detail":"i32","documentation":{"kind":"markdown","value":"The x coordinate."}"#),
        "a field's `///` docs should ride on the completion after `.`: {fields}"
    );

    let use_hover = answer(11);
    assert!(
        use_hover.contains(r#""kind":"markdown""#)
            && use_hover.contains("Doubles a number.")
            && use_hover.contains("```rill"),
        "hovering a documented call should show its docs: {use_hover}"
    );

    let triple_hover = answer(12);
    assert!(
        triple_hover.contains("Triple a number.")
            && triple_hover.contains("- **n**: the number.")
            && triple_hover.contains("three times `n`"),
        "hovering an imported function should show the header docs: {triple_hover}"
    );

    let decl_hover = answer(13);
    assert!(
        decl_hover.contains("Doubles a number.") && decl_hover.contains("(i32) -> i32"),
        "hovering the declaration name should show its docs and signature: {decl_hover}"
    );
}

#[test]
fn definition_opens_headers_and_finds_rill_declarations() {
    // A C declaration's definition is the header line; a Rill item's is its
    // name. References answer from the same table, so a type written in a
    // `var` annotation is found too.
    let dir = std::env::temp_dir().join("rill-lsp-definition");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workdir");
    let header = "\
#ifndef H_
#define H_

extern int triple(int n);

#define TRIPLE 3

#endif
";
    std::fs::write(dir.join("h.h"), header).expect("write the header");
    let source = "\
type M = @include(\"h.h\");
use M::*;
/// Doubles a number.
let double (x: i32) -> i32 <computes> = x * 2;
type Point = record { var x: i32; };
let main () -> i32 <computes> = {
    var p: Point = Point (1);
    double (M::triple (TRIPLE))
};
";
    let uri = format!("file://{}", dir.join("d.rill").display());
    let (triple_line, triple_char) = at(source, source.find("M::triple").expect("the callee") + 3);
    let (macro_line, macro_char) = at(source, source.find("TRIPLE)").expect("the macro") + 2);
    let (call_line, call_char) = at(source, source.find("double (").expect("the call") + 3);
    let (type_line, type_char) = at(source, source.find("p: Point").expect("the annotation") + 3);
    let mut messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", &uri), ("TEXT", &json_string(source))],
        ),
    ];
    for (id, line, character) in [
        (9, triple_line, triple_char),
        (10, macro_line, macro_char),
        (11, call_line, call_char),
        (12, type_line, type_char),
    ] {
        messages.push(raw_message(
            r#"{"jsonrpc":"2.0","id":ID,"method":"textDocument/definition","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("ID", &id.to_string()),
                ("URI", &uri),
                ("LINE", &line.to_string()),
                ("CHAR", &character.to_string()),
            ],
        ));
    }
    for (id, line, character) in [(13, macro_line, macro_char), (14, type_line, type_char)] {
        messages.push(raw_message(
            r#"{"jsonrpc":"2.0","id":ID,"method":"textDocument/references","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("ID", &id.to_string()),
                ("URI", &uri),
                ("LINE", &line.to_string()),
                ("CHAR", &character.to_string()),
            ],
        ));
    }
    messages.push(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string());
    messages.push(r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string());

    let output = run_session(&messages);
    let answer = |id: u32| -> String {
        output
            .iter()
            .find(|body| body.contains(&format!(r#""id":{id}"#)))
            .unwrap_or_else(|| panic!("no answer for {id}: {output:?}"))
            .to_string()
    };

    // `extern int triple` is line 3 (zero-based) of h.h; the name starts at
    // column 11. The scanner canonicalizes header paths, so compare against
    // the canonical directory.
    let canonical = std::fs::canonicalize(&dir).expect("canonical workdir");
    let triple = answer(9);
    assert!(
        triple.contains(&format!(
            r#""uri":"file://{}/h.h","range":{{"start":{{"line":3,"character":11}},"end":{{"line":3,"character":17}}}}"#,
            canonical.display()
        )),
        "triple should open the header at its name: {triple}"
    );

    // `#define TRIPLE 3` is line 5; the macro name starts at column 8.
    let macro_definition = answer(10);
    assert!(
        macro_definition.contains(r#""line":5,"character":8"#) && macro_definition.contains("/h.h"),
        "TRIPLE should open the header at its #define: {macro_definition}"
    );

    // The Rill function's definition is its name in this document (line 3).
    let rill = answer(11);
    assert!(
        rill.contains(&format!(r#""uri":"{uri}""#)) && rill.contains(r#""line":3,"character":4"#),
        "a Rill function's definition is its name: {rill}"
    );

    // `Point` in `var p: Point` is a type reference; its definition is the
    // type declaration on line 3, and references collect the annotation.
    let type_definition = answer(12);
    assert!(
        type_definition.contains(r#""line":4,"character":5"#),
        "a type annotation should reach the type declaration: {type_definition}"
    );

    let macro_refs = answer(13);
    assert!(
        macro_refs.contains(r#""line":7"#) && macro_refs.contains("range"),
        "references should collect the macro's use: {macro_refs}"
    );
    let type_refs = answer(14);
    assert!(
        type_refs.contains(r#""line":4"#) && type_refs.contains(r#""line":6"#),
        "references for a type should include the annotation and the declaration: {type_refs}"
    );
}

#[test]
fn references_from_a_binding_and_from_a_use_agree() {
    // A binding's name is not an expression, so the reference table alone has
    // nothing under the cursor there. Both a parameter and a `var` must answer
    // with their uses *and* their declaration, from either side, and a shadowed
    // local must not collect the outer one.
    let source = "\
let double (x: i32) -> i32 <computes> = x + x;
let main () -> i32 <computes> = {
    var total = 0;
    var inner = { var total = 3; total };
    set total = total + double (inner);
    total
};
";
    let (param_line, param_char) = at(source, source.find("x: i32").expect("parameter"));
    let first_use = source.find("= x + x").expect("parameter use") + 2;
    let (param_use_line, param_use_char) = at(source, first_use);
    let (param_second_line, param_second_char) = at(source, first_use + 4);
    let (var_line, var_char) = at(source, source.find("total = 0").expect("binding"));
    let (set_line, set_char) = at(source, source.find("set total").expect("set") + 4);
    let (tail_line, tail_char) = at(source, source.rfind("    total\n").expect("tail") + 4);
    let (inner_line, inner_char) = at(source, source.find("total = 3").expect("inner binding"));
    let (inner_use_line, inner_use_char) =
        at(source, source.find("3; total").expect("inner use") + 3);
    let uri = "file:///refs.rill";
    let mut messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        raw_message(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"URI","languageId":"rill","version":1,"text":TEXT}}}"#,
            &[("URI", uri), ("TEXT", &json_string(source))],
        ),
    ];
    for (id, line, character) in [
        (9, param_line, param_char),
        (10, param_use_line, param_use_char),
        (11, var_line, var_char),
        (12, set_line, set_char),
        (13, inner_line, inner_char),
    ] {
        messages.push(raw_message(
            r#"{"jsonrpc":"2.0","id":ID,"method":"textDocument/references","params":{"textDocument":{"uri":"URI"},"position":{"line":LINE,"character":CHAR}}}"#,
            &[
                ("ID", &id.to_string()),
                ("URI", uri),
                ("LINE", &line.to_string()),
                ("CHAR", &character.to_string()),
            ],
        ));
    }
    messages.push(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string());
    messages.push(r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string());

    let output = run_session(&messages);
    let answer = |id: u32| -> String {
        output
            .iter()
            .find(|body| body.contains(&format!(r#""id":{id}"#)))
            .unwrap_or_else(|| panic!("no answer for {id}: {output:?}"))
            .to_string()
    };
    let count = |body: &str| body.matches("\"uri\"").count();

    let from_param = answer(9);
    assert_eq!(
        count(&from_param),
        3,
        "parameter: declaration + two uses: {from_param}"
    );
    assert!(
        from_param.contains(&format!(r#""line":{param_line},"character":{param_char}"#))
            && from_param.contains(&format!(
                r#""line":{param_use_line},"character":{param_use_char}"#
            ))
            && from_param.contains(&format!(
                r#""line":{param_second_line},"character":{param_second_char}"#
            )),
        "the parameter's declaration and both uses: {from_param}"
    );
    assert_eq!(answer(10), from_param.replace(r#""id":9"#, r#""id":10"#));

    let from_var = answer(11);
    assert_eq!(
        count(&from_var),
        4,
        "binding: declaration and three uses: {from_var}"
    );
    assert!(
        from_var.contains(&format!(r#""line":{var_line},"character":{var_char}"#))
            && from_var.contains(&format!(r#""line":{set_line},"character":{set_char}"#))
            && from_var.contains(&format!(r#""line":{tail_line},"character":{tail_char}"#)),
        "the binding's declaration and uses: {from_var}"
    );
    assert!(
        !from_var.contains(&format!(r#""line":{inner_line},"#)),
        "a shadowed local must not join the outer one: {from_var}"
    );
    assert_eq!(answer(12), from_var.replace(r#""id":11"#, r#""id":12"#));

    let from_inner = answer(13);
    assert_eq!(
        count(&from_inner),
        2,
        "the inner binding: declaration and use only: {from_inner}"
    );
    assert!(
        from_inner.contains(&format!(r#""line":{inner_line},"character":{inner_char}"#))
            && from_inner.contains(&format!(
                r#""line":{inner_use_line},"character":{inner_use_char}"#
            )),
        "the inner declaration and use: {from_inner}"
    );
    assert!(
        !from_inner.contains(&format!(r#""line":{var_line},"#)),
        "the inner name must not collect the outer one: {from_inner}"
    );
}
