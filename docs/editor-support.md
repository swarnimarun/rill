# Editor support

Build the compiler, then configure your editor to launch the stdio server:

```sh
cargo build --release --locked
# Editor server command:
target/release/rill lsp
```

The server waits for LSP messages on stdin; it does not open an editor window.
Use the [Helix setup](../editors/helix/README.md) for hx, or the optional
[VS Code extension](../editors/vscode/README.md) and
[Neovim configuration](../editors/neovim/README.md).
These register `.rill` files and let you select the compiler executable.

## Language server

The server provides:

- Syntax diagnostics from the compiler's source tree, plus name, type,
  ownership and module checks from the existing compiler.
- Hover with inferred types, definition navigation for scoped locals and
  resolved module declarations, and document symbols.
- Completion for keywords, builtins, visible locals, declarations and exported
  members of imported modules, plus checked local record fields and nested
  callback fields. Callable and parameter details retain inferred types.
- Inferred type hints for locals, parameters and results, plus argument-name
  hints. Signature help tracks the active argument in ordinary and indirect
  calls, including incomplete calls and unsaved imported modules.
- Document formatting through the compiler's conservative formatter.
- Full and incremental edits with UTF-16 positions, version tracking and
  diagnostic clearing. Unsaved entry files and imports use in-memory overlays;
  editor requests never rewrite source files on disk.

Compiler source spans are converted from UTF-8 bytes to LSP UTF-16 positions,
including astral characters and CRLF/CR/LF line endings. Edit batches apply
sequentially and atomically. Stale versions are ignored. Invalid surrogate
positions and malformed edits cannot partially update a document.

Explicit parameter and named-function result annotations suppress the matching
inferred hints. Their checked types still appear in hover and signature help;
unannotated parameters and results retain inferred hints.

Completion and signature help can recover completed bindings while an
identifier, member or call is unfinished, including nested blocks and curried
calls. Recovered syntax supplies scope only. Removed expressions and unfinished
bindings never receive fabricated semantic types. Member completion follows
the compiler's exact nominal record identity, not a matching type spelling.

File-backed documents use the ordinary module loader, resolver, source origins
and checker. Library buffers do not need `main`; an existing `main` must still
take zero arguments. Untitled buffers support local checks; imports need a
file-backed location. Checking an executable through the CLI still requires
its ordinary entry point.

Keep the program's entry file open while editing nested library modules. The
server uses the unique open entry graph containing that module, so imports keep
their compiler-defined entry-directory meaning. With no matching entry, the
file is checked as a standalone library. Multiple matching open entries report
`E-LSP-CONTEXT`; close the extra entry to select a context.

The server does not run native tools during editing. Opening the SDL3 game
does not invoke pkg-config, compile headers or execute SDL3. A successful
`rill check` or native build publishes the referenced `@include` declarations
for editor analysis. Run that command once before editing an include-only
program, and again after adding new C references or changing the header/target
configuration. Recorded header or search-directory changes discard snapshots.
Snapshots describe the last configured build and never supply CLI codegen
inputs. Generated declarations support typed completion, hover, hints and
signature help; definition navigation does not point past a real Rill buffer.
Use `rill check` and `rill build` for current target-header and link validation.

## Highlighting

The [Tree-sitter package](../editors/tree-sitter-rill/README.md) includes the
grammar, generated ABI 15 C parser, highlight/locals/tag queries and corpus.
It covers primary braces/semicolons, accepted legacy expressions, modules,
records/ADTs/patterns, native declarations, ownership spellings, `not`/`!`,
lambdas, expression/whitespace application, pipelines and composition.
The Rust compiler remains authoritative for accepted syntax and semantics.

Helix and Neovim load that parser and its queries. VS Code uses the included
TextMate grammar alongside the LSP client; it does not load Tree-sitter queries
directly. The setup guides distinguish project sources from explicitly
installed user configuration and preserve existing settings.

Helix's queries include lambda function textobjects and continuation
indentation for pipelines and multiline local values. Field/type names keep
their colors when a parameter has the same spelling. See the
[real editor workflow verification](verification/2026-11-04-helix-workflows.md).

## Current limits

This is an experimental editor implementation, not completion of PLAN.md's
tooling or phase gates. Analysis recomputes compiler snapshots rather than
using an incremental semantic cache. Local navigation requires a successful
parse; syntax diagnostics and recovery remain available while editing invalid
source. Semantic errors without compiler spans anchor to the start of the
document instead of guessing an expression location.

The server advertises only implemented requests. Rename, references, code
actions, debugger mappings and a cost report are not included.
The input frame limit is 8 MiB; an open source document is limited to 1 MiB,
with at most 128 documents and 16 MiB of open text. Module loading retains its
existing containment, cycle, source-size and graph limits.

Protocol behavior follows the official [Language Server Protocol](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).
See the editor READMEs for tested runtime versions and host-test limits.
The [verification report](verification/2026-11-03-editor-support.md) records
the actual Helix installation, editor smoke tests and compiler/native checks.

See [functions and application](functions.md) for currying, closure inference and
representation/resource boundaries. [Function and hint verification](verification/2026-11-03-functions-and-hints.md)
records this subsequent increment.
