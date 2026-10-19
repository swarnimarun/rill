# Rill for editors

The grammar in `tree-sitter-rill/` is shared by every integration; `zed-rill/`
packages it as a Zed dev extension.

## Helix

`~/.config/helix/languages.toml` points Helix at the grammar source:

```toml
[[language]]
name = "rill"
scope = "source.rill"
file-types = ["rill"]
comment-tokens = ["#", "//"]
indent = { tab-width = 4, unit = "\t" }
language-servers = ["rill"]

[[grammar]]
name = "rill"
source = { path = "/path/to/rill/editors/tree-sitter-rill" }
```

Then:

```console
$ hx --grammar build
```

`hx --grammar build` compiles the parser but does **not** copy the grammar's
`queries/` directory into `~/.config/helix/runtime/queries/rill/`, and Helix
loads highlight queries from there. Keep them in sync with a symlink so a
grammar change and its query land in the same edit:

```console
$ rmdir ~/.config/helix/runtime/queries/rill
$ ln -s /path/to/rill/editors/tree-sitter-rill/queries \
    ~/.config/helix/runtime/queries/rill
```

A stale query copy fails at buffer load with
`Failed to compile highlights for 'rill': invalid node type …` and silently
turns highlighting off; `hx --health rill` still reports the queries as present,
because it only checks that the files exist.

## Zed

See `zed-rill/README.md`. Its `languages/rill/highlights.scm` is a copy of
`tree-sitter-rill/queries/highlights.scm`, since a published extension ships its
own files; refresh it whenever the query changes.
