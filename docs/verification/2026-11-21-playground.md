# Learning playground verification

The local playground and offline book build from the same lesson files, Markdown chapters and Rust compiler as the CLI. This checkpoint adds tooling to the `systems-ml` lineage. [Run the workbench](../learning-workbench.md) · [Trace the compiler](../compiler-core.md)

## Teaching and browser checks

- `rill playground --check-lessons` replayed all eight lessons, including the intentional operand diagnostic and the four-lane GPU map. The CLI test also ran the installed-style binary from a directory outside the source checkout.
- The Chromium browser regression ran against a live loopback server at desktop and mobile widths. It exercised the twelve compiler stages, edit and rerun, saved drafts, formatting output, reset, copied output, source diagnostics, kernel entry and inputs, stale responses and navigation between the book and editor. Every browser request stayed on the local origin. [Browser test](../../scripts/test-playground-browser.cjs)
- The independently generated static book had 39 pages, 3,834 checked links and 242 searchable destinations. The offline search tests passed for ranking, navigation, Unicode and local-file use. The chapter commands ran the terminal scene to score 1, saved `hello` in the editor and synchronized the database value `42` to a replica.
- Four grouped core tests exercised compiler contracts, including owner moves and cleanup in host IR, a type-checking program rejected by that IR subset, bundled imports and the named map lowerer. The browser displays target-specific LLVM text. Device dispatch evidence for this compiler lineage belongs to the [GPU checkpoint](2026-11-21-systems-ml.md#llvm-device-targets).
- A live HTTP probe evaluated `6 * 7`, produced an operand-type diagnostic and refused a cross-origin POST. A nonterminating loop hit the five-second limit while another request returned `42`. A subsequent request also succeeded. On Linux the server resolves the worker via its own running executable, including when Cargo has rebuilt the on-disk binary.

## Repository checks

Formatting and strict Clippy passed with default features. Strict Clippy also passed with minimal features. A minimal-feature selection passed 19 tests across `explorer_core`, `playground_cli` and `docs_cli`. The full default run of `cargo test --locked --no-fail-fast -- --show-output` exited successfully with **1,524 reported passes, zero failures and zero ignored**, in 161 result groups. Five passing test functions reported capability skips. Three voxel GPU checks found no native video device, the optional 256-cubed workload was disabled, and GCC skipped a Clang resource-directory check. The latter passed on a separate run with Clang.
