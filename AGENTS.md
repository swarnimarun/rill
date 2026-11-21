# Rill engineering instructions

- MUST read `PLAN.md`, `README.md`, `docs/phase-status.md`, and
  `docs/CLOUD_HANDOFF.md` before continuing this checkpoint.
- MUST preserve unrelated user changes and existing branch history. The `v1`
  checkpoint has a separate lineage from the older `main` implementation.
- MUST NOT describe this subset as a stable language or mark a phase complete
  without the acceptance evidence required by PLAN.md section 19.
- Use Rust for the compiler and C11 as the first output target.
- Keep source `{}` blocks and `;` statements as the documented primary syntax.
  Legacy expression syntax remains accepted for existing fixtures.
- Reject unsupported language or ABI forms with diagnostics. Never substitute
  a fabricated value, silently copy an owner, or guess a foreign signature.
- Record real blockers under `docs/blockers/` with evidence and next steps.

## Implementation boundaries

The file loader resolves local and explicitly bundled standard-library modules
before checking. Native bindings can export functions and constants; privacy
uses resolved symbol identity, while code generation preserves actual C names.
Every declaration must be validated before compatible duplicates coalesce.

Source native dependencies resolve through structured process arguments. Do
not introduce shell evaluation. Preserve pkg-config's linker sequence and raw
CLI linker arguments. Source `@link` entries supplement resolved dependencies.

The general interpreter/C backend and restricted typed owner backend currently
have different supported subsets. Owner programs must use the checked typed
path. Moves, borrows, cleanup and observable allocation/failure behavior must
agree across interpreter and C execution, including optimized C builds.

Raw C pointers have no checked Rill lifetime or automatic foreign-resource
cleanup. Header compatibility is not an effect or ownership proof. See
`docs/c-interop.md` before expanding this boundary.

## Keep the language, playground and book in sync

- Keep semantic changes in the shared Rust lexer, parser, checker, interpreter, typed IR, emitters and LSP presentation. `rill::explorer::inspect` in `src/explorer.rs` adapts these APIs for one source buffer. A browser view must use that API for results and diagnostics. The HTTP layer lives in `src/playground/` and the browser controls in `src/playground/assets/`.
- Keep each lesson’s source in `examples/playground/<id>.rill`. Add its title, exercise, starting view and expected result, compiler-output fragment or diagnostic substring to `examples/playground/lessons.json`. Use `ok: false` for a deliberate rejection. Run, check and IR evaluation compare the entire trimmed result. `build.rs` discovers and embeds the sources. The CLI and browser consume the same embedded catalogue. Rebuild and restart the server after changing lessons, guides or browser assets.
- When syntax, inference, ownership or IR changes, update the matching compiler and editor regression tests, the relevant lesson, and the [compiler walkthrough](docs/compiler-core.md). For a new playground view, update the shared explorer dispatch, the browser stage selector, its help text, [run guide](docs/learning-workbench.md) and view tests. Preserve precise distinctions between interpreted values, generated code and device execution.
- When a command, dependency or example changes, run its documented invocation and update the README quickstart, [run guide](docs/learning-workbench.md), owning example README and CLI help as applicable. Register new book chapters in `src/docgen/content.rs`. Both the live `/book/` and `rill docs --language` render those same checked-in Markdown sources. Keep historical verification records attached to their original checkpoints.

Run the targeted synchronization checks before the full matrix below.

```sh
cargo run --locked -- playground --check-lessons
cargo test --locked --test explorer_core --test playground_cli --test docs_cli
cargo run --locked -- docs --language -o build/book
python3 scripts/check-docs-links.py build/book
node scripts/test-docs-search.cjs
```

For a UI change, start `rill playground`, run `node scripts/test-playground-browser.cjs` with Playwright and Chromium available, and exercise desktop and mobile layouts. Check a source edit, a diagnostic, a target switch, and book navigation. Run default and minimal-feature Cargo suites sequentially in the same target directory, or use separate target directories. Their CLI binaries have the same filename.

## Verification

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

Run the smallest affected test set during development, then the full suite for
cross-component changes. Tests compile C using `cc`. The actual SDL3 integration
needs pkg-config and SDL3 development files and reports a skip when unavailable.
Zig and AArch64 NEON checks depend on their toolchains/targets. Report failures
and skips honestly; do not claim checks that were not run.

Before a checkpoint, inspect diffs and untracked files. Do not commit `target`,
temporary binaries, caches, credentials or local machine paths. Update docs
and examples when interfaces change. Push only when explicitly requested.
