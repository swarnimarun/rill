# Checked uniform C oracle and shared native declarations

## Scope

F0 is a refactoring checkpoint based on
`f1ba72670762686214c9a3ad70d5b9ce0fa0b569`, not typed foreign lowering or a
completed P1/P5 gate. `backend::emit_reference(&Program)` analyzes the complete
source program, rejects owners (including inactive code and owner-bearing type
declarations), and explicitly selects the existing uniform C representation.
Its layout and stack limits remain unchanged. Production `backend::emit` keeps
its existing typed-first selection and Copy-only unsupported fallback.

Native C type spellings, manual prototypes and header assertions move unchanged
to a crate-private helper. Every function and constant remains present, including
unused declarations. Includes retain their original position and ordering;
source links, packages, native symbols, module coalescing, driver arguments and
selected-importer checks are unchanged. The target C compiler remains the ABI
authority; neither emission API invokes it or proves effects/lifetimes.

The byte-buffer and vector differential tests now emit uniform C from the exact
same checked program rather than adding an unused raw conversion. Primitive C
tests also compare both emitted representations at `-O0` and `-O2`. This does
not widen accepted language forms, create a CLI option, or allow owner/foreign
mixing. Raw C values still await the separate typed-native migration.

## Environment and frozen baseline

Cloud x86_64 Linux; Rust/Cargo 1.99.1; GCC 14.2.0; Clang 19.1.7; SDL3 3.2.10.
Only the existing assigned Cargo target was reused, with no installs or cache
removal. The 2 GiB disk floor was respected. Further compilation was stopped when
remaining space reached 2.2 GiB; no additional full feature matrix was started.

Before edits, the existing `ffi`, `c_import_direct`, `native_cli`,
`native_modules` and `public_entrypoints` suites passed 27 tests. Three frozen
native programs cover header-backed calls, no-header manual calls, and an
unused declaration manifest. Their emitted C SHA-256 hashes are:

- Header calls: `7198fb330796cfbd08c85b61d10c79811ded17662a9f7c45a7213952e657ee24`
- Manual calls: `c9193ef43d744d4cd739b10938b5ec74cc5378c27cca5bb455b369cc5c9f268a`
- Unused manifest: `911ef9963a33bcf52654982d66bcbd6920390b650770612ebb73dd42de96dc4d`

Candidate C is byte-identical for all three. GCC and Clang builds at `-O0` and
`-O2` all exit zero with exactly matching stdout/stderr: UTF-8 text,
bool and ordered `step:19`/`step:23` events precede `42`; the unused manifest
prints only `42`. That manifest retains all supported foreign ABI kinds and
integer/bool/float/pointer constant assertions. This is bounded regression
evidence, not a new cross-target ABI guarantee.

## Verification

Final implementation checks, before independent review:

- New reference/owner-negative and public-entrypoint regressions: 9 passed.
- Expanded focused suite: 42 passed. The later direct-header/reference equality
  assertions also passed their five-test suite and both broader runs below.
- Full default suite: 541 passed, zero failures or ignored tests.
- Importer-enabled focused matrix: 251 passed, zero failures or ignored tests.
  It includes all 23 selected-importer cases and successful real direct/generated
  SDL3 integrations. Neither run reported a dependency skip.
- Strict all-target Clippy: passed with default features and with `c-import`.
- Final formatting and `git diff --check`: passed.
- Frozen C and native-trace comparisons: 3 byte-identical C outputs and 12
  successful GCC/Clang native executions.

Commands used with the existing configured cloud toolchain and assigned target:

```sh
cargo test --locked --test backend_reference --test public_entrypoints \
  --test ffi --test c_import_direct --test native_cli --test native_modules \
  --test typed_c --test typed_bytes_audit --test typed_vectors
cargo test --locked --no-fail-fast -- --nocapture
cargo test --locked --offline --features c-import --lib \
  --test selected_c_import --test c_import_direct --test native_cli \
  --test native_modules --test ffi --test backend_reference \
  --test public_entrypoints --test sdl3_direct -- --nocapture
cargo clippy --locked --offline --all-targets -- -D warnings
cargo clippy --locked --offline --features c-import --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The focused tests check each original ABI spelling, ordered unused prototypes
and assertions, unchanged dependency metadata, native symbol identity, header
mismatch rejection, checked forged metadata, and owner exclusion. The unchanged
selected-importer synthetic-main path passed its feature-enabled regressions,
including frontend/target type disagreement and changed preprocessing defines.

## Limits and remaining work

No full importer-enabled aggregate suite was run for the standalone author checkpoint because
it would duplicate many binaries under the remaining disk limit. The complete
default suite, the stated importer-enabled suites, and both Clippy variants
are the evidence available. No Zig execution or AArch64 NEON execution was run;
this is x86_64 Linux evidence. The SDL tests use the existing bounded headless
workflow, not physical input or hardware-renderer validation. New screenshots
were not part of this refactor.

Native calls and values still use uniform C on the production path. Explicit
checked native IR, foreign scalar/pointer transport, independent native-node
verification and their migration gates are separate subsequent work. This
checkpoint does not add effects, foreign lifetime proofs, aggregate ABIs,
callbacks, C-callable Rill exports, or a host interpreter FFI.

## Reviewed combined integration

Independent review accepted the exact F0 source at `2eebe405`, mechanically
confirmed the declaration/type helper extraction, replayed 21 pure validation
and emission checks and 18 existing native executables, and checked the frozen
C snapshots. Those independent replays did not compile new binaries.

The combined source `2b48052857bf846d795fa7c0b08a7d8329ce99f2` subsequently passed
the complete seven-command matrix: **541 default tests**, **575 importer-enabled
tests**, formatting, strict all-target Clippy in both feature modes and Windows
GNU all-target compile checks in both modes. There were no failures, ignored
tests or dependency skips; the exact source and clean worktree were unchanged.
The previously omitted full importer aggregate is therefore covered for this
combined checkpoint.

To avoid deleting existing work or exhausting persistent storage, this run used
a separate temporary RAM-backed Cargo target, two build jobs, disabled Rust
debug symbols and disabled incremental artifacts. Before each stage, guards
required at least 2 GiB target space and 3 GiB available memory. These build
settings reduce artifact storage; they add no language optimization or target
semantics claim. Source, logs and durable checkpoints remained separate from
the temporary build artifacts. Native differential tests retained their own
explicit C optimization settings.
