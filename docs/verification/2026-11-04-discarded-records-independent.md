# Independent discarded-record verification

Date: 2026-11-04 UTC.

Status: **independent bounded verification passed; full aggregate acceptance is
separate**. No production defect was found. No full PLAN phase or general
layoutless storage support is claimed.

## Exact source and changes

- Accepted enum-discard baseline: `312aa4fd714e5d7824b57b2ab05d85a5aeff14b9`.
- Record implementation reviewed: `0c525142bc95663b3cbeb77d08842603d29bedd8`,
  tree `9289267743603fab903c64d36c7a71c905198fa7`.
- Clean author handoff: `f9bb1c46ce5ab0f32ac8e7bc5f3c46974f6aa558`.
- Independent test addition: `10cd1b5c4b1dbbe8de44d2efc45379282602cd79`.
- Final tested source, including a test-only lint correction:
  `bd76587a4fafbb5d83cadb12acc913ffc18af934`.
- Final tested tree: `6bcca11365a1700a1486dbc7761fc825fd7727c9`.

Changes are three private test modules, their `#[cfg(test)]` declarations and
one standalone public probe. This report is a documentation-only addition after
that tested source. No production implementation was edited during independent
verification. The existing branch and other references were preserved; no
publication, backup mutation, toolchain install, new checkout/target or manual
source/cache/evidence deletion was performed.

## Final executed results

- **24 private tests passed**, zero failed/ignored. The fresh executable contains
  481 library tests; this run selected the 24 independent record tests and
  filtered out the other 457. It is not a full library execution.
- **24 explicitly typed public/module positives passed**, with reference/default
  interpreter parity, standalone verification and typed C. The ordinary public
  fixtures also require unchanged source, repeated deterministic emission and
  default C identical to typed C. Reference fallback never counts as typed support.
- **9 representation exclusions and 7 source-checker negatives passed**.
- **156 C compilations and executions passed**: 24 ordinary programs and 15
  separately instrumented exact event variants, each with GCC and Clang at O0
  and O2 using `-std=c11 -pedantic-errors`. Signals/timeouts do not count as traps.
- Both optional-origin APIs correctly reject the module fixture after ordinary
  semantic lowering, verification and execution succeed.
- Repository formatting, separate standalone-probe formatting and default strict
  all-target Clippy passed.
- All final C compiler diagnostic files and the final Rust public-probe compiler
  diagnostics are empty. This does not describe unrelated legacy suites.

Two earlier successful private/public passes are also retained. Across the three
public runs there are 468 C executions; the unique final matrix remains 156.
Full default/importer tests, importer lint and platform compile checks remain
separate integration gates.

## Independent proof and event coverage

The tests distinguish canonical schema indices from written child ordinals.
Index-only swaps, child-only swaps and complete-pair reorders are rejected even
when the two fields have the same i32 type. Other mutations cover duplicate,
missing, extra/out-of-range fields, wrong record/enum nominal identity, argument
arity and full child types, compilation brands, opaque scopes, occurrences,
source field roles, exact Observe/Discard callees, full record declarations and
lexical nonuse recounting.

Inactive owner/native schema mutations modify both retained declaration copies.
They are rejected in ordinary discarded record results, body-only records and a
record present only in an omitted interrupted suffix. Complete independent
pure-source reconstruction therefore remains exercised rather than relying on
physical layout or only executed children. Missing dead-body logical metadata,
excessive actual record width/depth and logical depth in an unexecuted late field
are rejected before consumers.

The record-rule budget has an independent analytical oracle, not only a measured
self-boundary. For the three-field R fixture with written right/hole/left order,
the helper costs **127 units**: 13 fixed units plus field costs 44, 46 and 24.
Exact and one-short reservations, four repeated calls sharing one budget, a
four-times-name-byte-length cost delta, complete nested substitution and a
33-field reverse-order mapping all pass. These are bounded work checks, not an
exhaustive allocation-stress proof.

A consistently changed record-pair order, with corresponding source child roles,
can pass standalone retained verification but must fail the separate relation to
the original CheckedProgram. A coherent field-literal change is tested the same
way. This distinction avoids claiming that standalone APIs possess an original
AST they were never given.

Flow tests independently establish normal completion for empty/returning records
and retain the recursive dependency of a bare nonreturning field. Changing a
record's stop field to a normally returning computation changes reconstructed
IR flow and cannot satisfy the old proof. A normal discarded call cannot justify
a NoNormal successor.

Execution cases include written order, nesting under records/enums, complete
nominal substitution, forwarding, conditionals, represented matches, dormant
branches, root/projected/sibling loans, once-only computed reads, distinct demand
instances, early/late arithmetic traps, direct generic divergence, represented
interruption adapters, omitted suffixes, vector fields and the main-inclusive
32-frame boundary. Private evaluator probes retain the original mutable caller
slot: early trap leaves 0, late trap leaves 1, and direct generic S.stop=spin()
leaves exactly 1 while retaining the unexecuted last field.

