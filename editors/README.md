# Rill for editors

The grammar in `tree-sitter-rill/` is shared by every integration; `zed-rill/`
packages it as a Zed dev extension.

## Helix

Helix stores the grammar path verbatim, so it must be the checkout's absolute
path. From the repository root, this writes the language, server and grammar
entries with the path it computes, then links the queries next to where Helix
loads them:

```console
$ root=$(pwd)
$ mkdir -p ~/.config/helix
$ cat >> ~/.config/helix/languages.toml <<EOF

[language-server.rill]
command = "rillc"
args = ["lsp"]

[[language]]
name = "rill"
scope = "source.rill"
file-types = ["rill"]
comment-tokens = ["#", "//"]
indent = { tab-width = 4, unit = "\t" }
language-servers = ["rill"]

[[grammar]]
name = "rill"
source = { path = "$root/editors/tree-sitter-rill" }
EOF

$ mkdir -p ~/.config/helix/runtime/queries
$ rm -rf ~/.config/helix/runtime/queries/rill
$ ln -s "$root/editors/tree-sitter-rill/queries" \
    ~/.config/helix/runtime/queries/rill

$ mkdir -p ~/.config/helix/runtime/grammars
$ hx --grammar build rill
```

If `languages.toml` already has entries for `rill`, update the `source` path
instead of appending a second block.

`hx --grammar build` compiles the parser but does **not** copy the grammar's
`queries/` directory into `~/.config/helix/runtime/queries/rill/`, and Helix
loads highlight queries from there. The symlink above is what keeps a grammar
change and its query in the same edit.

A stale query copy fails at buffer load with
`Failed to compile highlights for 'rill': invalid node type …` and silently
turns highlighting off; `hx --health rill` still reports the queries as present,
because it only checks that the files exist.

## Zed

See `zed-rill/README.md`. Its `languages/rill/highlights.scm` is a copy of
`tree-sitter-rill/queries/highlights.scm`, since a published extension ships its
own files; refresh it whenever the query changes.
