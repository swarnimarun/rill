#!/usr/bin/env python3
"""Check structural query ranges and highlights through the generated parser."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
from verify import GRAMMAR, Highlights, isolated_config

HELIX = GRAMMAR.parent / "helix" / "queries"


def main():
    with tempfile.TemporaryDirectory(prefix="rill-queries-") as directory:
        temporary = Path(directory)
        fixture = temporary / "queries.rill"
        environment = dict(os.environ, XDG_CACHE_HOME=str(temporary / "cache"))
        config = isolated_config(temporary)

        def cli(*arguments):
            result = subprocess.run(
                ["tree-sitter", *arguments, "--config-path", str(config)], cwd=GRAMMAR, env=environment,
                capture_output=True, text=True, timeout=60,
            )
            assert result.returncode == 0, (arguments, result.stdout, result.stderr)
            return result.stdout

        def captures(query, source):
            fixture.write_text(source)
            cli("parse", "--grammar-path", str(GRAMMAR), "--quiet", str(fixture))
            output = cli("query", "--grammar-path", str(GRAMMAR), str(query), str(fixture))
            result = []
            encoded = source.encode()
            lines = encoded.splitlines(keepends=True)
            for capture, row, column, end_row, end_column in re.findall(
                r"capture: (?:\d+ - )?([\w.]+), start: \((\d+), (\d+)\), end: \((\d+), (\d+)\)", output,
            ):
                start = sum(map(len, lines[:int(row)])) + int(column)
                end = sum(map(len, lines[:int(end_row)])) + int(end_column)
                result.append((capture, encoded[start:end].decode()))
            return result

        source = """fn outer(value) {
    let transform = fn(input) {
        fn() { input + value }
    };
    transform(value)
}
fn empty() {}
"""
        objects = captures(HELIX / "textobjects.scm", source)
        for function in [
            source[:source.index("\nfn empty")],
            "fn(input) {\n        fn() { input + value }\n    }",
            "fn() { input + value }",
            "fn empty() {}",
        ]:
            assert ("function.around", function) in objects, objects
        # Helix's bundled Rust queries use the complete body node for inside.
        # Verify that same range contract for named functions and lambdas.
        for body in ["{ input + value }", "{}"]:
            assert ("function.inside", body) in objects, objects

        source = """fn main() {
    let transform = fn(input) { input };
    let answer =
        20
            |> transform
            >> transform;
    answer
}
"""
        indents = captures(HELIX / "indents.scm", source)
        assert ("indent", "20\n            |> transform\n            >> transform") in indents, indents
        assert ("indent", "transform\n            >> transform") in indents, indents

        # A field label or type name is not a lexical variable reference, even
        # when a parameter with the same spelling is visible.
        source = """import helper as h;
fn inspect(count, State, value) {
    record State { count = count };
    value.count;
    h.identity(count)
}
"""
        locals_ = captures(GRAMMAR / "queries" / "locals.scm", source)
        assert locals_.count(("local.reference", "count")) == 2, locals_
        assert ("local.reference", "State") not in locals_, locals_
        assert locals_.count(("local.reference", "h")) == 1, locals_
        rendered = cli("highlight", "--grammar-path", str(GRAMMAR), "--html",
                       "--layout", "fragment", "--style", "minimal", str(fixture))
        highlights = Highlights()
        highlights.feed(rendered)
        assert [capture for token, capture in highlights.tokens if token == "count"] == [
            "variable parameter", "property", "variable parameter", "property", "variable parameter",
        ], highlights.tokens
        assert ("State", "type") in highlights.tokens, highlights.tokens
        assert [capture for token, capture in highlights.tokens if token == "h"] == [
            "module", "module",
        ], highlights.tokens

        fixture.write_text("fn inspect(callback, p) { p.callback(1) }\n")
        rendered = cli("highlight", "--grammar-path", str(GRAMMAR), "--html",
                       "--layout", "fragment", "--style", "minimal", str(fixture))
        highlights = Highlights()
        highlights.feed(rendered)
        assert [capture for token, capture in highlights.tokens if token == "p"] == [
            "variable parameter", "variable parameter",
        ], highlights.tokens
        assert [capture for token, capture in highlights.tokens if token == "callback"] == [
            "variable parameter", "function",
        ], highlights.tokens

        source = "fn scope(outer) { let f = fn(inner) { outer + inner }; outer + inner }\n"
        fixture.write_text(source)
        rendered = cli("highlight", "--grammar-path", str(GRAMMAR), "--html",
                       "--layout", "fragment", "--style", "minimal", str(fixture))
        highlights = Highlights()
        highlights.feed(rendered)
        assert [capture for token, capture in highlights.tokens if token == "inner"] == [
            "variable parameter", "variable parameter", "variable",
        ], highlights.tokens

        # These are syntax distinctions exercised by the Rust parser: exports
        # end legacy bodies, while export remains a contextual expression name;
        # a nested record-field call is an arbitrary expression invocation.
        source = """export let first value = let outcome = value in move outcome
export let second value = value
export fn third() { identity export }
fn chains(p) { p.inner.callback(20)(22); p.inner.callback 20 22 }
"""
        fixture.write_text(source)
        parsed = cli("parse", "--grammar-path", str(GRAMMAR), "--no-ranges", str(fixture))
        assert parsed.count("(legacy_function_declaration") == 2, parsed
        assert parsed.count("(function_declaration") == 2, parsed
        assert parsed.count("(apply_expression") == 2, parsed
        assert parsed.count("(field_expression") == 2, parsed
        rendered = cli("highlight", "--grammar-path", str(GRAMMAR), "--html",
                       "--layout", "fragment", "--style", "minimal", str(fixture))
        highlights = Highlights()
        highlights.feed(rendered)
        assert [capture for token, capture in highlights.tokens if token == "export"] == [
            "keyword", "keyword", "keyword", "variable",
        ], highlights.tokens
        assert [capture for token, capture in highlights.tokens if token == "callback"] == [
            "function", "function",
        ], highlights.tokens
        print("Verified Helix textobject/indent query ranges and lexical scope, member, type and module highlights.")


if __name__ == "__main__":
    main()
