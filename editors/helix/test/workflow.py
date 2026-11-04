#!/usr/bin/env python3
"""Prove navigation, completion, formatting and syntax editing in actual Helix."""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import tomllib

sys.dont_write_bytecode = True
from terminal_screen import TerminalScreen


def response_pairs(trace, method, uri, after=0):
    """Match actual replies to this method, document and server process."""
    result = []
    for index, request in enumerate(trace[after:], after):
        message = request["message"]
        if (request["direction"] != "client" or message.get("method") != method
                or message.get("params", {}).get("textDocument", {}).get("uri") != uri):
            continue
        for response in trace[index + 1:]:
            reply = response["message"]
            if (response["direction"] == "server"
                    and response.get("server_pid") == request.get("server_pid")
                    and reply.get("id") == message.get("id")):
                if "result" in reply:
                    result.append((message, reply))
                break
    return result


def utf16_offset(source, position):
    lines = source.splitlines(keepends=True)
    prefix = "".join(lines[:position["line"]])
    line = lines[position["line"]] if position["line"] < len(lines) else ""
    width = position["character"] * 2
    return len(prefix) + len(line.encode("utf-16-le")[:width].decode("utf-16-le"))


def document_text(trace, uri):
    """Reconstruct unsaved editor text from didOpen and incremental didChange."""
    source = None
    for event in trace:
        message = event["message"]
        params = message.get("params", {})
        if event["direction"] != "client" or params.get("textDocument", {}).get("uri") != uri:
            continue
        if message.get("method") == "textDocument/didOpen":
            source = params["textDocument"]["text"]
        elif message.get("method") == "textDocument/didChange":
            assert source is not None, "didChange arrived before didOpen"
            for change in params["contentChanges"]:
                if "range" in change:
                    start = utf16_offset(source, change["range"]["start"])
                    end = utf16_offset(source, change["range"]["end"])
                    source = source[:start] + change["text"] + source[end:]
                else:
                    source = change["text"]
    return source


