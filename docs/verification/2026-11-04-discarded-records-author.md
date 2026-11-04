# Direct discarded records: frozen author evidence

Status: **author-verified candidate, not integrated acceptance**. The accepted
baseline remains the preceding enum-discard increment. Independent execution,
full default/importer tests, both final lint modes and platform compile checks
remain separate acceptance gates. This does not close P1 or any full PLAN phase.

## Exact source and freshness

- Accepted base: `312aa4fd714e5d7824b57b2ab05d85a5aeff14b9`.
- Production/test implementation: `0c525142bc95663b3cbeb77d08842603d29bedd8`.
- Final tested source, including candidate contract docs:
  `93046a1da98f84585fe06464fe905c071a668856`.
- Final tested tree: `89af0dca81ef660d4f53a41c8821590a18d56f47`.
- This record and its linked status updates are documentation-only additions
  after that tested source.

The checkout was clean for final focused testing. Root source bytes were hashed,
their timestamps refreshed to force actual compilation, and the hashes checked
unchanged. Cargo JSON records `fresh: false` and the actual source paths for the
library, binary, library-test executable and all 15 selected integration-test
executables. The retained ledger identifies 19 emitted artifacts, counting the
library metadata file separately. The final Clang run used the same freshly
compiled new-suite executable; it did not claim another fresh Rust compilation.

Environment: Linux x86-64, Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7.
Two Cargo jobs, no incremental compilation and zero dev/test debug information
were used throughout. The source and sole target stayed on the authorized RAM
filesystem. No toolchain installation, manual deletion, new checkout/target,
remote publication or backup mutation was performed by the author.

## Executed results

The final selected command passed **640 tests**, with zero failures or ignored
tests:

| Selected cohort | Passed |
|---|---:|
| Library, including 14 new record proof/execution tests | 457 |
| Direct discarded records | 17 |
| Earlier discarded results, including migrated direct R | 10 |
| Phantom products, including two migrated Store cases | 23 |
| Nullary phantoms | 20 |
| Local values | 17 |
| Interrupted arguments | 8 |
| Nonreturning results | 10 |
| Represented records | 8 |
| Field loans | 6 |
| Copy root loans | 4 |
| Native typed transport | 18 |
| Raw native typed transport | 20 |
| Existing owner ADTs | 13 |
| Existing owner borrows | 3 |
| Existing owner calls | 6 |

The selected compiler defaults to GCC; older suites that explicitly exercise
both GCC and Clang retain that behavior. This is focused breadth, not the full
repository suite. No selected run reported dependency skips. Existing owner
controls ran unchanged; no new owner-record implementation or acceptance is
claimed. The oldest suites retain their normal teardown of their own newly
created temporary files. New record evidence and the selected newer suites were
retained; the K2 helper now also honors the existing artifact-retention switch.

The new record suite additionally passed **17/17 under Clang**. Its 47 C fixtures
per compiler, comprising 40 ordinary cases and seven instrumented variants,
compiled and executed at O0 and O2: **188 final C executions**. Every compile
used `-std=c11 -pedantic-errors`; every process was bounded. Signals and timeouts
are not accepted as runtime traps. Final new-suite compiler diagnostics are empty.
The earlier development GCC/Clang pair also passed, but its repeated 188
executions are not counted in this final total.

The existing standalone K4 public harness was rerun as an **author regression**,
not as new independent review. It passes 36 explicitly typed positives, 11
remaining representation exclusions and eight source-checker negatives, with
**216 additional GCC/Clang O0/O2 C executions**. This includes the fourth migrated
fixture, `layoutless_record`, and preserves the adjacent projected-local-shape
negative. Two legacy weak-mutable K1 match builds retain two Clang
`-Wparentheses-equality` warnings each. Their complete stderr is retained; this
older matrix is not warning-free. No failed or warning-bearing control was dropped.

Repository formatting and default strict all-target Clippy passed. Full
importer-enabled execution, final aggregate lint/platform checks and independent
record execution are pending, not implied by these results.

## Semantics and adversarial checks

