# Nonreturning results: integrated verification

## Accepted source and scope

The production candidate is `f387a545fdac417be1aed9c1eb9329b75613f048`.
Independent regression tests were retained at `80835c59`; the final tested
source is `bdd878feea85809a5dc741eeda943a17ce0cebc9`. Changes after the
production candidate are test-only. The baseline is accepted K2 checkpoint
`984f75f`. This bounded K3 acceptance closes no full PLAN phase or stable ABI gate.

The [representation contract](../blockers/nonreturning-results.md) describes
separate logical no-normal results and ordinary value storage. Checked-source
and exact-instance IR control analyses independently prove the absence of a
finite normal-return path. A mandatory bounded source relation preserves actual
operators, values, arguments, places, bindings, branches and continuations.
Original-source correspondence is checked against the actual CheckedProgram;
standalone verification establishes correspondence to its retained witness.

Direct/mutual bare-result recursion, concrete prefixes and branch inputs,
unused immutable divergent lets and bare main results are admitted. Concrete
functions retain their value ABI. Bare results use internal C void signatures,
not an invented unit or integer. Real arguments and recursive calls execute
through the existing 32-active-frame guard. No result printer/storage is emitted
for a bare result. Source functions gain no unsupported noreturn promise.

Eager layoutless operands/arguments, used or mutable layoutless bindings,
unknown stored payloads, unsupported local-shape combinations and new
owner/native combinations remain outside this increment. In particular,
`ignore(forever())` and `let x = forever(); x` need separate work. Optional
origin APIs explicitly reject the new metadata after ordinary verification.
Inference, generalization, ownership and foreign ABI rules are unchanged.

## Author and independent execution

The exact production candidate passed 266 typed unit tests, formatting,
default strict all-target Clippy, and ten source acceptance tests with GCC at
O0/O2. The same ten Clang tests passed before the final conservative Binary
snapshot reservation and style-only cleanup. The independent final-source
matrix and full aggregate below cover the completed production code.

Independent execution retained fifteen new private/relation regressions and
passed all 388 unit tests, including 281 typed tests. It also passed:

- Fifteen positive source cases, including three ordinary closed controls.
- Five expected checked-source/representation rejections.
- Five exact event-trace cases under GCC and Clang at O0/O2.
- Eighty compiled C executions in total, with no unexpected C diagnostics.
- Formatting, strict default all-target Clippy and diff checks.

The matrix uses C11 with `-pedantic-errors`. It checks actual source-main
execution, 31 entered recursive prefixes, 32 boundary argument mutations,
argument evaluation before the rejected 33rd frame, value/computed-read
snapshots, disjoint field loans, dormant branches and distinct weak-result
identities. Private cases cover removed/foreign proofs, original CheckedProgram
changes, retained dead tails, lexical/field/pattern mutations, unused native
metadata and exact structural-depth 128/129 boundaries. No implementation defect
was found during this independent execution.

Fresh Cargo JSON identified the exact checkout and reported `fresh:false` for
production and final unit artifacts. Root timestamp refreshes preserved bytes.
The production rlib SHA256 was
`23c671bc803d95e1f94299a4d89372e7398a5088b0b9839c3affbb5fd8de80bc`;
the final independent unit artifact SHA256 was
`11ae0bbf49de745c4df0916e969b13c77f9a11c3483db0e658b14edbae57e28d`.
No private fields were exposed to satisfy a harness.

## Full final aggregate

On 4 November 2026, 01:43:20–01:47:34 UTC, all seven stages passed on the clean
final source `bdd878feea85809a5dc741eeda943a17ce0cebc9`:

- `cargo fmt --all -- --check`.
- `cargo test --locked --no-fail-fast -- --nocapture`: **860 passed**.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo test --locked --features c-import --no-fail-fast -- --nocapture`:
  **908 passed**.
- `cargo clippy --locked --features c-import --all-targets -- -D warnings`.
- `cargo check --locked --target x86_64-pc-windows-gnu --all-targets`.
- The same Windows GNU compile check with `--features c-import`.

Both test configurations had zero failures, ignored tests or dependency-skip
notices. Before that full run, the combined K1/K2/K3 suites passed 51 tests.
Actual SDL3/native fixtures ran in the supported Linux environment. Existing
[SDL framebuffer captures](2026-11-03-typed-native-raw-integration.md) remain
separate earlier visual evidence; this increment makes no new physical-input
or hardware-rendering claim. Windows results are compile checks, not execution.
No Zig or AArch64 NEON execution is claimed.

The environment used Rust/Cargo 1.99.1, GCC 14.2.0, Clang/libclang 19.1.7 and
SDL3 3.2.10, two Cargo jobs, disabled incremental compilation and zero dev/test
debug information. Fresh compilation was recorded from the final checkout;
source identity stayed clean and unchanged throughout the seven-stage run.

## Retained failures and corrections

Source review before admission corrected precharge ordering/lookup bounds,
canonical ownership of collapsed nodes and pattern/binding arenas, iterative
depth-preflight ordering, and the reservation for every Binary scope snapshot.
Each correction received focused tests before public admission.

Retained development failures include invalid/missing test metadata, parser
fixture syntax/depth errors, a source-loader checking assumption, temporary-name
collisions, private-field access from an external test harness, and test style
lints. They were corrected without exposing private production fields, skipping
cases or changing runtime behavior to satisfy a fixture.

The first full aggregate on `80835c59` found four stale K1/K2 assertions in each
test configuration: they still expected now-supported no-normal cases to be
rejected. All other tests and both lint/platform modes passed. Test-only commit
`bdd878f` retains every genuine unknown-payload negative and adds positive
execution plus post-ordinary-success origin rejection coverage. Its migration
was independently source-reviewed. The complete final aggregate above passed;
the initial failing logs were retained rather than replaced.

## Remaining work

P1 remains open. Next bounded representation tasks are interrupted eager
arguments and separately specified discarded normally-returning results.
Neither may manufacture an unknown value, erase effects or pretend that a
payload-bearing type is phantom. Complete MIR/source mapping, broader language
contracts and later PLAN gates still require their own implementation/evidence.
