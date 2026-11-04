use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

struct Client {
    child: Child,
    input: ChildStdin,
    messages: mpsc::Receiver<Value>,
    reader: Option<JoinHandle<()>>,
    notifications: Vec<Value>,
}

impl Client {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("lsp")
            .env("PATH", "")
            .env("CC", "lsp-must-not-run-a-c-compiler")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (send, messages) = mpsc::channel();
        let reader = std::thread::spawn(move || loop {
            let mut length = None;
            loop {
                let mut header = String::new();
                if output.read_line(&mut header).unwrap() == 0 {
                    return;
                }
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.strip_prefix("Content-Length: ") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.expect("server output must be framed")];
            output.read_exact(&mut body).unwrap();
            if send.send(serde_json::from_slice(&body).unwrap()).is_err() {
                return;
            }
        });
        Self {
            child,
            input,
            messages,
            reader: Some(reader),
            notifications: vec![],
        }
    }

    fn send(&mut self, value: Value) {
        self.raw(&serde_json::to_vec(&value).unwrap());
    }

    fn raw(&mut self, body: &[u8]) {
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        for fragment in body.chunks(7) {
            self.input.write_all(fragment).unwrap();
        }
        self.input.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.messages
            .recv_timeout(Duration::from_secs(10))
            .expect("language server did not respond")
    }

    fn request(&mut self, id: i32, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let message = self.next();
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    fn initialize(&mut self) {
        let response = self.request(1, "initialize", json!({"capabilities":{}}));
        assert_eq!(
            response["result"]["capabilities"]["positionEncoding"],
            "utf-16"
        );
        assert_eq!(
            response["result"]["capabilities"]["textDocumentSync"]["change"],
            2
        );
        assert_eq!(
            response["result"]["capabilities"]["inlayHintProvider"],
            true
        );
        assert_eq!(
            response["result"]["capabilities"]["signatureHelpProvider"]["triggerCharacters"],
            json!(["(", ","])
        );
        self.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"rill","version":1,"text":text}}}));
    }

    fn change(&mut self, uri: &str, version: i32, changes: Value) {
        self.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":version},"contentChanges":changes}}));
    }

    fn diagnostics(&mut self, uri: &str, version: Option<i64>) -> Value {
        let matches = |message: &Value| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri
                && message["params"].get("version").and_then(Value::as_i64) == version
        };
        if let Some(index) = self.notifications.iter().position(matches) {
            return self.notifications.remove(index)["params"]["diagnostics"].clone();
        }
        loop {
            let message = self.next();
            if matches(&message) {
                return message["params"]["diagnostics"].clone();
            }
            self.notifications.push(message);
        }
    }

    fn shutdown(&mut self) {
        assert_eq!(
            self.request(90, "shutdown", Value::Null)["result"],
            Value::Null
        );
        self.send(json!({"jsonrpc":"2.0","method":"exit"}));
        assert!(self.wait().success());
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "server did not exit"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill LSP {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn uri(&self, file: &str) -> String {
        rill::lsp::text::path_to_uri(&self.0.join(file)).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lifecycle_and_json_errors_follow_the_real_stdio_protocol() {
    let mut client = Client::new();
    client.raw(b"{");
    assert_eq!(client.next()["error"]["code"], -32700);
    assert_eq!(
        client.request(0, "textDocument/hover", json!({}))["error"]["code"],
        -32002
    );
    client.initialize();
    assert_eq!(
        client.request(2, "initialize", json!({}))["error"]["code"],
        -32600
    );
    assert_eq!(
        client.request(3, "not/a/method", json!({}))["error"]["code"],
        -32601
    );
    assert_eq!(
        client.request(4, "textDocument/hover", json!({}))["error"]["code"],
        -32602
    );
    assert_eq!(
        client.request(5, "shutdown", Value::Null)["result"],
        Value::Null
    );
    assert_eq!(
        client.request(6, "not/a/method", json!({}))["error"]["code"],
        -32600
    );
    client.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(client.wait().success());
    let mut early = Client::new();
    early.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert_eq!(early.wait().code(), Some(1));
}

#[test]
fn malformed_transport_headers_terminate_without_stdout_noise() {
    for header in [
        "Content-Length: 8388609\r\n\r\n",
        "Content-Length: 1\r\nContent-Length: 1\r\n\r\nx",
        "Content-Length: -1\r\n\r\n",
    ] {
        let mut client = Client::new();
        client.input.write_all(header.as_bytes()).unwrap();
        client.input.flush().unwrap();
        assert_eq!(client.wait().code(), Some(1));
        assert!(client.messages.try_recv().is_err());
    }
}

#[test]
fn inlay_hints_and_signature_help_use_stdio_overlays_and_validate_utf16_params() {
    let directory = Directory::new();
    let main = directory.uri("main.rill");
    let helper = directory.uri("helper.rill");
    let source =
        "import helper as h; fn main() -> i32 { let text = \"😀\"; h.add(byte_len(text), 2) }";
    let mut client = Client::new();
    client.initialize();
    client.open(
        &helper,
        "export fn add(left, right) -> i32 { left + right }",
    );
    client.open(&main, source);
    let range = rill::lsp::text::range(source, 0..source.len());
    let result = client.request(
        10,
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":main},"range":range}),
    );
    assert!(result["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hint| hint["label"] == ": str"));
    assert!(result["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hint| hint["label"] == "left:"));
    assert!(!result["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hint| hint["label"]
            .as_str()
            .is_some_and(|label| label.starts_with(" ->"))));
    let at = source.rfind(", 2").unwrap() + 1;
    let result = client.request(
        11,
        "textDocument/signatureHelp",
        json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(source, at)}),
    );
    assert_eq!(result["result"]["activeParameter"], 1);
    assert!(result["result"]["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("left: i32, right: i32"));
    let bad = rill::lsp::text::position(source, source.find('😀').unwrap());
    let bad = json!({"line":bad["line"],"character":bad["character"].as_u64().unwrap() + 1});
    for (id, method, extra) in [
        (12, "textDocument/signatureHelp", json!({"position":bad})),
        (
            13,
            "textDocument/inlayHint",
            json!({"range":{"start":bad,"end":bad}}),
        ),
        (
            14,
            "textDocument/inlayHint",
            json!({"range":{"start":{"line":0,"character":10},"end":{"line":0,"character":0}}}),
        ),
    ] {
        let mut params = extra;
        params["textDocument"] = json!({"uri":main});
        assert_eq!(client.request(id, method, params)["error"]["code"], -32602);
    }
    let incomplete = "import helper as h; fn main() -> i32 { h.add(1, }";
    client.change(&main, 2, json!([{"text":incomplete}]));
    let result = client.request(15, "textDocument/signatureHelp", json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(incomplete,incomplete.find("1,").unwrap() + 2)}));
    assert_eq!(result["result"]["activeParameter"], 1);
    client.shutdown();
}

#[test]
fn edits_diagnostics_navigation_and_formatting_use_unsaved_text() {
    let dir = Directory::new();
    let uri = dir.uri("main.rill");
    let mut client = Client::new();
    client.initialize();
    client.open(
        &uri,
        "fn answer(value) { value + 1 }\nfn main() { answer(41) }",
    );
    assert_eq!(client.diagnostics(&uri, Some(1)), json!([]));
    let params = json!({"textDocument":{"uri":uri},"position":{"line":1,"character":14}});
    let hover = client.request(2, "textDocument/hover", params.clone());
    assert!(
        hover["result"]["contents"].to_string().contains("i32"),
        "{hover}"
    );
    let definition = client.request(3, "textDocument/definition", params);
    assert_eq!(definition["result"]["uri"], uri, "{definition}");
    assert_eq!(definition["result"]["range"]["start"]["line"], 0);
    let symbols = client.request(
        4,
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(symbols["result"].as_array().unwrap().len(), 2);
    client.change(&uri, 2, json!([{"text":"// 🦀\r\nfn main(){41}\r\n"}]));
    assert_eq!(client.diagnostics(&uri, Some(2)), json!([]));
    client.change(&uri,3,json!([{"range":{"start":{"line":1,"character":10},"end":{"line":1,"character":12}},"text":"not false"}]));
    assert_eq!(client.diagnostics(&uri, Some(3)), json!([]));
    client.change(&uri,4,json!([{"range":{"start":{"line":0,"character":4},"end":{"line":0,"character":4}},"text":"invalid surrogate edit"}]));
    client.change(&uri,4,json!([
        {"text":"fn main() { 0 }"},
        {"range":{"start":{"line":99,"character":0},"end":{"line":99,"character":0}},"text":"invalid batch"}
    ]));
    client.change(&uri, 2, json!([{"text":"stale"}]));
    let formatted = client.request(
        5,
        "textDocument/formatting",
        json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}),
    );
    assert!(
        formatted["result"][0]["newText"]
            .as_str()
            .unwrap()
            .contains("not false"),
        "{formatted}"
    );
    client.change(&uri, 5, json!([{"text":"fn main() { not }"}]));
    assert!(!client
        .diagnostics(&uri, Some(5))
        .as_array()
        .unwrap()
        .is_empty());
    client.change(&uri, 6, json!([{"text":"fn main() { true + 1 }"}]));
    assert!(!client
        .diagnostics(&uri, Some(6))
        .as_array()
        .unwrap()
        .is_empty());
    client.change(&uri, 7, json!([{"text":"fn main() { 42 }"}]));
    assert_eq!(client.diagnostics(&uri, Some(7)), json!([]));
    client.send(json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}));
    assert_eq!(client.diagnostics(&uri, None), json!([]));
    assert!(!dir.0.join("main.rill").exists());
    client.shutdown();
}

#[test]
fn unsaved_imports_refresh_dependents_and_resolve_definitions() {
    let dir = Directory::new();
    let entry = dir.uri("main.rill");
    let helper = dir.uri("helper.rill");
    let mut client = Client::new();
    client.initialize();
    client.open(&entry, "import helper; fn main() { helper.answer(41) }");
    assert!(!client
        .diagnostics(&entry, Some(1))
        .as_array()
        .unwrap()
        .is_empty());
    client.open(&helper, "export fn answer(value) { value + 1 }");
    assert_eq!(client.diagnostics(&entry, Some(1)), json!([]));
    assert_eq!(client.diagnostics(&helper, Some(1)), json!([]));
    let unrelated = dir.uri("missing-parent/scratch.rill");
    client.open(&unrelated, "fn main() { 0 }");
    assert_eq!(client.diagnostics(&entry, Some(1)), json!([]));
    assert_eq!(client.diagnostics(&helper, Some(1)), json!([]));
    assert!(!client
        .diagnostics(&unrelated, Some(1))
        .as_array()
        .unwrap()
        .is_empty());
    let result = client.request(
        2,
        "textDocument/definition",
        json!({"textDocument":{"uri":entry},"position":{"line":0,"character":35}}),
    );
    assert_eq!(result["result"]["uri"], helper, "{result}");
    let completion = client.request(
        3,
        "textDocument/completion",
        json!({"textDocument":{"uri":entry},"position":{"line":0,"character":34}}),
    );
    assert!(
        completion["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "answer"),
        "{completion}"
    );
    client.change(
        &helper,
        2,
        json!([{"text":"export fn answer(value) { true + value }"}]),
    );
    assert!(!client
        .diagnostics(&entry, Some(1))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!client
        .diagnostics(&helper, Some(2))
        .as_array()
        .unwrap()
        .is_empty());
    client.change(
        &helper,
        3,
        json!([{"text":"export fn answer(value) { value + 1 }"}]),
    );
    assert_eq!(client.diagnostics(&entry, Some(1)), json!([]));
    assert_eq!(client.diagnostics(&helper, Some(3)), json!([]));
    client.shutdown();
}

#[test]
fn sdl3_editing_requires_no_native_tools_or_library_execution() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sdl3-direct/main.rill");
    let uri = rill::lsp::text::path_to_uri(&entry).unwrap();
    let mut client = Client::new();
    client.initialize();
    let bindings_uri = rill::lsp::text::path_to_uri(&entry.with_file_name("sdl.rill")).unwrap();
    client.open(&bindings_uri, include_str!("fixtures/sdl3_manual.rill"));
    assert_eq!(client.diagnostics(&bindings_uri, Some(1)), json!([]));
    client.open(&uri, &fs::read_to_string(entry).unwrap());
    assert_eq!(client.diagnostics(&uri, Some(1)), json!([]));
    client.shutdown();
}

#[cfg(all(feature = "c-import", unix))]
#[test]
fn automatic_header_snapshots_supply_real_stdio_editor_features_without_native_tools() {
    let directory = Directory::new();
    let main_uri = directory.uri("main.rill");
    let entry = rill::lsp::text::uri_to_path(&main_uri).unwrap();
    let root = entry.parent().unwrap();
    fs::write(
        root.join("api.h"),
        "#include <stdint.h>\nuint32_t generated_count(uint32_t value);\n",
    )
    .unwrap();
    let native = "@include(\"api.h\");";
    let source = "import native as n; fn main() { let value = n.generated_count(u32(42)); value }";
    fs::write(root.join("native.rill"), native).unwrap();
    fs::write(&entry, source).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("check")
        .arg(&entry)
        .args(["--c-arg", "-I", "--c-arg"])
        .arg(root)
        .output()
        .unwrap();
    assert!(checked.status.success(), "{:?}", checked.stderr);

    // Client::new removes PATH and sets CC to a nonexistent executable.
    let mut client = Client::new();
    client.initialize();
    client.open(&main_uri, source);
    assert_eq!(client.diagnostics(&main_uri, Some(1)), json!([]));
    let at = source.find("generated_count").unwrap();
    let position = json!({"line":0,"character":at+2});
    let completion = client.request(
        2,
        "textDocument/completion",
        json!({"textDocument":{"uri":main_uri},"position":position}),
    );
    assert!(
        completion["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "generated_count"
                && item["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("u32"))),
        "{completion}"
    );
    let hover = client.request(
        3,
        "textDocument/hover",
        json!({"textDocument":{"uri":main_uri},"position":position}),
    );
    assert!(hover["result"].to_string().contains("u32"), "{hover}");
    let definition = client.request(
        4,
        "textDocument/definition",
        json!({"textDocument":{"uri":main_uri},"position":position}),
    );
    assert!(
        definition["result"].is_null(),
        "generated declarations must not point beyond a real file: {definition}"
    );
    let hints = client.request(5, "textDocument/inlayHint", json!({"textDocument":{"uri":main_uri},"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":source.len()}}}));
    assert!(
        hints["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hint| hint["label"] == ": u32"),
        "{hints}"
    );
    let signature = client.request(6, "textDocument/signatureHelp", json!({"textDocument":{"uri":main_uri},"position":{"line":0,"character":at+"generated_count(".len()}}));
    assert!(
        signature["result"].to_string().contains("u32"),
        "{signature}"
    );
    fs::write(
        root.join("api.h"),
        "#include <stdint.h>\nint32_t generated_count(int32_t value);\n",
    )
    .unwrap();
    client.change(&main_uri, 2, json!([{"text":source}]));
    assert!(!client
        .diagnostics(&main_uri, Some(2))
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        fs::read_to_string(root.join("native.rill")).unwrap(),
        native
    );
    assert_eq!(fs::read_to_string(entry).unwrap(), source);
    client.shutdown();
}

#[test]
fn contextual_export_names_select_the_name_instead_of_the_modifier() {
    for (source, start) in [
        ("export fn export() { 42 } fn main() { export() }", 10),
        ("export let export = 42; fn main() { export() }", 11),
    ] {
        let dir = Directory::new();
        let uri = dir.uri("main.rill");
        let mut client = Client::new();
        client.initialize();
        client.open(&uri, source);
        assert_eq!(client.diagnostics(&uri, Some(1)), json!([]));
        let result = client.request(
            2,
            "textDocument/definition",
            json!({
                "textDocument":{"uri":uri},
                "position":{"line":0,"character":source.rfind("export()").unwrap()+2}
            }),
        );
        assert_eq!(
            result["result"]["range"],
            json!({
                "start":{"line":0,"character":start},"end":{"line":0,"character":start+6}
            })
        );
        let symbols = client.request(
            3,
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        );
        assert_eq!(
            symbols["result"][0]["selectionRange"],
            result["result"]["range"]
        );
        client.shutdown();
    }
}

#[test]
fn stdio_hints_preserve_type_variables_and_returned_functions() {
    let mut client = Client::new();
    client.initialize();
    let uri = "untitled:lambda-vars";
    let source = "fn main() { let f = fn(x, y) { y }; 42 }";
    client.open(uri, source);
    let result = client.request(
        20,
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},
        "range":rill::lsp::text::range(source,0..source.len())}),
    );
    let end = source.find("y)").unwrap() + 2;
    assert!(result["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |hint| hint["position"] == rill::lsp::text::position(source, end)
                && hint["label"] == " -> 'b"
        ));
    let uri = "untitled:returned-function";
    let source = "fn main() { let f = fn(x) { fn() { x } }; f(42)() }";
    client.open(uri, source);
    let result = client.request(
        21,
        "textDocument/signatureHelp",
        json!({"textDocument":{"uri":uri},
        "position":rill::lsp::text::position(source,source.rfind("f(42").unwrap()+4)}),
    );
    assert_eq!(
        result["result"]["signatures"][0]["label"],
        "fn f(x: i32) -> fn() -> i32"
    );
    client.shutdown();
}

