#!/usr/bin/env python3
"""Stage or install Rill Helix support while preserving unrelated settings."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


HEADER = re.compile(r"(?m)^\s*(\[\[?[^\n]+?\]\]?)\s*(?:#[^\n]*)?$")
HELIX_SCOPES = {
    "number": "constant.numeric.integer",
    "property": "variable.other.member",
    "module": "namespace",
    "string.escape": "constant.character.escape",
}


def helix_highlights(source):
    return re.sub(r"@([A-Za-z0-9_.]+)", lambda match: "@" + HELIX_SCOPES.get(match[1], match[1]), source)


def basic_string(value):
    return json.dumps(str(value), ensure_ascii=False)


def replace_values(block, values):
    for key, value in values.items():
        assignment = re.compile(rf"(?m)^([ \t]*{re.escape(key)}[ \t]*=)[^\n]*$")
        found = assignment.search(block)
        if found:
            # Refuse ambiguous multiline assignments instead of dropping text.
            tomllib.loads(found.group(0))
            block = assignment.sub(lambda match: f"{match.group(1)} {value}", block, count=1)
        else:
            lines = block.splitlines(keepends=True)
            insert = next((index + 1 for index in reversed(range(len(lines))) if lines[index].strip() and not lines[index].lstrip().startswith("#")), len(lines))
            if insert and not lines[insert - 1].endswith("\n"):
                lines[insert - 1] += "\n"
            lines.insert(insert, f"{key} = {value}\n")
            block = "".join(lines)
    return block


def merge_languages(source, server, grammar):
    parsed = tomllib.loads(source)
    for old, new in [
        (
            "# The server is `rill lsp` - a subcommand of the installed compiler, so an update to\n# the compiler updates the server with it. (`rill-lsp` is installed too, but keeping\n# one path means the editor can never run a different build than the command line.)",
            "# Rill uses this checkout's compiled `rill lsp`; existing PATH tools are unchanged.",
        ),
        ("# The formatter indents with tabs.", "# The Rill formatter indents with four spaces."),
        (
            "# The grammar lives in the rill repository, so Helix builds it from source with\n# `hx --grammar build` - no published grammar repo needed for local work.\n# `hx --grammar build` compiles the parser only; the queries stay in sync via the\n# symlink at ~/.config/helix/runtime/queries/rill -> editors/tree-sitter-rill/queries\n# (and the LSP formats with the nearest `.rill-fmt`).",
            "# The local grammar is compiled from this checkout. Rerun editors/helix/setup.py\n# after grammar changes to refresh the installed parser and queries. Formatting\n# uses the Rill LSP.",
        ),
    ]:
        source = source.replace(old, new)
    replacements = {
        "server": {"command": basic_string(server), "args": '["lsp"]'},
        "language": {
            "scope": '"source.rill"', "file-types": '["rill"]',
            "comment-tokens": '["//"]', "indent": '{ tab-width = 4, unit = "    " }',
            "language-servers": '["rill"]', "roots": '["PLAN.md", ".git"]',
        },
        "grammar": {"source": '{ path = ' + basic_string(grammar) + ' }'},
    }
    headers = list(HEADER.finditer(source))
    pieces = [source[:headers[0].start()] if headers else source]
    seen = set()
    for index, header in enumerate(headers):
        stop = headers[index + 1].start() if index + 1 < len(headers) else len(source)
        block = source[header.start():stop]
        label = None
        name = header.group(1)
        if name == "[language-server.rill]":
            label = "server"
        elif name in ("[[language]]", "[[grammar]]"):
            kind = "language" if name == "[[language]]" else "grammar"
            data = tomllib.loads(block)
            if data[kind][0].get("name") == "rill":
                label = kind
        if label:
            if label in seen:
                raise ValueError(f"multiple Rill {label} sections require manual reconciliation")
            seen.add(label)
            block = replace_values(block, replacements[label])
        pieces.append(block)
    result = "".join(pieces).rstrip() + "\n"
    for label, header in [("server", "[language-server.rill]"), ("language", "[[language]]"), ("grammar", "[[grammar]]")]:
        if label not in seen:
            body = "" if label == "server" else 'name = "rill"\n'
            for key, value in replacements[label].items():
                body += f"{key} = {value}\n"
            result += f"\n{header}\n{body}"
    merged = tomllib.loads(result)
    for key, value in parsed.items():
        if key not in ("language", "grammar", "language-server") and merged.get(key) != value:
            raise ValueError(f"merge changed unrelated top-level setting {key}")
    for kind in ("language", "grammar"):
        before = [item for item in parsed.get(kind, []) if item.get("name") != "rill"]
        after = [item for item in merged.get(kind, []) if item.get("name") != "rill"]
        if before != after:
            raise ValueError(f"merge changed unrelated {kind} entries")
    for name, value in parsed.get("language-server", {}).items():
        if name != "rill" and merged["language-server"].get(name) != value:
            raise ValueError(f"merge changed unrelated language server {name}")
    return result


def stage(root, source_config, destination, server, compiler):
    if destination.is_symlink():
        raise ValueError("staging destination must not be a symlink")
    destination.mkdir(parents=True, exist_ok=True)
    for relative in ["runtime", "runtime/queries", "runtime/queries/rill", "runtime/grammars"]:
        target = destination / relative
        if target.is_symlink() or (target.exists() and not target.is_dir()):
            raise ValueError(f"staging path is not an ordinary directory: {target}")
    for relative in ["languages.toml", "config.toml", "runtime/grammars/rill.so", *["runtime/queries/rill/" + name for name in ("highlights.scm", "locals.scm", "indents.scm", "textobjects.scm")]]:
        if (destination / relative).is_symlink():
            raise ValueError(f"staging must not overwrite a symlink: {destination / relative}")
    existing = source_config / "languages.toml"
    original = existing.read_text() if existing.exists() else ""
    merged = merge_languages(original, server, root / "editors/tree-sitter-rill")
    (destination / "languages.toml").write_text(merged)
    editor_config = source_config / "config.toml"
    if editor_config.exists():
        shutil.copy2(editor_config, destination / "config.toml")
    runtime = destination / "runtime"
    queries = runtime / "queries/rill"
    queries.mkdir(parents=True, exist_ok=True)
    generic = root / "editors/tree-sitter-rill/queries/highlights.scm"
    (queries / "highlights.scm").write_text(helix_highlights(generic.read_text()))
    shutil.copy2(root / "editors/tree-sitter-rill/queries/locals.scm", queries / "locals.scm")
    for name in ("indents.scm", "textobjects.scm"):
        shutil.copy2(root / "editors/helix/queries" / name, queries / name)
    grammars = runtime / "grammars"
    grammars.mkdir(parents=True, exist_ok=True)
    source = root / "editors/tree-sitter-rill/src"
    subprocess.run([compiler, "-shared", "-fPIC", "-I", str(source), str(source / "parser.c"), "-o", str(grammars / "rill.so")], check=True)
    return ["languages.toml", "runtime/grammars/rill.so", "runtime/queries/rill"]


def install(staged, destination, names):
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    backups = []
    replaced = []
    try:
        for name in names:
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists() or target.is_symlink():
                backup = target.with_name(target.name + ".rill-backup-" + stamp)
                target.rename(backup)
                backups.append((target, backup))
            source = staged / name
            replaced.append(target)
            if source.is_dir():
                shutil.copytree(source, target)
            else:
                shutil.copy2(source, target)
    except BaseException:
        for target in reversed(replaced):
            if target.is_dir() and not target.is_symlink():
                shutil.rmtree(target)
            elif target.exists() or target.is_symlink():
                target.unlink()
        for target, backup in reversed(backups):
            if backup.exists() or backup.is_symlink():
                backup.rename(target)
        raise
    return backups


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    default_config = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "helix"
    parser.add_argument("--source-config", type=Path, default=default_config)
    parser.add_argument("--destination", type=Path, default=Path(__file__).resolve().parent / ".local/helix")
    parser.add_argument("--server", type=Path, default=root / "target/release/rill")
    parser.add_argument("--cc", default=os.environ.get("CC", "cc"))
    parser.add_argument("--install", action="store_true", help="apply the staged Rill files with dated backups; leave config.toml unchanged")
    options = parser.parse_args()
    source_config = options.source_config.expanduser().resolve()
    destination = options.destination.expanduser().resolve()
    server = options.server.expanduser().resolve(strict=True)
    if not os.access(server, os.X_OK):
        parser.error("--server must name an executable Rill compiler")
    if destination == source_config and not options.install:
        parser.error("overwriting the source configuration requires --install")
    if options.install:
        if (destination / "languages.toml").exists():
            source_config = destination
        with tempfile.TemporaryDirectory(prefix="rill helix install ") as temporary:
            staged = Path(temporary) / "helix"
            names = stage(root, source_config, staged, server, options.cc)
            backups = install(staged, destination, names)
            for target, backup in backups:
                print(f"Backed up {target} to {backup}")
    else:
        stage(root, source_config, destination, server, options.cc)
    print(f"Rill Helix support ready in {destination}")


if __name__ == "__main__":
    main()
