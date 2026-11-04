# Editor support implementation plan

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](../../verification/2026-11-04-ernie-integration.md).

> For agentic workers: use the delegate workflow with independent file ownership and review each delivered artifact.

Goal: provide a working stdio language server, Tree-sitter highlighting, and editor launch instructions for the compiler's accepted subset.

Architecture: `rill lsp` owns bounded JSON-RPC framing, lifecycle and versioned open documents. Pure language queries reuse the compiler's syntax tree, source origins, module resolver and checker. Tree-sitter supplies incremental editor parsing/highlights only; it cannot authorize compiler execution.

Tech stack: Rust/C11 compiler, serde_json protocol messages, Tree-sitter grammar/queries, Helix setup, with optional VS Code and Neovim client configuration.

Spec: PLAN.md sections 16, 18 and 19, README.md accepted syntax, docs/source-format.md and docs/module-source-origins.md. This increment does not complete P1 or the wider language roadmap.

## Constraints

- Preserve all existing uncommitted implementation and user changes.
- Keep braces and semicolons primary; support accepted legacy syntax in highlighting.
- Do not launch native compilers, pkg-config, imported code or shell evaluation during editor requests.
- Unsaved document overlays must pass through the existing module resolver/checker without temporary source writes.
- Use UTF-16 LSP positions, including surrogate pairs and all LSP line endings.
- Return no navigation result when the source does not establish a target.

## Tasks

- [x] Ground existing source/checker/module APIs; compare a synchronous stdio server with an async framework and syntax-only analysis with overlay-backed compiler analysis. Choose synchronous transport plus compiler-backed overlays to avoid a second type checker or native processes.
- [x] Add shared text conversion and URI helpers in src/lsp/text.rs. Reject invalid incoming UTF-16 positions; preserve percent-encoded paths. Test astral Unicode, CRLF, CR, empty lines and sequential incremental edits. Clamp oversized character positions to line end per LSP; reject split surrogates.
- [x] Add modules::load_with_overlays and compile::load_program_with_sources_and_overlays. Test unsaved imports/new files, containment, symlinks, duplicate identities, limits, cycles, exact snapshots and unchanged disk bytes.
- [x] Implement src/lsp/analysis.rs with diagnostics, symbols, hover, definitions and completion using compiler syntax/origins/resolved symbols. Test lexical scopes, imports, native bindings, library buffers and errors in unsaved text.
- [x] Implement src/lsp/mod.rs and src/lsp/protocol.rs. Add `rill lsp` dispatch in main.rs and library module export. Test real framed stdio initialization, edits, diagnostics clearing, navigation, formatting, errors, malformed frames and shutdown/exit.
- [x] Implement editors/tree-sitter-rill grammar, queries, corpus and generation/build workflow. Parse all examples without error/missing nodes and test actual highlight captures plus incremental reparsing.
- [x] Provide the requested Helix configuration/parser installer, plus VS Code extension/client and Neovim configuration with command selection and filetype registration. Test client packaging/config syntax and state honestly whether a GUI editor host was available.
- [x] Independently review and fix findings. Run formatting, strict all-target Clippy, focused editor tests, Tree-sitter tests and full compiler regressions, including real SDL3 checks after CLI changes.
- [x] Update README, source-tooling docs and phase status with working setup, supported features, limitations and exact verification. Review final diff; leave changes reviewable without pushing or mixing earlier work into a commit.

## Interfaces

```rust
pub fn diagnostics(uri: &str, text: &str, overlays: &BTreeMap<String, String>) -> Vec<Value>;
pub fn document_symbols(text: &str) -> Vec<Value>;
pub fn hover(uri: &str, text: &str, offset: usize, overlays: &BTreeMap<String, String>) -> Option<Value>;
pub fn definition(uri: &str, text: &str, offset: usize, overlays: &BTreeMap<String, String>) -> Option<Value>;
pub fn completion(uri: &str, text: &str, offset: usize, overlays: &BTreeMap<String, String>) -> Vec<Value>;
```

The text module converts positions/ranges to JSON and file URIs to paths. The server stores document text and version and applies edits atomically before queries. Analysis consumes snapshots, never server mutable state.

## Verification commands

```sh
cargo test --locked --test lsp_protocol --test lsp_analysis --test module_overlays
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --no-fail-fast
cd editors/tree-sitter-rill && tree-sitter test
```

Protocol assertions use the actual compiler executable and framed stdin/stdout. Tree-sitter assertions use the generated parser and real query captures, not regex-only checks. Full test execution can use the previously established bounded parallel target runner; record the exact configuration and capability skips.

## User-requested additions

- [x] Prioritize Helix/hx. Exercise actual Helix health, rendered captures and LSP lifecycle, then install the reviewed Rill entries with backups while preserving other user settings.
- [x] Allow braced `if` without `else` for unit-valued branches. Preserve mandatory `else` for value-producing branches and legacy `then` syntax, and verify source origins, formatter, interpreter, C and owner cleanup.

See [verification](../../verification/2026-11-03-editor-support.md) for final commands, results and limits.