class Editor:
    def __init__(self, hx, root, output, environment, source):
        self.output = output
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 150, 0, 0))

        def terminal():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [hx, "--log", str(output / "helix.log"), "-vvv", str(source)],
            stdin=slave, stdout=slave, stderr=slave, cwd=root,
            env=environment, preexec_fn=terminal)
        os.close(slave)
        self.raw = bytearray()
        self.screen = TerminalScreen(40, 150)

    def send(self, keys):
        os.write(self.master, keys.encode() if isinstance(keys, str) else keys)

    def drain(self):
        if select.select([self.master], [], [], 0.05)[0]:
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                return
            self.raw.extend(chunk)
            self.screen.feed(chunk)
            if b"\x1b[6n" in chunk:
                self.send(b"\x1b[1;1R")
            if b"\x1b[c" in chunk:
                self.send(b"\x1b[?1;2c")

    def events(self):
        path = self.output / "lsp.jsonl"
        result = []
        if path.exists():
            for line in path.read_text().splitlines():
                try:
                    result.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # The proxy may be appending its final line.
        return result

    def wait(self, predicate, label, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.drain()
            if predicate():
                return
            if self.process.poll() is not None:
                assert predicate(), f"Helix exited while waiting for {label}"
                return
        raise AssertionError(f"timed out waiting for {label}; see {self.output}")

    def command(self, command):
        self.send(":" + command + "\r")

    def barrier(self, label):
        self.command("echo " + label)
        self.wait(lambda: self.screen.lines()[-1] == label, label)

    def open(self, source, row=1, column=1):
        self.command("open " + json.dumps(f"{source}:{row}:{column}"))
        self.wait(lambda: any(source.name in line and re.search(rf"\b{row}:{column}\s*$", line)
                              for line in self.screen.lines()), f"cursor {source.name}:{row}:{column}")
        self.barrier(f"opened-{source.stem}-{row}-{column}")

    def pairs(self, method, source, after=0):
        return response_pairs(self.events(), method, source.as_uri(), after)

    def text(self, source):
        return document_text(self.events(), source.as_uri())

    def snapshot(self, name):
        (self.output / (name + "-screen.txt")).write_text(self.screen.text() + "\n")

    def close(self):
        (self.output / "screen.ansi").write_bytes(self.raw)
        self.snapshot("final")
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait()
        os.close(self.master)


def enabled(value):
    return value is True or (isinstance(value, dict) and any(enabled(child) for child in value.values()))


def run(options, output):
    directory = Path(__file__).resolve().parent.parent
    root = directory.parent.parent
    spec = importlib.util.spec_from_file_location("rill_helix_setup", directory / "setup.py")
    setup = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(setup)
    empty = output / "source-config"
    empty.mkdir()
    config = output / "config/helix"
    proxy = output / "rill-proxy"
    shutil.copy2(directory / "test/lsp_proxy.py", proxy)
    proxy.chmod(0o700)
    setup.stage(root, options.source_config or empty, config, proxy, os.environ.get("CC", "cc"))
    config_file = config / "config.toml"
    config_before = config_file.read_bytes() if config_file.exists() else None
    config_options = tomllib.loads(config_before.decode()) if config_before else {}
    editor_options = config_options.get("editor", {})
    assert not enabled(editor_options.get("auto-save", False)), "disable copied auto-save or omit --source-config"
    environment = os.environ.copy()
    environment.update({
        "XDG_CONFIG_HOME": str(config.parent), "XDG_CACHE_HOME": str(output / "cache"),
        "RILL_REAL_SERVER": str(options.server.resolve()), "RILL_LSP_TRACE": str(output / "lsp.jsonl"),
        "TERM": "xterm-256color",
    })
    environment.pop("HELIX_RUNTIME", None)
    fixtures = {
        "main.rill": "import helper as h;\nfn identity(value) { value }\nfn add(left, right) { left + right }\nfn main() {\n    let local_value = identity(20);\n    let increment = add(1);\n    let transform = fn(value) {\n        value + local_value\n    };\n    let pipeline = increment >> transform;\n    let answer = 21 |> pipeline;\n    h.finish(answer)\n}\n",
        "helper.rill": "export fn finish(value) { value + 1 }\nfn private_helper() { 0 }\n",
        "completion.rill": "import helper as h;\nfn main() {\n    let local_value = 41;\n    0\n}\n",
        "record-completion.rill": "type Inner = { value: i32 }\ntype Outer<a> = { inner: Inner; callback: a }\nfn main() {\n    let state = record Outer { inner = record Inner { value = 41 }; callback = fn(value) { value + 1 } };\n    0\n}\n",
        "format.rill": "fn main(){0}\n",
        "indent.rill": "fn main() {\n    let f = fn(x) { x };\n    20 |>f >> f\n}\n",
        "textobjects.rill": "fn named(x) { x }\nfn empty() {}\nfn main() {\n    let outer = fn(x) {\n        let inner = fn(y) { y + x };\n        inner\n    };\n    outer(20)(22)\n}\n",
        "rhs.rill": "fn main() {\n    let f = fn(x) { x };\n    let answer =20 |> f;\n    answer\n}\n",
        "lambda-indent.rill": "fn main() {\n    let f =\n        fn(x) {\n            let value = x;\n            value\n        };\n    f(42)\n}\n",
    }
    for name, source in fixtures.items():
        (output / name).write_text(source)
    main = output / "main.rill"
    helper = output / "helper.rill"
    completion = output / "completion.rill"
    formatting = output / "format.rill"
    indent = output / "indent.rill"
    textobjects = output / "textobjects.rill"
    summary = {"editor": "Helix", "navigation": [], "highlights": [], "completion": []}
    editor = Editor(options.hx, root, output, environment, main)
    try:
        editor.wait(lambda: editor.text(main) == fixtures[main.name], "initial didOpen")
        # Runtime options keep the copied user configuration byte-for-byte intact.
        for command in ["set auto-completion false", "set lsp.display-inlay-hints false",
                        "set indent-heuristic tree-sitter", "indent-style 4"]:
            editor.command(command)
        editor.barrier("session-options-ready")

        for row, column, scope in [(2, 4, "function"), (6, 21, "function"), (7, 21, "keyword"),
                                   (10, 30, "operator"), (11, 21, "operator")]:
            editor.open(main, row, column)
            checkpoint = len(editor.raw)
            editor.command("tree-sitter-highlight-name")
            editor.wait(lambda: scope.encode() in editor.raw[checkpoint:], "rendered " + scope)
            editor.snapshot(f"highlight-{row}")
            summary["highlights"].append({"line": row, "column": column, "scope": scope})

        for row, column, target, target_row, target_column in [
                (5, 23, main, 2, 4), (6, 21, main, 3, 4), (11, 24, main, 10, 9),
                (12, 7, helper, 1, 11)]:
            editor.open(main, row, column)
            checkpoint = len(editor.events())
            editor.send("gd")
            editor.wait(lambda: bool(editor.pairs("textDocument/definition", main, checkpoint)), "definition response")
            request, reply = editor.pairs("textDocument/definition", main, checkpoint)[-1]
            location = reply["result"]
            assert location["uri"] == target.as_uri(), reply
            assert location["range"]["start"] == {"line": target_row - 1, "character": target_column - 1}, reply
            editor.wait(lambda: any(target.name in line and re.search(rf"\b{target_row}:{target_column}\s*$", line)
                                    for line in editor.screen.lines()), "definition destination on screen")
            editor.snapshot(f"definition-{row}")
            summary["navigation"].append({"request_id": request["id"], "from_line": row, "target": target.name,
                                           "position": location["range"]["start"]})

        editor.open(main, 12, 7)
        checkpoint = len(editor.events())
        hover_keys = "K" if config_options.get("keys", {}).get("normal", {}).get("K") == "hover" else " k"
        editor.send(hover_keys)
        editor.wait(lambda: bool(editor.pairs("textDocument/hover", main, checkpoint)), "hover response")
        request, reply = editor.pairs("textDocument/hover", main, checkpoint)[-1]
        hover = "fn finish(value: i32) -> i32"
        assert hover in reply["result"]["contents"]["value"], reply
        editor.wait(lambda: editor.screen.styled_text(hover, standalone=True) is not None, "typed hover popup")
        editor.snapshot("hover")
        summary["hover"] = {"request_id": request["id"], "rendered": hover, "keys": hover_keys.strip() if hover_keys == "K" else "Space k"}
        editor.send(b"\x1b")
        editor.wait(lambda: editor.screen.styled_text(hover, standalone=True) is None, "hover popup dismissal")
        editor.barrier("hover-dismissed")

        editor.open(main, 12, 1)
        checkpoint = len(editor.events())
        editor.send(" s")
        editor.wait(lambda: bool(editor.pairs("textDocument/documentSymbol", main, checkpoint)), "document-symbol response")
        request, reply = editor.pairs("textDocument/documentSymbol", main, checkpoint)[-1]
        assert {item["name"] for item in reply["result"]} == {"identity", "add", "main"}, reply
        editor.send("add")
        editor.wait(lambda: any("add" in line and "function" in line.lower() for line in editor.screen.lines()), "filtered document-symbol picker")
        editor.snapshot("symbols")
        editor.send(b"\r")
        editor.wait(lambda: any(main.name in line and re.search(r"\b3:4\s*$", line)
                                for line in editor.screen.lines()), "symbol-picker navigation")
        summary["symbols"] = {"request_id": request["id"], "selected": "add", "position": {"line": 2, "character": 3}}

        record_completion = output / "record-completion.rill"
        for completion, row, prefix, label, expected, detail in [
                (completion, 4, "loc", "local_value", "local_value", None),
                (completion, 4, "h.fi", "finish", "h.finish", None),
                (record_completion, 5, "state.inner.va", "value", "state.inner.value", "value: i32")]:
            editor.open(completion, row, 5)
            editor.send(b"\x1bc" + prefix.encode())  # Change without touching any clipboard/register.
            editor.wait(lambda: f"    {prefix}\n" in (editor.text(completion) or ""), "unsaved completion prefix")
            checkpoint = len(editor.events())
            editor.send(b"\x18")  # Helix insert-mode Ctrl-x requests completion.
            editor.wait(lambda: bool(editor.pairs("textDocument/completion", completion, checkpoint)), "manual completion response")
            request, reply = editor.pairs("textDocument/completion", completion, checkpoint)[-1]
            choices = reply["result"].get("items", []) if isinstance(reply["result"], dict) else reply["result"]
            assert any(item["label"] == label for item in choices), reply
            if detail:
                assert any(item["label"] == label and item.get("detail") == detail and item.get("kind") == 5
                           for item in choices), reply
            assert not any(item["label"] == "private_helper" for item in choices), reply
            editor.wait(lambda: any(label in line and any(kind in line.lower() for kind in ["variable", "function", "field"])
                                    for line in editor.screen.lines()), "visible completion menu")
            editor.snapshot("completion-" + label)
            editor.send(b"\t\r\x1b")
            expected_source = fixtures[completion.name].replace("    0\n", f"    {expected}\n")
            editor.wait(lambda: editor.text(completion) == expected_source, "accepted completion edit")
            editor.wait(lambda: editor.screen.lines()[-2].startswith(" NOR"), "normal mode after completion")
            editor.barrier("accepted-" + label)
            editor.snapshot("accepted-" + label)
            summary["completion"].append({"request_id": request["id"], "prefix": prefix, "selected": label,
                                           "buffer_line": expected, **({"detail": detail} if detail else {})})
            editor.send("u")
            prefix_source = fixtures[completion.name].replace("    0\n", f"    {prefix}\n")
            editor.wait(lambda: editor.text(completion) in (prefix_source, fixtures[completion.name]), "completion acceptance undo")
            if editor.text(completion) == prefix_source:
                editor.send("u")
            editor.wait(lambda: editor.text(completion) == fixtures[completion.name], "completion undo")

        editor.open(formatting)
        unsaved = "fn main(){let answer=40+2;answer}\n"
        editor.send(b"%\x1bc" + unsaved.rstrip("\n").encode() + b"\x1b")
        editor.wait(lambda: editor.text(formatting) == unsaved, "unsaved formatting input")
        editor.wait(lambda: editor.screen.lines()[-2].startswith(" NOR"), "normal mode before formatting")
        checkpoint = len(editor.events())
        editor.command("format")
        editor.wait(lambda: bool(editor.pairs("textDocument/formatting", formatting, checkpoint)), "formatting response")
        request, reply = editor.pairs("textDocument/formatting", formatting, checkpoint)[-1]
        assert len(reply["result"]) == 1, reply
        formatted = reply["result"][0]["newText"]
        assert "let answer = 40 + 2;" in formatted and formatted != unsaved, reply
        editor.wait(lambda: editor.text(formatting) == formatted, "formatted unsaved buffer")
        editor.wait(lambda: "let answer = 40 + 2;" in editor.screen.text(), "formatted source on screen")
        editor.snapshot("formatted")
        summary["formatting"] = {"request_id": request["id"], "input": "unsaved buffer", "formatted": formatted}

        editor.open(main, 8, 10)
        editor.send(b"maf\x1bd")
        lambda_text = "fn(value) {\n        value + local_value\n    }"
        without_lambda = fixtures[main.name].replace(lambda_text, "")
        editor.wait(lambda: editor.text(main) == without_lambda, "lambda function textobject selection")
        editor.snapshot("lambda-textobject")
        editor.send("u")
        editor.wait(lambda: editor.text(main) == fixtures[main.name], "lambda textobject undo")
        summary["lambda_textobject"] = {"keys": "maf Alt-d", "selected": lambda_text, "undo": "restored exact buffer"}

        summary["function_textobjects"] = []
        for name, row, column, keys, selected in [
                ("named-inside", 1, 15, "mif", "{ x }"),
                ("empty-inside", 2, 12, "mif", "{}"),
                ("nested-lambda-inside", 5, 31, "mif", "{ y + x }"),
                ("nested-lambda-around", 5, 31, "maf", "fn(y) { y + x }"),
                ("outer-lambda-around", 6, 10, "maf", "fn(x) {\n        let inner = fn(y) { y + x };\n        inner\n    }")]:
            editor.open(textobjects, row, column)
            editor.send(keys.encode() + b"\x1bd")
            expected = fixtures[textobjects.name].replace(selected, "", 1)
            editor.wait(lambda: editor.text(textobjects) == expected, name + " exact selection")
            editor.snapshot(name)
            summary["function_textobjects"].append({"case": name, "keys": keys + " Alt-d", "selected": selected})
            editor.send("u")
            editor.wait(lambda: editor.text(textobjects) == fixtures[textobjects.name], name + " undo")

        editor.open(indent, 3, 9)
        editor.send(b"a\r\x1b")
        indented = fixtures[indent.name].replace("    20 |>f >> f\n", "    20 |>\n        f >> f\n")
        editor.wait(lambda: editor.text(indent) == indented, "pipeline continuation indentation")
        editor.snapshot("indent")
        rhs = output / "rhs.rill"
        editor.open(rhs, 3, 16)
        editor.send(b"a\r\x1b")
        rhs_indented = fixtures[rhs.name].replace("    let answer =20 |> f;\n", "    let answer =\n        20 |> f;\n")
        editor.wait(lambda: editor.text(rhs) == rhs_indented, "local RHS continuation indentation")
        editor.snapshot("rhs-indent")
        lambda_indent = output / "lambda-indent.rill"
        editor.open(lambda_indent, 4, 1)
        editor.send(b"ovalue;\x1b")
        lambda_indented = fixtures[lambda_indent.name].replace("            let value = x;\n", "            let value = x;\n            value;\n")
        editor.wait(lambda: editor.text(lambda_indent) == lambda_indented, "multiline lambda body indentation")
        editor.snapshot("lambda-indent")
        summary["indentation"] = {"heuristic": "tree-sitter", "pipeline_spaces": 8, "local_rhs_spaces": 8,
                                  "multiline_lambda_body_spaces": 12}

        editor.command("qa!")
        editor.wait(lambda: editor.process.poll() is not None, "Helix shutdown")
        assert editor.process.wait(timeout=5) == 0
        trace = editor.events()
        shutdown = next(event["message"] for event in trace if event["message"].get("method") == "shutdown")
        assert any(event["direction"] == "server" and event["message"].get("id") == shutdown["id"]
                   and "result" in event["message"] and event["message"]["result"] is None for event in trace)
        server_pids = [event["message"]["params"]["pid"] for event in trace
                       if event["message"].get("method") == "$test/serverPid"]

        def server_exists(pid):
            try:
                os.kill(pid, 0)
                return True
            except ProcessLookupError:
                return False

        deadline = time.monotonic() + 3
        while any(server_exists(pid) for pid in server_pids) and time.monotonic() < deadline:
            editor.drain()
            time.sleep(0.02)
        assert server_pids and not any(server_exists(pid) for pid in server_pids), "Rill server survived editor shutdown"
        for name, source in fixtures.items():
            assert (output / name).read_text() == source, f"editor wrote {name} to disk"
        assert (config_file.read_bytes() if config_file.exists() else None) == config_before
        assert not any(event["message"].get("method") == "textDocument/didSave" for event in trace)
        summary.update({"disk_sources": "all unchanged", "copied_config": "unchanged", "didSave": False,
                        "shutdown": "reply received; editor and server exited cleanly"})
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"Helix real-editor workflow passed; artifacts: {output}")
    finally:
        editor.close()


def main():
    root = Path(__file__).resolve().parents[3]
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument("--hx", default=os.environ.get("HX", "hx"))
    arguments.add_argument("--server", type=Path, default=root / "target/debug/rill")
    arguments.add_argument("--source-config", type=Path)
    arguments.add_argument("--output", type=Path)
    options = arguments.parse_args()
    if shutil.which(options.hx) is None:
        print("SKIP Helix workflow: hx required")
        return
    if options.output:
        output = options.output.resolve()
        if output.exists() and any(output.iterdir()):
            arguments.error("--output must name a new or empty directory")
        output.mkdir(parents=True, exist_ok=True)
        run(options, output)
    else:
        with tempfile.TemporaryDirectory(prefix="rill helix workflow ") as temporary:
            run(options, Path(temporary).resolve())


if __name__ == "__main__":
    main()
