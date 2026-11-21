# Direct discarded records: integrated acceptance

The exact tested source is `9dae2324b1196c71b84b660af12afb3dd54d92b9`, tree
`ee4b545541b5e9563f842298ebcf9eeefd302df3`. Production and author tests were
frozen at `0c52514`; the author handoff was `f9bb1c4`. Independent tests and a
test-only lint correction were frozen at `bd76587`, followed by the independent
report. This acceptance record and status updates are documentation-only.

## Full matrix

All seven stages passed on 4 November 2026, from 04:30:46 to 04:34:59 UTC:

- Repository formatting.
- Default tests: **996 passed**, zero failed or ignored.
- Default strict all-target Clippy, with warnings denied.
- Importer-enabled tests: **1,044 passed**, zero failed or ignored.
- Importer-enabled strict all-target Clippy, with warnings denied.
- Windows GNU all-target compile checks, default and importer-enabled.

Neither full test run reported dependency skips. The actual SDL3 integration
ran with SDL3 3.2.10; no new screenshots, physical-input or GPU evidence are
claimed. Windows checks compile only, not runtime verification. This matrix
adds no Zig or AArch64 execution claim.

The exact source was clean before and after all stages. Byte-preserving root
timestamp refreshes forced compilation from that checkout. Rust/Cargo 1.99.1,
GCC 14.2.0 and Clang/libclang 19.1.7 were installed. Builds used two jobs,
no incremental compilation, zero dev/test debug information, and unchanged
persistent/RAM/memory guards.

Default test-log SHA-256:
`063bbf29cad58e5334f390b18d6fbb4e5755e60a9cb1a8fe9383d025d4b17059`.
Importer-enabled test-log SHA-256:
`31923d1950452b70a3f24edaf5c7244bcb0a21c14586b37a3a572e580cd5ccf4`.

## Author and independent evidence

The [author record](2026-11-04-discarded-records-author.md) reports 640 focused
tests, 17 additional Clang tests, **188 new-suite C11 O0/O2 executions** and
216 executions of the migrated older regression harness. The new record suite
is diagnostic-free. Two legacy K1 Clang warning files remain documented in the
older harness; its success is not a warning-free claim.

The [independent record](2026-11-04-discarded-records-independent.md) reports
**24 private proof/source-relation tests**, 24 typed public/module positives,
nine retained representation exclusions, seven source-checker negatives and
**156 GCC/Clang C11 O0/O2 executions**, including 15 exact event variants.
Both optional-origin APIs retain their documented rejection after successful
typed semantics. Independent C execution produced no diagnostics or production
defect. Initial harness compile mistakes and a lint-only selector correction
were retained and repaired without suppressions or production changes.

The private modules are included in the aggregate. The public/module/event
probe in `tests/support/discarded_records_independent_probe.rs` is a separately
executed standalone harness, not automatically discovered by Cargo.

Independent coverage includes equal-typed field/index/full-pair reorderings,
complete unique schema coverage and substitution, Type/Value module namespace
separation, opaque brands/scopes, inactive owner/native schemas in omitted
suffixes, lexical nonuse, actual caller state after traps, positive-flow
reconstruction, actual-tree depth/width and the analytic 127-unit mapping-rule
budget boundary. Internally coherent witness/IR edits remain distinct from the
separate original CheckedProgram comparison. No exhaustive soundness theorem
or complete allocation-stress proof is claimed.

## Accepted scope and next work

The [bounded record contract](../blockers/discarded-results.md#direct-record-follow-on)
retains schema indices and logical-only children in written evaluation order.
Direct discarded records execute each field once, preserve real calls and
traps, and create no unknown aggregate/local storage or fabricated value.
Every field remains part of mandatory source/schema/order verification even
when an earlier field cannot return. Empty records return normally; discard
is not itself a nonreturn seed.

Four older negative fixtures were explicitly promoted to the newly supported
boundary. The record containing `forever()` now lowers but actually traps;
it does not continue normally. Used, mutable, aliased, projected or observed
layoutless records, genuine unknown storage, contextual local-shape mixing,
unknown nominal interrupted results and new owner/native combinations remain
outside this increment.

The next bounded source-usability task is closed function-result annotations,
using existing concrete types rather than inventing layouts or default types.
That syntax is not implemented by this checkpoint. Existing fallback remains.
No full P1, P2, P5 or other PLAN phase, stable ABI, deployment or release is claimed.
