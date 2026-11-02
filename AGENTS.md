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