#[test]
fn record_member_completions_use_utf16_overlays_scopes_and_native_type_identity() {
    let directory = Directory::new();
    let main = directory.uri("main.rill");
    let left = directory.uri("left.rill");
    let right = directory.uri("right.rill");
    let mut client = Client::new();
    client.initialize();
    client.open(
        &left,
        "export type Point = { x: i32 } export fn make() { record Point { x = 42 } }",
    );
    client.open(&right, "@include(\"missing-header.h\"); @pkg_config(\"not-installed\"); export type Point = { ready: bool } export fn make() { record Point { ready = true } } export extern fn pointer() -> ptr<Point>;");
    let source = "// unsaved modules\r\nimport left; import right; fn main() { let text = \"😀\"; let p = right.make(); p. }";
    client.open(&main, source);
    let result = client.request(30, "textDocument/completion", json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(source, source.rfind("p.").unwrap() + 2)}));
    let items = result["result"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "ready" && item["detail"] == "ready: bool"),
        "{result}"
    );
    assert!(!items.iter().any(|item| item["label"] == "x"));

    let shadowed = "import left; import right; fn main() { let text = \"😀\"; let right = left.make(); right. }";
    client.change(&main, 2, json!([{"text":shadowed}]));
    let result = client.request(31, "textDocument/completion", json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(shadowed, shadowed.rfind("right.").unwrap() + 6)}));
    let items = result["result"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "x" && item["detail"] == "x: i32"),
        "{result}"
    );
    assert!(!items
        .iter()
        .any(|item| item["label"] == "ready" || item["label"] == "make"));

    let pointer = "import right; fn main() { let p = right.pointer(); p. }";
    client.change(&main, 3, json!([{"text":pointer}]));
    let result = client.request(32, "textDocument/completion", json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(pointer, pointer.rfind("p.").unwrap() + 2)}));
    assert_eq!(result["result"]["items"], json!([]));
    client.shutdown();
}

