#!/usr/bin/env python3
"""Exercise the generated parser and actual highlight rendering with the CLI."""
import html.parser
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

GRAMMAR = Path(__file__).resolve().parents[1]
ROOT = GRAMMAR.parents[1]


def isolated_config(directory):
    config = directory / "tree-sitter.json"
    config.write_text(json.dumps({"parser-directories": [str(GRAMMAR.parent)]}))
    return config


class Highlights(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.spans = []
        self.tokens = []

    def handle_starttag(self, tag, attrs):
        if tag == "span":
            self.spans.append(dict(attrs).get("class", ""))

    def handle_endtag(self, tag):
        if tag == "span":
            self.spans.pop()

    def handle_data(self, value):
        if self.spans:
            self.tokens.append((value, self.spans[-1]))


def main():
    if shutil.which("tree-sitter") is None:
        raise SystemExit("tree-sitter CLI is required; see README.md")
    with tempfile.TemporaryDirectory(prefix="rill-tree-sitter-") as directory:
        temporary = Path(directory)
        env = dict(os.environ, XDG_CACHE_HOME=str(temporary / "cache"))
        config = isolated_config(temporary)

        def cli(*args, valid=True):
            if args[0] in ("parse", "query", "highlight", "tags"):
                args = (*args, "--config-path", str(config))
            result = subprocess.run(
                ["tree-sitter", *args], cwd=GRAMMAR, env=env,
                capture_output=True, text=True, timeout=60,
            )
            if valid and result.returncode:
                raise AssertionError(f"{args}:\n{result.stdout}\n{result.stderr}")
            if not valid and result.returncode == 0:
                raise AssertionError(f"unsupported syntax parsed without errors: {args}")
            return result.stdout

        if "--check-generated" in sys.argv:
            generated = temporary / "generated"
            cli("generate", "--abi", "15", "--output", str(generated), str(GRAMMAR / "grammar.js"))
            for source in generated.rglob("*"):
                if source.is_file():
                    checked_in = GRAMMAR / "src" / source.relative_to(generated)
                    assert checked_in.read_bytes() == source.read_bytes(), f"regenerate {checked_in}"

        cli("test")
        sources = sorted(ROOT.glob("examples/**/*.rill")) + sorted(ROOT.glob("stdlib/**/*.rill"))
        sources += sorted(ROOT.glob("tests/fixtures/**/*.rill"))
        assert sources, "repository source corpus is missing"
        cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", *map(str, sources))

        fixture = temporary / "highlights.rill"
        fixture.write_text('''// syntax colors
@include("SDL3/SDL.h");
export extern fn SDL_Init(flags: u32) -> bool;
type Point = { x: i32 }
fn main() {
    let mut p = record Point { x = 42 };
    let transform = fn(read input: Option<Option<i32>>) { input };
    transform 1;
    (callback)(1);
    1 |> transform >> callback;
    callback << transform <| 1;
    not SDL_Init(u32(0)) && !false
}
export fn answer() -> i32 { 42 }
''')
        for query in ["highlights", "locals", "tags"]:
            cli("query", "--grammar-path", str(GRAMMAR), "--quiet",
                str(GRAMMAR / "queries" / f"{query}.scm"), str(fixture))
        tags = cli("tags", "--grammar-path", str(GRAMMAR), str(fixture))
        assert "SDL_Init" in tags and "main" in tags and "Point" in tags, tags
        assert "transform" in tags and "callback" in tags, tags
        locals_output = cli("query", "--grammar-path", str(GRAMMAR),
                            str(GRAMMAR / "queries" / "locals.scm"), str(fixture))
        local_captures = locals_output.splitlines()
        assert any("local.scope" in line and "text: `fn(read input" in line
                   for line in local_captures), locals_output
        assert any("local.definition" in line and "text: `input`" in line
                   for line in local_captures), locals_output
        rendered = cli("highlight", "--grammar-path", str(GRAMMAR), "--html",
                       "--layout", "fragment", "--style", "minimal", str(fixture))
        highlights = Highlights()
        highlights.feed(rendered)
        for token, capture in [
            ("// syntax colors", "comment"), ("include", "attribute"),
            ("fn", "keyword"), ("main", "function"), ("SDL_Init", "function"),
            ("flags", "variable parameter"), ("u32", "type builtin"),
            ("Point", "type"), ("x", "property"), ("42", "number"),
            ("not", "operator"), ("!", "operator"), ("false", "constant builtin"),
            ("transform", "function"), ("input", "variable parameter"),
            ("callback", "function"), ("|>", "operator"), ("<|", "operator"),
            (">>", "operator"), ("<<", "operator"),
        ]:
            assert (token, capture) in highlights.tokens, (token, capture, highlights.tokens)
        assert any(capture == "string" for _, capture in highlights.tokens)

        original = "fn main() { false }\n"
        edited = original[:12] + "not " + original[12:]
        fixture.write_text(original)
        incremental = cli("parse", "--grammar-path", str(GRAMMAR), "--no-ranges",
                          str(fixture), "--edits", "0,12 0 not ")
        fixture.write_text(edited)
        fresh = cli("parse", "--grammar-path", str(GRAMMAR), "--no-ranges", str(fixture))
        assert incremental == fresh, (incremental, fresh)
        assert "unary_expression" in fresh

        # Removing the space changes an argument into an adjacent postfix call.
        original = "fn main() { f x (y) }\n"
        edited = original.replace("x (", "x(")
        fixture.write_text(original)
        incremental = cli("parse", "--grammar-path", str(GRAMMAR), "--no-ranges",
                          str(fixture), "--edits", "0,15 1 ")
        fixture.write_text(edited)
        fresh = cli("parse", "--grammar-path", str(GRAMMAR), "--no-ranges", str(fixture))
        assert incremental == fresh, (incremental, fresh)
        assert "call_expression" in fresh

        fixture.write_text('fn main() { "a\x00b" }')
        cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture))

        for keyword in [
            "let", "mut", "move", "inout", "read", "record", "extern", "const", "fn", "type",
            "match", "while", "do", "in", "if", "then", "else", "true", "false", "not",
        ]:
            fixture.write_text(f"fn {keyword}() {{ 0 }}")
            cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture), valid=False)
        for name in ["import", "export", "as", "include", "link", "pkg_config", "ptr", "not_ready"]:
            fixture.write_text(f"fn {name}({name}) {{ {name} }}")
            cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture))

        for source in [
            "fn main() { if true { () } }",
            "fn main() { if true { 42 } }",
            "fn main() { if true { if false { () }; }; () }",
            "fn main() { if true { () } else { () } }",
            "fn main() { foo.bar.baz() }",
            "fn main() { 1 2 }",
            "fn main() { if f x { 1 } else { 0 } }",
            "fn main() { while f x { () } }",
            "fn main() { f x // break adjacency\n(y) }",
        ]:
            fixture.write_text(source)
            cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture))

        for source in [
            "fn main() { if true then () }",
            "fn main() { let x = 1 in x }",
            "fn main() { let x = 1 x }",
            "fn main() -> { 1 }",
            "fn main() -> i32",
            "fn main() { fn(x) -> i32 { x } }",
            'fn main() { c"\\0" }',
            'fn main() { c"a\x00b" }',
            "fn main() { |x| x }",
            "fn main() { fn(x) => x }",
            "fn main() { fn x { x } }",
            "fn main() { fn(x) x }",
            "fn main() { fn(x:){x} }",
            "fn main() { f not x }",
            "fn main() { f > > g }",
            "fn main() { f < < g }",
            "@unknown(\"x\"); fn main() { 0 }",
        ]:
            fixture.write_text(source)
            cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture), valid=False)

        fixture.write_text("fn broken() { let value = ; }\nfn recovered() { 42 }\n")
        recovered = subprocess.run(
            ["tree-sitter", "parse", "--grammar-path", str(GRAMMAR), "--config-path", str(config), str(fixture)],
            cwd=GRAMMAR, env=env, capture_output=True, text=True, timeout=60,
        )
        assert recovered.returncode != 0
        assert "ERROR" in recovered.stdout or "MISSING" in recovered.stdout
        assert 'name: (identifier [1, 3] - [1, 12])' in recovered.stdout, recovered.stdout
        assert "integer [1, 17] - [1, 19]" in recovered.stdout, recovered.stdout
        subprocess.run([sys.executable, str(GRAMMAR / "scripts" / "query_test.py")], check=True)
        corpus_count = (GRAMMAR / "test/corpus/syntax.txt").read_text().count("\n---\n")
        print(f"Verified {corpus_count} corpus cases, {len(sources)} repository sources, highlight/locals/tag queries,")
        print("actual highlight classes, incremental negation/application reparses, NUL strings, 20 reserved keywords, 8 contextual names, rejected forms and error recovery.")


if __name__ == "__main__":
    main()
