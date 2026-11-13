# Rill engineering instructions

- MUST read `PLAN.md`, `README.md`, `docs/phase-status.md`, and
  `docs/CONTINUATION_PLAN.md` before continuing work.
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

## Resuming work

Inspect `git status --short`, `git log -5 --oneline`, and
`git log -1 --decorate` before editing. The workspace may use Jujutsu or a
detached Git HEAD. Earlier `v1-ernie` work was integrated into the newer local
`v1` lineage; do not switch branches or merge histories to match old reports.

Read the contract for the component being changed and its relevant verification
record. `docs/CONTINUATION_PLAN.md` owns the ordered queue,
`docs/phase-status.md` owns capability claims, and `docs/build-performance.md`
owns performance workloads and measurement gates. Historical test totals apply
only to their recorded revisions and environments.

Prepared release and Pages workflows do not establish that remote assets or a
deployment exist. See `docs/distribution.md`, `docs/install.md`, and
`docs/docs-maintenance.md` for those workflows.

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

The typed verifier is not the complete ownership/effect MIR proof proposed in
PLAN section 16. Owner programs remain monomorphic and acyclic, without owned
records or foreign declarations. New move/borrow/cleanup behavior needs
differential and failure-path evidence.

Exact-graph checked-C reuse does not cache C compilation or linking. Native and
shader recipes still prepare current inputs; changed pure graphs recheck.
Project watch restarts processes. Interpreter hooks, scalar native generations,
Copy-wire calls and versioned Copy-state reload have separate contracts in
`docs/hot-reload.md`, `docs/c-exports.md` and `docs/native-hot-reload.md`.
Do not infer owner, callback, foreign-resource or GPU-resource migration support
from them. External shader tools and working renderers do not establish a Rill
shader language or checked GPU lifetimes.

## Verification

Use Rust/Cargo and a C11 compiler available as `cc`. Default features include
`c-import`, which loads libclang at runtime. `--no-default-features` retains
pure Rill and explicit extern support. Configure host SDK paths locally.

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

For changes affecting both feature configurations, also run:

```sh
cargo test --locked --no-default-features --no-fail-fast
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

Run the smallest affected test set during development, then the full suite for
cross-component changes. Tests compile C using `cc`. The actual SDL3 integration
needs pkg-config and SDL3 development files and reports a skip when unavailable.
Zig and AArch64 NEON checks depend on their toolchains/targets. Report failures
and skips honestly; do not claim checks that were not run.

Record optimization levels actually tested, including O3 when accepting
production optimization behavior. Do not run timing measurements concurrently
with tests. Record importer, SDL, Zig, NEON and GPU outcomes separately from
successful test returns. Required engine GPU jobs use `RILL_REQUIRE_GPU=1`;
missing capabilities must fail those jobs. Compile-only checks do not establish
runtime acceptance.

For documentation-only changes, follow
`docs/docs-maintenance.md#build-and-check-locally`:

```sh
cargo run --locked --release --no-default-features -- docs --language -o target/site
python3 scripts/check-docs-links.py target/site
node scripts/test-docs-search.cjs
```

Run affected documentation tests when needed. These checks do not rerun compiler
or GPU acceptance. Register new manual chapters in `src/docgen/content.rs`;
keep generated HTML out of Git.

## Recording a checkpoint

Update affected contracts and phase status, link a dated verification record
with commands, results and skips, and revise the active queue when acceptance
changes a dependency. Preserve earlier evidence, including failures and totals.

Before a checkpoint, inspect diffs and untracked files. Do not commit `target`,
temporary binaries, caches, credentials or local machine paths. Update docs
and examples when interfaces change. Push only when explicitly requested.
Run `git diff --check` and review the final diff before handing off.
