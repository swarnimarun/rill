# Explicit selected-C configurations: integrated verification

Status: focused checks, independent execution probes and the full combined
aggregate pass on `19f0e58cb21705601a9d872ce8bcfcd18a032b07`, whose production
source is merge `78dc979073dfe6ba9a8d421a62b49d19fb943b54`. This bounded
configuration increment is accepted; it is not P5/P11 completion.

Base: `2d866b0e2b16e753baba743cb95fd1c2277f9034`. Initial source checkpoints:
`ff9958d0638f475a897c7859ba71b7ff90bb0f12` (codec/preparation) and
`fd6abf3eeb305396bba584ed7aa2fbbd6ad62459` (worker/CLI/snapshot consumer).
Subsequent source refinements preserve strict-versus-legacy scalar assertions,
include the module-length prefix in metadata bounds, reject ambiguous resource
operands and validate the combined linker sequence.

The initial checkpoints were source-only. Subsequent checks used two build jobs,
debug info disabled and incremental compilation disabled. Direct formatting,
full `cargo fmt --all -- --check` and `git diff --check` pass.

## Implemented contract

- Explicit deterministic bounded binary records; escaped read-only inspection.
- Comparison-only compiler assertion, fixed working-directory basis, ordered
  recipe and package outputs, structured target/scalar/constant facts and exact
  binding bytes. No raw C, executable selection, success cache or ambient secrets.
- Strict C/link grammar applied before frontend/target execution; every use gets
  fresh supervised preparation, with normal Rill checking and successful exit.
- Configured consumers compare the exact loaded binding snapshot they lower,
  reject additional native dependencies and carry fresh exact assertions into
  full translation-unit checks/builds. No second package resolution or module-source read.
- One-file atomic configuration, module and configured executable publication;
  runtime CWD remains separate from compiler CWD. Unconfigured commands preserve
  their legacy behavior and raw linker arguments.

## Acceptance sources and executed focused checks

Pure codec/argument/fact tests cover raw Unix bytes, deterministic encoding,
truncation/trailing/version/tag/count limits, metadata/module budgets, invalid
resources, macro/driver escapes, ordered links, range/type facts, assertion
rendering and strict-versus-legacy behavior. Worker tests cover the new request
frame and explicit child CWD in addition to existing fault supervision tests.

`tests/native_configuration.rs` covers malformed inspection/CLI conflicts,
unavailable-feature rejection, one-load source identity, deterministic capture,
fresh materialization and all four consumers, moved projects/different caller
CWD, compiler authority, signature/constant/transitive drift, logical-module and
byte identity, extra dependencies, optimization-dependent preprocessing, raw
native fixture O0/O2 execution, non-UTF-8 paths, package-order drift, strict
bypasses, timeout and atomic failure retention. Additional cases exercise configured SDL3 O0/O2,
runtime CWD independence, signal cancellation and concurrent publication.

The pre-merge author matrix passed:

- Fresh default and importer-enabled typechecks. Compiler JSON identifies the
  exact native-configuration worktree manifest/source roots and `fresh: false`
  for its library and binary. Source-root timestamps were refreshed with SHA256
  before/after equality; all builds reused the same build target with two
  jobs, debug info disabled and incremental compilation disabled.
- Default native units: 20 passed. Default configuration/CLI/snapshot: 3 passed.
- Importer-enabled native units: 21 passed. Worker protocol/supervision: 12 passed.
  New configured CLI/native suite: 15 passed.
- Existing importer/module/native/typed-FFI/SDL regressions: 94 passed across
  `ffi`, `module_cli`, `module_compile`, `native_cli`, `native_modules`,
  `sdl3_direct`, `selected_c_import`, `typed_foreign` and `typed_foreign_raw`.
- Both strict `cargo clippy --all-targets -- -D warnings` modes passed.
- Both Windows GNU `cargo check --target x86_64-pc-windows-gnu --all-targets`
  modes passed. This is compile-only, not Windows supervision/execution support.
- After the final narrow Unix-only timeout-field change, default configuration
  tests (3), importer-enabled config/importer/native/module/SDL tests (50), fmt,
  both strict Clippy modes and both Windows checks passed again without warnings.

