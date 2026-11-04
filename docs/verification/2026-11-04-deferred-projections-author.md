# Deferred nominal projections: author verification

4 November 2026. Author checks pass on executable/test source
`02d5aa5bcefe8bbf0eeeabafbf7c4fe3e9eb0594`, tree
`b91411683385baa5759709e7170c0f26ad2f2bd4`, descended from accepted source
`81b329ff2fb96b589999b4c771378df57c436ff2`. The following documentation-only
checkpoint changes no executable or test inputs. Independent review/execution
and final aggregate acceptance remain pending. This does not complete P1, P2,
C02 or the full language plan. See the [bounded contract](../deferred-projections.md).

## Final focused checks

The final generation-2 runs, with normal Rust flags, passed:

- `cargo test --locked --lib --test deferred_projections --test deferred_projection_origins --test recursive_signatures --test result_annotations --test deferred_projection_execution -- --nocapture`:
  **543 tests** (510 library, 8 semantic, 7 origin, 7 signature, 10 annotation and
  1 C execution harness), 14:01:59–14:03:16 UTC with GCC.
- Twenty affected integration suites: **142 tests**, 14:05:29–14:07:58 UTC.
- Complete new Clang C matrix, 14:03:48–14:04:53 UTC. Its repeated Rust harness
  test is not added to the selected test count.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The **685 selected Rust tests** are not a full aggregate count. The affected
suites were `type_solver`, `value_restriction`, `typed_local_values`, `records`,
`record_audit`, `typed_records`, `field_loans`, `nested_field_loans`,
`read_temporaries`, `typed_field_loans`, `typed_copy_loans`, `loan_scopes`,
`recursion_guard`, `recursive_signature_execution`, `recursive_signature_origins`,
`typed_foreign`, `typed_foreign_raw`, `owner_borrow_checker`, `owner_adt` and
`owned_bytes_checker`. Existing accepted owner/native regressions were unchanged.

Final selected runs report zero failures, ignored tests or SKIP notices. GCC,
affected regressions and Rust lint report no warnings. Clang reports **eight
`-Wparentheses-equality` warnings** from the unchanged typed C emitter: the two
selective-Pair fixtures and polymorphic nested reads, across both optimization
levels. These warnings are retained, not suppressed or counted as failures.

## Execution and semantic evidence

The matrix retains **45 fixtures: 42 returns and 3 traps**. All pass source
checking, canonical field metadata checks, independent typed verification, typed
and default interpreters, and the reference interpreter. Production C output is
asserted byte-identical to direct typed C emission.

There are **340 new matrix C11 executions**: 170 GCC and 170 Clang, each comprising
90 typed/production and 80 uniform-reference executions at O0/O2. Five local-shape
fixtures explicitly reject in the reference C backend, as described below; they
still execute production C at both levels with both compilers. No fixtures are
silently dropped. Both documented examples separately pass direct CLI checks and
interpreter execution, plus **8 C executions** across GCC/Clang O0/O2; those are
not added to the 340-case-matrix total.

Coverage includes both generic SCC orders, all six intermediate-parameter orders,
singleton later equalities, dependent and partially resolved paths, exact
quantifier membership, unrelated and selective local generalization, once-only
read instantiation, name-segment loan overlap, shadowing, original-slot writes,
effect order and snapshots. Executed recursive back-edges return 42 at 31 and 32
active calls (including main) and trap at 33, preserving the existing call guard.
Forty negative source scenarios reject through all seven public consumers,
including sixteen conflicts reached while the nominal root is unknown.

Library tests cover exact budget exhaustion, wide shallow pending inputs,
long schema/name work and atomic rejection without a CheckedProgram. Typed
record/field-loan tests independently reject corrupted canonical metadata and
write authority. Seven module/source tests retain exact nominal identities,
privacy, binding IDs, source roles and the existing optional-origin boundary.
Only compatibility corpus case 5 and the documented intermediate-parameter
rejection were migrated.

## Inherited reference-C boundary

The two unrelated-None cases, two selective-Pair cases and polymorphic nested
reads assert the exact unchanged diagnostic:

`E-C-ADT: unresolved polymorphic value requires local specialization`

The accepted reference layout analyzer substitutes enclosing function schemes,
not locally generalized schemes. Existing typed-local-value tests intentionally
compare the reference interpreter and production/typed C without requiring
uniform reference C for these shapes.

A fresh archived immutable accepted `81b329ff` baseline reproduced the boundary
for the known-order unrelated-None and selective-Pair fixtures and the independent
polymorphic-read fixture. Analysis, typed verification, all three evaluators and
production/typed C emission passed, and reference C returned that exact error.
This baseline probe did not execute C. Its standalone Rust probe reported one
unused-field warning. No reference-C layout expansion was made.

## Retained failures and artifact isolation

The early clean implementation checkpoint `b051d208` passed 510 library and 32
focused tests before its durable backup. These repeated checks are not added to
the final count. Preliminary fixture-only failures concerned generic declaration
spacing and a test assumption about the typed discard-let shape; both were fixed.

The archived baseline initially used the former shared target. Subsequent current
source probes exposed stale cross-checkout build artifacts and failed before any
C execution. Rebuilding only the library did not invalidate the stale harness.
Those logs and the later explicit-Rust-flags recovery run remain retained but are
excluded from final evidence counts. No caches or source history were deleted.

All final evidence instead uses the fresh, exclusively current-source
`target-generation-2` with `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS` unset. No
baseline builds use this generation. Its source-input and binary/library hashes
are retained separately.

The first Clang attempt in generation 2 hit a retained-directory name collision
from reused container process IDs before any compiler invocation. This was a
pre-execution harness issue, not a compiler failure. Timestamped artifact names
preserve prior evidence; both complete matrices were then rerun from their start
on the final harness source.

## Guards, environment and remaining gates

The final timestamped harness was exercised with injected resource rejection.
First-admission denial exits 86 after one guard and zero compiler-spy/executable
calls. Pre-execution denial exits 86 after two guards, one compiler-spy call and
zero executable calls. Neither writes a later shell marker. The fixed resource
guard also runs during these probes. These are serial control-flow tests, not
real C executions or cancellation guarantees for already-running children.

Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7 use the frozen toolchain, two Cargo
jobs, zero dev/test debug information, disabled incremental compilation and
serial tests. Fixed 2 GiB disk and 3 GiB MemAvailable floors pass; no actual
resource floor failed. New harness compilation and execution have separate
guards and subprocess deadlines. All temporary evidence is retained.

Evidence is retained under `deferred-projections-author-20261104T132500Z`:
final stages 17–23, raw outputs, compiler artifacts, baseline proof, injected
probe counts and hash manifests. Earlier failed and repeated stages remain
separately labeled and are not added to final counts.

Independent execution and the complete default/importer/lint/Windows/SDL
aggregate remain mandatory after the final freeze. No new layout, owner/native,
GPU, physical-input, Windows execution or completed-phase claim follows.
