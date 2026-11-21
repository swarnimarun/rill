# Rill in Helix

Helix starts the same Rust compiler with the `lsp` argument and loads the local
Rill Tree-sitter parser. This setup was exercised with Helix 25.07.1 and grammar
ABI 15. Python 3.11 or later and a C compiler are needed for setup; no grammar
download or third-party Helix plugin is required.

Build the current compiler from the repository root:

```sh
cargo build --release --locked
```

Stage a configuration for review. This reads the existing Helix language file,
updates only Rill's entries, and copies `config.toml` unchanged into the stage:

```sh
rill_hx_config="${XDG_CONFIG_HOME:-$HOME/.config}/helix"
python3 editors/helix/setup.py \
  --source-config "$rill_hx_config" \
  --destination /tmp/rill-helix-review/helix \
  --server "$PWD/target/release/rill"
diff -u "$rill_hx_config/languages.toml" /tmp/rill-helix-review/helix/languages.toml
XDG_CONFIG_HOME=/tmp/rill-helix-review \
  hx --log /tmp/rill-helix-review/helix.log --health rill
```

The server path is generated from your checkout, rather than relying on an
older `rill` executable on `PATH`. It does not replace installed compiler
binaries or symlinks. The stage contains the compiled parser and highlight,
local, indentation and textobject queries.
The setup adapts numeric, member, namespace and escape captures to Helix's
documented scopes; the generic Tree-sitter package stays unchanged.

To apply the reviewed setup to Helix:

```sh
python3 editors/helix/setup.py --install \
  --destination "$rill_hx_config" --server "$PWD/target/release/rill"
hx --health rill
hx examples/sdl3-direct/main.rill
```

Installation merges the destination's current `languages.toml`, preserves other
languages and language servers, and leaves its `config.toml` untouched. Existing
Rill parser/query paths and the language file get dated `.rill-backup-*` copies.
A query-directory symlink is backed up as a symlink, without modifying its
target. Failed copies roll back the replaced files. The default command stages
under `editors/helix/.local/helix`; overwriting the source configuration requires
`--install`. Ambiguous duplicate Rill entries or
multiline values requiring reconciliation are rejected before installation.

In Helix, use space then `k` for hover, or `K` if configured, `gd` for definition,
space then `s` for document
symbols, and `:format` for LSP formatting. Syntax and type diagnostics update
while editing, including unsaved imports. Braced `if` may omit `else` when its
result is unit; legacy `if ... then ... else ...` still requires both arms.
`not ready` and `!ready` both negate booleans.

The server also supplies inferred type/name hints and signature help for calls.
Helix displays these when its configuration contains:

```toml
[editor.lsp]
display-inlay-hints = true
auto-signature-help = true
```

The installer preserves `config.toml`; it does not change those preferences.
See Helix's [LSP settings](https://docs.helix-editor.com/editor.html#editorlsp-section).

In insert mode, `Ctrl-x` requests completion; use `Tab` to select and `Enter`
to accept. Completion includes checked nested record fields and callable
types, including while a name or call is unfinished. In normal mode, `maf`
selects a function or lambda and `mif` selects its body block. Pipelines and
multiline local values use the supplied Tree-sitter indentation queries.

After compiler or grammar changes, rebuild the compiler and rerun setup. This
refreshes the generated parser and queries; restarting Helix starts the new
server. Formatting is provided by LSP, so no external formatter is configured.

Run the merge/backup tests and a real editor smoke test:

```sh
python3 editors/helix/test/setup_test.py
python3 editors/helix/test/terminal_screen_test.py
python3 editors/helix/test/workflow_test.py
python3 editors/helix/test/smoke.py --server "$PWD/target/release/rill" \
  --output /tmp/rill-helix-proof
python3 editors/helix/test/workflow.py --server "$PWD/target/release/rill" \
  --output /tmp/rill-helix-workflow-proof
```

The smoke test uses an isolated Helix configuration and a PTY. It opens the
actual SDL3 example, checks all Rill health entries, verifies rendered keyword,
numeric, member, namespace and escape highlights, observes LSP initialization
and `didOpen`, inserts an invalid
character without saving, checks diagnostics, undoes it, checks clearing, then
opens a temporary function fixture. It requires a matching, nonempty
`textDocument/inlayHint` response and the rendered inline `answer: i32` hint.
Typing `(` and `,` must produce actual `signatureHelp` responses with active
parameter indices 0 and 1, a visible signature popup, and emphasis moving from
the first parameter to the second. The source function line cannot satisfy the
popup check. The test then quits cleanly. Original file bytes must remain
unchanged. `--hx` selects a
different Helix executable. Add `--source-config "$rill_hx_config"` to exercise
your existing editor settings in the isolated session. Use a new or empty output directory; existing
traces are never reused as evidence for a new run.
Hint and automatic signature options are enabled for the isolated session if
needed; configuration files are left unchanged. The proof directory includes
the LSP proxy trace, raw terminal output, decoded hint and popup screens,
parameter styles, and response IDs in `summary.json`.
The test acknowledges a normal cursor motion after opening the hint fixture
to rearm Helix 25.07.1's idle timer, then requires the actual request and render.
For the signature-render phase only, the session disables automatic completion
so a later completion response cannot replace Helix's signature popup. Automatic
`(`/`,` signature requests and visible parameter emphasis remain required;
the separate workflow test verifies completion menus and accepted edits.

The separate workflow test drives definition jumps through generic/curried and
imported functions, hover, symbol picker selection, actual completion acceptance
for locals/imports/nested record fields, and formatting applied to an unsaved
buffer. It verifies `maf`/`mif` for named, empty and nested lambda bodies with
exact undo, plus pipeline and split-value indentation using the Tree-sitter
heuristic. Inside-function selections include the body block, following
Helix's bundled Rust convention. Both tests preserve all fixture files and
copied settings. Use a fresh output directory for each run.
The workflow selects a bundled true-color theme inside its isolated session,
because Helix's basic-color fallback omits operator scopes. Scope checks read
the decoded popup beside the cursor; status paths cannot satisfy them.
Auto-save must be disabled for that test because it edits the real example
buffer. The test refuses a copied configuration with auto-save enabled.

The configuration follows Helix's official [language configuration](https://docs.helix-editor.com/languages.html)
and [adding languages](https://docs.helix-editor.com/guides/adding_languages.html)
guides. Native header and linker checks remain separate compiler commands;
editing does not execute SDL3 or pkg-config.