Actual configured SDL3 3.2.10 ran three-frame game variants at saved O0/O2;
existing direct/generated SDL3 tests also passed, with no skip messages. This
adds execution evidence, not new framebuffer captures, physical input or
hardware-renderer claims. The environment used Rust/Cargo 1.99.1, GCC 14.2.0,
Clang/libclang 19.1.7 on x86_64 Linux. The final pre-merge importer-enabled CLI
binary SHA256 was
`c1a1cfcb9bab28119dfd633633556a9873659e9a356ec5769ded3d268aa375c2`.

Two initial issues are retained in the evidence: the new raw fixture passed a
mutable pointer to an exact const-pointer parameter and correctly failed Rill
type checking; the fixture was changed to the existing exact-type handle
function, not by weakening the checker. The first Windows checks compiled with
an unused timeout-field warning; field storage is now Unix-only and both checks
were rerun clean. All original failure/warning logs are retained separately.

## Integrated focused verification

The same worktree merged accepted published Copy-node commit
`4bb9973af06e983f97464d285935c4f141bd708c`, retaining both histories in merge
`78dc979073dfe6ba9a8d421a62b49d19fb943b54`. Only appended CLOUD_HANDOFF prose
conflicted; both sections were retained. There were no production conflicts.

The merged source received another hash-preserving source-root timestamp
refresh. Fresh default and importer-enabled typechecks again recorded the exact
manifest and source roots with `fresh: false` for both package targets. The
following passed without warnings or skip messages:

- Default native units 20 and configuration/CLI/snapshot tests 3.
- Importer-enabled native units 21 and worker tests 12.
- Combined config/importer/module/native/typed-FFI/SDL and provenance regressions
  148: the earlier 94, the new configured suite 15, module origins 13, Copy-node
  origins 17 and function/source origins 9.
- Full formatting check, both strict all-target Clippy modes and both Windows
  GNU all-target compile-only checks.

Configured and direct/generated real SDL3 execution passed again. The integrated
importer-enabled CLI binary SHA256 is
`167e40a3a9d6a877274dcf2257bebd23df30f14275ccc7062ad5a6781f661989`.
Original failures, all successful reruns, artifact identity records and hashes
are retained separately for independent review.

## Independent execution and final aggregate

The independent harness ran 41 CLI commands: 12 successful operations and 29
expected rejections. All five groups passed. It checked compiler versus runtime
and output CWD, an unused selected constant changed only during final C
compilation, artifact-named executable rejection, six malformed artifact
classes, and eight argument bypass attempts rejected before compiler launch.
Existing outputs survived failures and rejected runs produced no execution
marker. The exact importer-enabled binary hash above was checked before and
after the probes. No unexpected failure or skip occurred.

The final clean source `19f0e58cb21705601a9d872ce8bcfcd18a032b07` passed the
complete matrix from 23:15 to 23:19 UTC on 3 November 2026:

- `cargo fmt --all -- --check`.
- `cargo test --locked --no-fail-fast -- --nocapture`: 678 passed, zero failed
  or ignored, no dependency skip notices.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo test --locked --features c-import --no-fail-fast -- --nocapture`:
  726 passed, zero failed or ignored, no dependency skip notices.
- `cargo clippy --locked --features c-import --all-targets -- -D warnings`.
- Both default and importer-enabled `cargo check --locked --target
  x86_64-pc-windows-gnu --all-targets` modes. These are compile-only checks.

Each build configuration freshly compiled the final repository source after a
hash-preserving source-root timestamp refresh. The source stayed clean at the
same commit throughout. Actual configured and direct/generated SDL3 execution
is included in these Linux suites; these runs add no physical-input, hardware-
renderer, Windows-runtime, Zig or AArch64 execution claim. The subsequent
checkpoint changes only verification/status documentation.

The configuration is not a hermetic lock or cache identity. Trusted inputs must
remain stable during a command. Non-Unix process-tree supervision, aggregate or
callback ABI forms, foreign lifetime/effect contracts and P5/P11 phase completion
remain outside this bounded increment.