## Module identity and retained exclusions

The module-loader fixture deliberately verifies that a record type's resolved
name differs from its sole constructor's resolved name. The resolver assigns
Type and Value symbols different identities. Both semantic consumers and C
execution succeed with that canonical representation; no invalid requirement
that the two spellings match was added.

The source-only audit of integration/support/private-test rejection tables found
no extra direct-record negative beyond the four author migrations. The adjacent
`r.value` generalized-local-shape projection remains excluded. Observed/used/
mutable layoutless results, arbitrary unknown argument storage, unknown nominal
interruption results, source errors, existing owner/native boundaries and
optional-origin restrictions were not weakened. Direct Store{value=forever()}
now lowers but executes a real runtime trap, never normal 42.

## First failures and corrections

The first standalone Rust probe compile failed before any execution because of
one missing escaped brace in a format string and one attempted external access
to private `TypedMain.adts`. These were fixture errors. The corrected public
probe uses public APIs; exact no-layout assertions remain in the private tests.
The original prepared source and `public-build-1.stderr` are retained.

The first strict Clippy run rejected a test selector written as `0 | 1 | 2`.
Test-only commit `bd76587` changes it to `0..=2` without suppressing the lint.
`clippy-1.log` is retained. All private/public tests, C executions, formatting and
strict default Clippy were then rerun against that exact final source. No test,
failed diagnostic or production case was silently dropped.

## Freshness, resources and reproduction

Toolchain: rustc 1.99.1, Cargo 1.99.1, GCC 14.2.0, Clang 19.1.7, Linux x86-64.
Two Cargo jobs, zero dev/test debug information and incremental compilation off
were used. Source root bytes were hashed before their timestamps were refreshed,
then verified unchanged after testing. Final Cargo JSON records `fresh: false`
and the actual source path for the library and private executable.

Every Rust/C build and strict lint invocation had an explicit resource guard.
Across 480 recorded samples, minima were 2,097,336 KiB persistent free,
2,456,208 KiB RAM free and 5,990,700 KiB MemAvailable. Required floors remained
2,097,152 KiB for both filesystems and 3,145,728 KiB memory. No guard failed.
Persistent storage is never inferred from the RAM working directory.

Evidence root: `${CARGO_TARGET_DIR}/discarded-records-independent-review`.
The execution environment supplies the sole target on RAM. Key final files:
`private-build-3.jsonl`, `private-artifact-3.json`, `private-run-3.log`,
`lib-build-3.jsonl`, `library-artifact-3.json`, `public-build-4.stderr`,
`public-probe-4`, `public-run-4.log`, `public-output-4/`, `clippy-2.log`,
`fmt-final-2.log`, `probe-fmt-final-2.log`, `final-source-3-before.sha256`,
`final-source-3-after.log`, `evidence-summary.json` and `evidence-manifest.json`.
The manifest includes generated source, executables, runtime outputs and
unmodified compiler diagnostics.

Run private probes with `cargo test --lib independent_k4r`. The standalone
`tests/support/discarded_records_independent_probe.rs` is compiled with
`--extern rill=<fresh rlib>` and the target dependency directory; Cargo does not
automatically execute it as an integration test. It requires a fresh
`K4_PROBE_OUTPUT` directory and `K4_RESOURCE_GUARD` pointing to the existing
`tests/support/discarded_resource_guard.sh`. That guard requires explicit
`K4_PERSISTENT_ROOT` and `K4_RAM_ROOT`, with no working-directory fallback.

Final SHA-256:

- Library, 29,576,268 bytes: `f48f5b5020366fa9346023a0e3ea77044ac1d3f1c153892410e9fb9bc3d609d6`.
- Private executable, 15,739,656 bytes: `0a826d71cf423dc246fa8b1f63b180d2a5af6cc93ba047463cd76c575e3b3f0e`.
- Public executable, 15,313,576 bytes: `f5e496f8ea4e39759fe37336fe7cb1ada3405796bb2185224906c489444f7894`.
- Private run log: `8bca56210adce82f03fa8274d629e5eb469c884ff2fe4237e8e2ef85b7d6995c`.
- Public run log: `ea284bca460cd5aed2e7fbe5f106184721e96f0cc28cbc02d08ce423c17ff2f1`.
- Evidence summary: `a6c6ceacee2aecc910a43babb8d0a00af0c83293e7e42d43c316708836681c2a`.
- Evidence manifest: `b4f472efd291a2e69087557f20879ac53314c8e79e7ca78c14caba13fde190c0`.