#[test]
fn stdio_nested_recovery_checks_complete_prefixes_without_typing_unfinished_bindings() {
    let directory = Directory::new();
    let main = directory.uri("main.rill");
    let helper = directory.uri("helper.rill");
    let mut client = Client::new();
    client.initialize();
    client.open(&helper, "export fn add(left, right) { left + right }");
    let source = "import helper as h;\r\nfn main() { let text = \"😀\"; { h.add(20)( } }";
    client.open(&main, source);
    let at = source.rfind('(').unwrap() + 1;
    let result = client.request(
        40,
        "textDocument/signatureHelp",
        json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(source,at)}),
    );
    assert_eq!(
        result["result"]["signatures"][0]["label"],
        "fn (right: i32) -> i32"
    );

    let nested = "import helper as h; type Holder<a> = { callback: a } type Outer<a> = { nested: a }\r\nfn main() { let text = \"😀\"; let h = record Outer { nested = record Holder { callback = fn(value) { value + 1 } } }; { h.nested.callback( } }";
    client.change(&main, 2, json!([{"text":nested}]));
    let at = nested.rfind("callback(").unwrap() + 9;
    let result = client.request(
        41,
        "textDocument/signatureHelp",
        json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(nested,at)}),
    );
    assert_eq!(
        result["result"]["signatures"][0]["label"],
        "fn h.nested.callback(i32) -> i32"
    );

    let unfinished = "fn main() { let text = \"😀\"; let outer = 1; { let value = val } }";
    client.change(&main, 3, json!([{"text":unfinished}]));
    let at = unfinished.rfind("val }").unwrap() + 3;
    let result = client.request(
        42,
        "textDocument/completion",
        json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(unfinished,at)}),
    );
    let items = result["result"]["items"].as_array().unwrap();
    assert!(items
        .iter()
        .any(|item| item["label"] == "outer" && item["detail"] == "let outer: i32"));
    assert!(!items.iter().any(|item| item["label"] == "value"));

    let shadowed = "fn main() { let text = \"😀\"; let value = 1; { let value = val } }";
    client.change(&main, 4, json!([{"text":shadowed}]));
    let at = shadowed.rfind("val }").unwrap() + 3;
    let result = client.request(
        43,
        "textDocument/completion",
        json!({"textDocument":{"uri":main},"position":rill::lsp::text::position(shadowed,at)}),
    );
    let items = result["result"]["items"].as_array().unwrap();
    assert!(items
        .iter()
        .any(|item| item["label"] == "value" && item["detail"] == "let value: i32"));
    assert!(!items.iter().any(|item| item["detail"] == "let value: unit"));
    client.shutdown();
}