The [bounded contract](../blockers/discarded-results.md#direct-record-follow-on)
adds a separate record discard node, carrying canonical schema indices in
written evaluation order. Layout registration skips discarded record and enum
products while retaining represented call arguments, locals and loaned places.
The evaluator and C emitter sequence all actual fields without aggregate storage,
result slots, dummy values or assignments from void calls.

Public cases compare explicit typed lowering/verification/evaluation with both
reference and default evaluation, require default C to equal typed C, check
repeatability and unchanged source, and separately record uniform-C rejection.
They cover nested records/enums and full nominal substitution, empty normal flow,
forwarding, branches/matches, dormant branches, body-only metadata, root/field/
sibling loans, computed reads and snapshots, distinct Observe/Discard calls,
early/late traps, bare nonreturning fields, represented interruption adapters,
and exact main-inclusive 32-frame entry/argument/leave traces.

Private tests independently reject definition/kind/arity changes; duplicate,
missing, extra and out-of-range field mappings; equal-typed field or pair swaps;
wrong logical arguments, foreign brands and occurrences; malformed source roles
and full declarations; inactive owner/native dependencies, including source-only
interrupted suffixes; hidden-use and binding-token fraud; missing dead-body
metadata; cross-demand calls; and excessive actual depth/width before consumers.
They inspect the original mutable caller slot after early/late/interrupting
traps. A consistently edited retained witness and IR can remain internally
valid but still fails the separate original CheckedProgram comparison.

Direct record-rule tests measure label lookup, name comparisons, full schema
substitution and child queueing, with exact and one-short budgets, repeated
operations sharing one cumulative budget, nested nominal arguments and a wide
reverse-written-order record. Flow tests establish true empty/returning record
flow and retain real recursive call dependencies; a forged normal stop field
cannot satisfy the source relation. Existing 100,000-work, 1,024-instance and
128-depth limits are unchanged.

The first private-test compilation exposed two ordinary Rust borrow errors in a
schema-mutation fixture; both were corrected before the frozen implementation.
All subsequent private/public executions and strict lint passed. No production
semantic defect or additional stale direct-record rejection was found in the
completed author checks. Independent execution may still identify new issues.

## Evidence and resource bounds

Retained evidence is under `${CARGO_TARGET_DIR}/discarded-records-author`, with
that target supplied by the execution environment. Important files are
`focused-final.jsonl`, `focused-final.stderr`, `focused-artifacts.json`,
`root-before-focused.sha256`, `root-refresh-focused.log`,
`public-clang-final.log`, `k4-regression.log`, `k4-regression-output/`,
`clippy-final.log`, `fmt-final.log`, `evidence-summary.json` and
`evidence-manifest.json`. Development logs, including the first failed private
compile, are preserved beside them.

Builds had explicit guards for the persistent filesystem, RAM filesystem and
MemAvailable; no guard inferred persistent storage from the RAM working directory.
Across 1,963 retained guard samples, minimum free space was 2,097,360 KiB
persistent and 2,488,420 KiB RAM, with 5,475,060 KiB MemAvailable. Required floors
were 2,097,152 KiB for each filesystem and 3,145,728 KiB memory. No guard failed.

SHA-256:

- Library: `cd652b2a2a50e231e2b812868b925324f83c4ac6cdb1c2b47d1a499755b6487c`.
- Library-test executable: `2212e81a9692b6a60b39f66b8b5ceec0182cdc7d629af6ac4afd83f822a4fde1`.
- New public-test executable: `2d0d8faab07ca6a96d0e1b49703ee2ee35fe5e91cf8a879bfa7f8c7985478bc7`.
- Evidence summary: `e7ae3b7f838119e3c7187e1c3759c1ee19b50280466edb2c12f4a1c62d965dee`.
- Evidence manifest: `bfa38ffc1ff40a3482ffb191d3c706c658660ad22592f883f11a000d5d066204`.

No new SDL capture, hardware input, GPU, Windows runtime, Zig or AArch64 execution
claim is made by this bounded record slice.
