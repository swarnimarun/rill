# Integrated Copy function-body origins

Verified 3 November 2026 on exact source
`e805e3cf995fc29b1f5bdf29bd82d0bcb8b59942`. The verification report and accompanying
README/status updates are documentation-only changes after those checks.

## Bounded capability

The separate optional `lower_main_with_copy_node_origins` API owns the ordinary
verified typed program, exact source snapshots, function-instance index and
validated Copy-body origin tables. It covers function roots, parameters/results,
expressions, call operands, field tuples, Copy patterns, local captures and
shape materializations. Generated, erased, embedded and source-derived cases
remain explicit. Snapshot-branded handles prevent mixing tables from different
lowerings; ordinals are deterministic within a snapshot, not cross-edit IDs.

Optional construction observers share the ordinary semantic lowering path.
Independent final structure, source-route, lexical and shape checks validate
correspondence rather than zipping source and IR preorder or guessing parent
spans. Work reservations and shared failure latches preserve ordinary error
precedence and prevent partially successful maps. Generated C is not instrumented.
See the [API contract](../typed-copy-node-origins.md) for precise coverage,
queries, budgets and exclusions.

The combined native seam preserves F2's full manifest, pointer registry, raw
operations, C-string traversal and existing backend/evaluator behavior. Both
origin APIs explicitly reject native representation after ordinary successful
lowering and before metadata checks. This includes inactive raw fields and
phantom arguments missed by the shallow checker flag. The older function-only
index retains its accepted owner subset; the new body index remains Copy-only.

## Final full-source verification

| Check | Result |
| --- | --- |
| Formatting | passed |
| Default complete tests | 661 passed |
| Default strict all-target Clippy | passed |
| Importer-enabled complete tests | 695 passed |
| Importer-enabled strict all-target Clippy | passed |
| Windows GNU default all-target compile | passed |
| Windows GNU importer-enabled all-target compile | passed |

Commands were the repository's locked full-test/strict-Clippy commands, with
`--features c-import` for importer coverage and `--target x86_64-pc-windows-gnu`
for the separate compile checks. Both full configurations reported zero failed,
ignored or dependency-skipped tests. Existing real SDL/importer/native execution
regressions passed within the full suites. No Windows runtime, GPU rendering,
physical-input or additional screenshot result is inferred from those checks.

The Linux environment used Rust/Cargo 1.99.1, GCC 14.2.0, Clang/libclang 19.1.7
and SDL3 3.2.10. Source remained clean and identical throughout all seven stages.
Fresh package builds were forced by hash-preserving crate-root timestamp refresh,
with exact checkout/package/artifact identity retained to prevent stale
cross-worktree library reuse.

## Focused and independent evidence

Before the aggregate gate, the exact integrated source passed 172 focused tests:
38 private provenance tests, 117 public/native/affected integration tests and
17 independent native-verifier unit tests. Formatting and strict all-target
Clippy also passed. The isolated pre-native source had passed 137 focused tests;
those historical results are distinct from the combined-source counts.

Public tests compare ordinary, reference and mapped evaluation plus exact
emitted-C bytes. Three representative cases execute generated C at both O0 and
O2. Adversarial tests include valid same-typed binder/local substitutions and a
complete alternate return-only function instance whose edited IR and refreshed
construction fingerprints still pass ordinary verification. Source
correspondence rejects the wrong mapping. Tiny limits exercise pre-scan and
pre-growth charging, child order, reverse coverage and both observer-latch
directions. Invalid fixture spacing found during development was corrected;
compiler/test evidence was retained rather than relabeled as an implementation
pass.

An independent fresh build and two external public-API probes passed 39 cases:
11 pure/owner/native/error-precedence cases and 28 integrated native/pure
controls. They check deterministic public rows, source reverse coverage,
foreign-handle rejection, exact ordinary/reference/mapped evaluation and C-byte
parity. Eighteen raw-layout cases have `checked.has_c_values == false`, covering
inactive, phantom and nested raw metadata. Both origin APIs return the explicit
native boundary without changing ordinary C output. The independently verified
library SHA-256 was
`1a44f5e74aaeb5fc307241ff9f033ecfe7d13e8b7bed0c30b038d4b4a0310c6f`.
Those external probes compare C text; they do not claim additional native C
execution beyond the separately recorded repository tests.

## Remaining gates

No complete owner initialization/cleanup, native ABI provenance, source storage
map, package/directive map or generated-C byte-range map is claimed. The API is
not a stable cross-edit identity or a proof of inference/semantic correctness.
P1, C18, P6 debugging and the remaining language phases stay open. Native
configuration persistence and unverified owned-record increments are separate
work and are not included in this checkpoint.
