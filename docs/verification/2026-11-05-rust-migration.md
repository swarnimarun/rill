# Rust migration guide verification

The guide describes the current experimental subset and links proposed features
separately. No new loop exits, early return, iterator, trait or macro semantics
are introduced by this documentation change.

## Executed checks

- `cargo test --locked --test rust_migration_examples`: 1 passed. The test loads
  all four complete guide programs, checks their source matches the guide,
  executes the interpreter, emits C and verifies `42` at both O0 and O3.
- `cargo test --locked --test docs_cli`: 13 passed, including the bundled
  migration/proposal routes, control-flow anchor and search-index entry.
- Generated the language site with `rill docs --language` in a temporary tree.
  `scripts/check-docs-links.py` passed with 28 HTML pages, 2,186 links and 208
  search addresses. `scripts/test-docs-search.cjs` passed matching/ranking,
  snippets, safe text, URLs, paging, Unicode, keyboard and file-protocol checks.
- `git diff --check` passed before committing this guide.

The environment is arm64 macOS with the installed `cc`. This guide-only
checkpoint did not rerun the full compiler/native/GPU suite. Compiler changes
in the concurrent incremental/reload work have separate acceptance requirements.
