# Normally returning discarded results: integrated acceptance

The exact tested source is `99b6fd21798e32d0822070bd3060b7b3cb8787e4`, tree
`c5a93c513b7393d04e2dc1c02e1f5668ba634e0f`. Production changes were frozen at
`ee5763f`; later commits add independent tests, evidence, a portable resource
guard and an explicit migration of one older rejection fixture. This acceptance
record and status updates are documentation-only changes after the tested source.

## Full matrix

All seven stages passed on 4 November 2026, from 03:44:51 to 03:48:58 UTC:

- Repository formatting.
- Default tests: **939 passed**, zero failed or ignored.
- Default strict all-target Clippy, with warnings denied.
- Importer-enabled tests: **987 passed**, zero failed or ignored.
- Importer-enabled strict all-target Clippy, with warnings denied.
- Windows GNU all-target compile checks, default and importer-enabled.

Neither complete test run reported dependency skips. Actual SDL3 integration ran
with SDL3 3.2.10; this checkpoint adds no new visual capture set, physical input
or GPU evidence. Windows checks compile only. No Zig or AArch64 runtime claim
is added by this matrix.

The checkout was clean at the exact source before and after every stage. A
byte-preserving timestamp refresh forced compilation from the tested checkout;
both test logs record that compilation. Rust/Cargo 1.99.1, GCC 14.2.0 and
Clang/libclang 19.1.7 were installed. Builds used two jobs, no incremental
compilation, zero dev/test debug information, and the established persistent,
RAM and memory guards.

Default test-log SHA-256:
`6ca027b86113f6a7d24b3007364f16f1059a523c42475e313e7950d33eaeffa6`.
Importer-enabled test-log SHA-256:
`8fc1bf3638fe7ca616a9f5d8cd4d678850ce3929b16e88febceeada4731b2a1d`.

## Independent evidence and retained corrections

The [author record](2026-11-04-discarded-results-author.md) documents 504 focused
GCC tests, nine Clang tests and 124 C11 O0/O2 executions. The separate
[independent record](2026-11-04-discarded-results-independent.md) documents 25
private proof/source-relation tests, 35 typed public cases, 12 representation
exclusions, eight source-checker negatives, **212 final GCC/Clang executions**,
six dual optional-origin cases and two earlier module-loader rejections.
The standalone public/origin probes are retained under `tests/support`; Cargo
does not automatically discover those two executables. Their separately recorded
execution is distinct from the full matrix above. The private modules do run
in the aggregate.

Independent checks cover exact demand/callee identities, normal entry/leave and
depth-boundary events, ordered effects/traps, hidden lexical uses, full inactive
schemas, weak scopes, original CheckedProgram equality, positive flow, actual
logical depth and frozen-evidence limits. A pre-freeze proof-only suffix gap was
confirmed and fixed: complete retained source purity is now independently
reconstructed even for nominal types occurring only in an omitted interrupted
suffix. Paired owner/native schema mutations reject at all public consumers.
This does not claim an exhaustive allocation stress proof.

The first aggregate on `096eea3` had one failure in each test mode, with 937/985
passes. An old K3b exclusion still rejected the intended unused returning
`absent()` example. Test-only commit `0e77b27` retains its unchanged source as a
named positive typed/reference/C test, preserves five other exclusions, and adds
used/mutable `absent()` negatives. The entire resulting six-test binary passes
with each compiler, including **80 further C11 O0/O2 executions**. No production
change was needed for this migration; initial failure logs remain preserved.

Known Clang `-Wparentheses-equality` diagnostics remain on retained legacy
Copy-match controls: two files in the independent K4 matrix and four in the
migration matrix. The reports retain exact diagnostics. These C matrices are
not warning-free; strict C11/pedantic errors and all Rust warning-denial gates
remain enabled. No failing case or unsupported boundary was silently dropped.

## Accepted scope and next work

The [bounded contract](../blockers/discarded-results.md) adds a normally returning
`Discarded(LogicalType)` computation for lexically unused immutable initializers.
Actual calls and enum operands execute in source order, with real normal returns,
without unknown aggregate/local storage, guessed payload types or fabricated
values. Demand belongs to exact function-instance identity. Full logical schemas,
independent source/IR correspondence, pure-source dependencies and the separate
original-source relation remain mandatory before consumption.

Direct layoutless records are the next structural discard increment. Observed,
used or mutable layoutless values, genuinely unknown storage, local-shape mixing,
unknown nominal interrupted results and new owner/native combinations remain
outside this checkpoint. Existing fallback is not retired. No full P1, P2, P5
or other PLAN phase, stable ABI, deployment or release is claimed.
