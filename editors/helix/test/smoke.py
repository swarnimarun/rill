#!/usr/bin/env python3
"""Exercise actual Helix, Tree-sitter and Rill LSP over a private PTY."""
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
from terminal_screen import TerminalScreen


def main():
    directory = Path(__file__).resolve().parent.parent
    root = directory.parent.parent
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument("--hx", default=os.environ.get("HX", "hx"))
    arguments.add_argument("--server", type=Path, default=root / "target/debug/rill")
    arguments.add_argument("--source-config", type=Path, help="copy an existing Helix configuration into the isolated test")
    arguments.add_argument("--output", type=Path)
    options = arguments.parse_args()
    if shutil.which(options.hx) is None:
        print("SKIP Helix integration: hx required")
        return
    spec = importlib.util.spec_from_file_location("rill_helix_setup", directory / "setup.py")
    setup = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(setup)
    temporary = tempfile.TemporaryDirectory(prefix="rill helix smoke ") if options.output is None else None
    output = Path(temporary.name) if temporary else options.output.resolve()
    if not temporary and output.exists() and any(output.iterdir()):
        arguments.error("--output must name a new or empty directory so old LSP traces cannot pass the test")
    output.mkdir(parents=True, exist_ok=True)
    config = output / "config/helix"
    empty = output / "source-config"
    empty.mkdir(exist_ok=True)
    proxy = output / "rill-proxy"
    shutil.copy2(directory / "test/lsp_proxy.py", proxy)
    proxy.chmod(0o700)
    setup.stage(root, options.source_config or empty, config, proxy, os.environ.get("CC", "cc"))
    editor_file = config / "config.toml"
    editor_before = editor_file.read_bytes() if editor_file.exists() else None
    editor_options = {}
    if editor_file.exists():
        editor_options = tomllib.loads(editor_file.read_text()).get("editor", {})
        auto_save = editor_options.get("auto-save", False)

        def enabled(value):
            return value is True or (isinstance(value, dict) and any(enabled(child) for child in value.values()))

        assert not enabled(auto_save), "the smoke edits the real example; disable auto-save or omit --source-config"
    environment = os.environ.copy()
    environment.update({
        "XDG_CONFIG_HOME": str(config.parent), "XDG_CACHE_HOME": str(output / "cache"),
        "RILL_REAL_SERVER": str(options.server.resolve()), "RILL_LSP_TRACE": str(output / "lsp.jsonl"),
        "TERM": "xterm-256color",
    })
    environment.pop("HELIX_RUNTIME", None)
    health = subprocess.run([options.hx, "--log", str(output / "health.log"), "--health", "rill"], env=environment, capture_output=True, text=True, check=True)
    (output / "health.txt").write_text(health.stdout)
    for title in ["Tree-sitter parser", "Highlight queries", "Textobject queries", "Indent queries"]:
        assert any(line.startswith(title + ":") and "✓" in line for line in health.stdout.splitlines()), health.stdout
    source = root / "examples/sdl3-direct/main.rill"
    before = source.read_bytes()
    first_function = next(index for index, line in enumerate(before.decode().splitlines(), 1) if line.startswith("fn "))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 140, 0, 0))

    def terminal():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

    process = subprocess.Popen([options.hx, "--log", str(output / "helix.log"), "-vvv", f"{source}:{first_function}:1"], stdin=slave, stdout=slave, stderr=slave, cwd=root, env=environment, preexec_fn=terminal)
    os.close(slave)
    screen = bytearray()
    visible = TerminalScreen(36, 140)

    def drain():
        if select.select([master], [], [], 0.05)[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                return
            screen.extend(chunk)
            visible.feed(chunk)
            if b"\x1b[6n" in chunk:
                os.write(master, b"\x1b[1;1R")
            if b"\x1b[c" in chunk:
                os.write(master, b"\x1b[?1;2c")

    def events():
        trace = output / "lsp.jsonl"
        if not trace.exists():
            return []
        result = []
        for line in trace.read_text().splitlines():
            try:
                result.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return result

    def wait(predicate, label, timeout=15):
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            drain()
            if predicate():
                return
            if process.poll() is not None:
                assert predicate(), f"Helix exited while waiting for {label}"
                return
        raise AssertionError(f"timed out waiting for {label}; see {output}")

    def has_method(name):
        return any(event["message"].get("method") == name for event in events())

    def cursor_at(path, row, column):
        return any(path.name in line and re.search(rf"\b{row}:{column}\s*$", line)
                   for line in visible.lines())

    def inlay_requests_settled():
        pending = set()
        for event in events():
            message = event["message"]
            identity = (event.get("server_pid"), message.get("id"))
            if event["direction"] == "client" and message.get("method") == "textDocument/inlayHint":
                pending.add(identity)
            elif event["direction"] == "server" and "id" in message:
                pending.discard(identity)
        return not pending

    def responses(method, uri, after=0, parameter=None):
        trace = events()
        result = []
        for index, request in enumerate(trace[after:], after):
            message = request["message"]
            if request["direction"] != "client" or message.get("method") != method or message.get("params", {}).get("textDocument", {}).get("uri") != uri:
                continue
            for response in trace[index + 1:]:
                reply = response["message"]
                if response["direction"] == "server" and response.get("server_pid") == request.get("server_pid") and reply.get("id") == message.get("id") and reply.get("result"):
                    if parameter is None or reply["result"].get("activeParameter") == parameter:
                        result.append((message, reply))
                    break
        return result

    try:
        wait(lambda: has_method("textDocument/didOpen"), "actual SDL3 didOpen")
        # These session options also exercise hints with an empty source config.
        # The copied settings file is never modified.
        for option in ["display-inlay-hints", "auto-signature-help"]:
            if not editor_options.get("lsp", {}).get(option, option == "auto-signature-help"):
                os.write(master, f":set lsp.{option} true\r".encode())
        os.write(master, b":tree-sitter-highlight-name\r")
        wait(lambda: b"keyword" in screen, "rendered keyword highlight")
        row, column = next((index, match.start() + 1) for index, line in enumerate(before.decode().splitlines(), 1) if (match := re.search(r"\b20\b", line)))
        checkpoint = len(screen)
        os.write(master, f":open {json.dumps(str(source) + ':' + str(row) + ':' + str(column))}\r".encode())
        time.sleep(0.1)
        os.write(master, b":tree-sitter-highlight-name\r")
        wait(lambda: b"constant.numeric" in screen[checkpoint:], "rendered numeric highlight")
        os.write(master, b"geA?")
        time.sleep(0.15)
        os.write(master, b"\x1b")

        def diagnostics(nonempty):
            return [index for index, event in enumerate(events()) if event["message"].get("method") == "textDocument/publishDiagnostics" and bool(event["message"]["params"]["diagnostics"]) == nonempty]

        wait(lambda: bool(diagnostics(True)), "diagnostic after unsaved invalid edit")
        bad = diagnostics(True)[-1]
        os.write(master, b"u")
        wait(lambda: any(index > bad for index in diagnostics(False)), "diagnostic clearing after undo")
        fixture = output / "capture.rill"
        fixture_text = 'import helper as h;\ntype State = { count: i32 }\nfn main() {\n    let value = record State { count = 42 };\n    let text = "line\\n";\n    h.identity(value.count)\n}\n'
        fixture.write_text(fixture_text)
        (output / "helper.rill").write_text('export fn identity(value) { value }\n')
        scope_checks = [
            (4, fixture_text.splitlines()[3].index("42") + 1, "constant.numeric"),
            (6, fixture_text.splitlines()[5].index("count") + 1, "variable.other.member"),
            (6, fixture_text.splitlines()[5].index("h.") + 1, "namespace"),
            (5, fixture_text.splitlines()[4].index("\\n") + 1, "constant.character.escape"),
        ]
        for row, column, scope in scope_checks:
            checkpoint = len(screen)
            os.write(master, f":open {json.dumps(str(fixture) + ':' + str(row) + ':' + str(column))}\r".encode())
            time.sleep(0.1)
            os.write(master, b":tree-sitter-highlight-name\r")
            wait(lambda: scope.encode() in screen[checkpoint:], "rendered " + scope + " highlight")
        wait(inlay_requests_settled, "previous inlay-hint requests to finish")
        functions = output / "functions.rill"
        functions_text = "fn add(x, y) { x + y }\nfn main() {\n    let answer = add(20,22);\n    answer\n}\n"
        functions.write_text(functions_text)
        functions_uri = functions.as_uri()
        event_checkpoint = len(events())
        os.write(master, f":open {json.dumps(str(functions) + ':3:1')}\r".encode())
        wait(lambda: cursor_at(functions, 3, 1), "function fixture cursor after open")
        # Helix 25.07.1's command prompt does not rearm its one-shot idle
        # timer. Normal editor motions do, even if :open outlasted the timer.
        os.write(master, b"l")
        wait(lambda: cursor_at(functions, 3, 2), "normal-mode hint refresh motion")
        os.write(master, b"h")
        wait(lambda: cursor_at(functions, 3, 1), "hint refresh cursor restoration")
        wait(lambda: bool(responses("textDocument/inlayHint", functions_uri, event_checkpoint)), "nonempty inlayHint response for function fixture")
        hint_request, hint_reply = responses("textDocument/inlayHint", functions_uri, event_checkpoint)[-1]
        answer_end = len("    let answer")
        assert any(hint.get("label") == ": i32" and hint.get("position") == {"line": 2, "character": answer_end} for hint in hint_reply["result"]), hint_reply
        wait(lambda: "answer: i32" in visible.text(), "rendered inline inferred answer type")
        (output / "inlay-screen.txt").write_text(visible.text() + "\n")
        popup_label = "fn add(x: i32, y: i32) -> i32"

        def parameter_styles():
            styles = visible.styled_text(popup_label, standalone=True)
            if styles:
                return [styles[popup_label.index(parameter)] for parameter in ["x: i32", "y: i32"]]

        # Completion and signature help share Helix's popup area. Keep this
        # phase about automatic signature rendering; workflow.py separately
        # proves real completion requests, menus and accepted edits.
        os.write(master, b":set auto-completion false\r")
        column = functions_text.splitlines()[2].index("add(") + len("add") + 1
        os.write(master, f":open {json.dumps(str(functions) + ':3:' + str(column))}\r".encode())
        wait(lambda: cursor_at(functions, 3, column), "signature fixture cursor after open")
        first_checkpoint = len(events())
        os.write(master, b"i(")
        wait(lambda: bool(responses("textDocument/signatureHelp", functions_uri, first_checkpoint, 0)), "signatureHelp response for first parameter")
        wait(lambda: parameter_styles() and parameter_styles()[0] != parameter_styles()[1], "rendered signature-help popup emphasizing first parameter")
        first_request, first_reply = responses("textDocument/signatureHelp", functions_uri, first_checkpoint, 0)[-1]
        first_styles = parameter_styles()
        (output / "signature-first-screen.txt").write_text(visible.text() + "\n")
        second_checkpoint = len(events())
        os.write(master, b"20,")
        wait(lambda: bool(responses("textDocument/signatureHelp", functions_uri, second_checkpoint, 1)), "signatureHelp response advancing to second parameter")
        wait(lambda: parameter_styles() == list(reversed(first_styles)), "rendered second-parameter popup emphasis")
        second_request, second_reply = responses("textDocument/signatureHelp", functions_uri, second_checkpoint, 1)[-1]
        assert not any(event["direction"] == "client"
                       and event["message"].get("method") == "textDocument/completion"
                       and event["message"].get("params", {}).get("textDocument", {}).get("uri") == functions_uri
                       for event in events()[first_checkpoint:]), "signature proof received a competing completion request"
        (output / "signature-second-screen.txt").write_text(visible.text() + "\n")
        (output / "signature-styles.json").write_text(json.dumps({"parameters": ["x: i32", "y: i32"], "first": first_styles, "second": parameter_styles()}, indent=2) + "\n")
        os.write(master, b"\x1b")
        time.sleep(0.1)
        os.write(master, b":q!\r")
        wait(lambda: process.poll() is not None, "Helix shutdown")
        assert process.wait(timeout=5) == 0
        assert before == source.read_bytes(), "editor test changed the SDL3 source on disk"
        assert functions.read_text() == functions_text, "editor saved temporary signature-help edits"
        assert (editor_file.read_bytes() if editor_file.exists() else None) == editor_before, "editor changed the copied configuration"
        assert has_method("initialize") and has_method("initialized")
        shutdown = next(event["message"] for event in events() if event["message"].get("method") == "shutdown")
        assert any(event["direction"] == "server" and event["message"].get("id") == shutdown["id"] and "result" in event["message"] and event["message"]["result"] is None for event in events()), "missing shutdown response"
        assert '"method":"exit"' in (output / "helix.log").read_text(), "Helix did not request exit"
        server_pid = next(event["message"]["params"]["pid"] for event in events() if event["message"].get("method") == "$test/serverPid")

        def server_exists():
            try:
                os.kill(server_pid, 0)
                return True
            except ProcessLookupError:
                return False

        deadline = time.monotonic() + 3
        while server_exists() and time.monotonic() < deadline:
            drain()
            time.sleep(0.02)
        assert not server_exists(), "Rill server survived editor shutdown"
        (output / "summary.json").write_text(json.dumps({"editor": "Helix", "health": "all Rill checks passed", "highlights": ["keyword", *[scope for _, _, scope in scope_checks]], "diagnostics": "published and cleared", "inlay_hints": {"request_id": hint_request["id"], "count": len(hint_reply["result"]), "rendered": "answer: i32", "refresh": "acknowledged normal cursor 3:1 -> 3:2 -> 3:1"}, "signature_help": {"request_ids": [first_request["id"], second_request["id"]], "active_parameters": [first_reply["result"]["activeParameter"], second_reply["result"]["activeParameter"]], "rendered": popup_label, "emphasis": "advanced to second parameter", "auto_completion": "disabled for this signature-render phase; completion acceptance tested separately"}, "disk_source": "unchanged", "copied_config": "unchanged", "shutdown": "response received; editor and server exited", "exit_notification_forwarded": has_method("exit")}, indent=2) + "\n")
        print(f"Helix real-editor smoke passed; artifacts: {output}")
    finally:
        (output / "screen.ansi").write_bytes(screen)
        (output / "final-screen.txt").write_text(visible.text() + "\n")
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        os.close(master)
        if temporary:
            temporary.cleanup()


if __name__ == "__main__":
    main()
