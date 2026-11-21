# Deferred nominal projections: integrated acceptance

4 November 2026. The bounded nominal-projection increment is accepted on exact
clean source `81c8c15144d9ad149b08bbc801b45bacd391748c`, tree
`d0cb34eb8013ca4e735838d3719c53c443f4d380`. All seven aggregate stages pass.
The following documentation-only checkpoint changes no executable source,
tests, examples or build inputs. This does not complete P1, P2, C02 or the
full language plan.

## Accepted contract

[Deferred nominal projections](../deferred-projections.md) retain exact per-use
roots, field-name segments and result constraints within one monomorphic
function component. Ordinary equalities may establish nominal identities after
a field read, assignment or read/inout field operand was encountered. Ready
constraints resolve before local generalization inputs are collected; all must
resolve before component generalization or a CheckedProgram is returned.
Completed paths publish the existing canonical FieldInfo. Known paths stay eager.

Source-order body traversal, value restriction, ordinary SCC polymorphism,
left-to-right effects, snapshots and original-slot writes remain. Symbolic source
loans use binding identity and complete field segments, with independent typed
record/loan verification unchanged. Exclusive roots require one mutable monotype;
quantified roots remain immutable and unborrowed. No structural search by field
label, deferred local schemes, new physical representation or new owner/native
form is introduced.

The intentional compatibility migrations are corpus case 5 becoming equal to
case 6 and the formerly rejected intermediate-parameter example becoming valid.
An unresolved standalone projection still rejects at its component boundary,
even if a later caller supplies a record. Universal inference-order independence
is not claimed.

Projection and field-loan work has a cumulative 1,000,000-unit allowance,
4,096 admitted deferred obligations and 1 MiB retained path-name bytes per
analysis, retaining the existing 1..16-segment limit. Admission precedes retained
clones; schema/name work, retries and symbolic comparisons are charged. Eager
field paths can also exhaust this policy. These are deterministic policy bounds,
not a complete compiler memory/time guarantee; solver limits remain independent.

## Fresh aggregate evidence

Seven serial stages passed from **14:37:59 through 15:06:06 UTC**:

- Formatting, 14:37:59–14:38:01.
- Default tests, 14:38:01–14:50:16: **1,103 passed**.
- Default strict all-target Clippy, 14:50:16.
- Importer-enabled tests, 14:50:16–15:04:35: **1,151 passed**.
- Importer-enabled strict all-target Clippy, 15:04:35–15:05:06.
- Windows GNU all-target compilation, default, 15:05:06–15:05:43.
- Windows GNU all-target compilation with c-import, 15:05:43–15:06:06.

Each test mode has 107 test summaries and zero failures, ignored tests, SKIP
notices or warnings. The source stayed clean and ref snapshots were unchanged
throughout the counted matrix. Windows checks are compile-only; Windows binaries
were not executed.

```sh
cargo fmt --all -- --check
cargo test --locked -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo check --locked --all-targets --target x86_64-pc-windows-gnu
cargo check --locked --all-targets --target x86_64-pc-windows-gnu --features c-import
```

The initial relative-path launcher invocation stopped while hashing its own path,
before any Cargo stage. The unchanged runner then completed the counted matrix
when invoked by absolute path. The failed preliminary evidence is retained and
is not counted as a test run.

## Separate focused evidence and provenance

The historical [author report](2026-11-04-deferred-projections-author.md) records
685 selected tests, its 340-execution matrix and eight direct-example C runs.
Two final test-file changes were still uncommitted during its initial stages
17–19. Later hashes match their subsequently committed content, but no complete
pre-run dirty-tree fingerprint exists. Those original at-run HEAD values and
artifact-reuse/harness failures remain preserved; they are not rewritten.

The [independent report](2026-11-04-deferred-projections-independent.md) records
**694 unique selected tests** on clean committed source, **112 independent C11
executions**, and a complete clean-source **340-execution replacement** of the
author matrix. The replacement removes the earlier source-provenance limitation
for those fixtures and selected suites; it is not added to the historical 340.
The focused Rust suites overlap the aggregate and are not added to its totals.

Independent cases cover generic SCC permutations, dependent paths, partial
local generalization, source identity, occurs and authority rejection, one-time
read instantiation, exact schema-work boundaries and atomic admission failure.
The 45-case author matrix retains 42 returns and three traps, including actual
recursive back-edges at 31/32 active calls and a trap at 33. GCC and Clang run
both matrices at O0/O2. Eight known Clang equality-parentheses warnings remain
in the clean author-matrix replay; the separate 112-run independent matrix and
the GCC aggregate are warning-free.

The five author and two independent local-shape fixtures explicitly retain the
uniform reference-C diagnostic `E-C-ADT: unresolved polymorphic value requires local specialization`.
They still pass all three evaluators and real production/typed C execution with
both compilers and optimization levels. None is silently dropped. Static review
and the retained immutable accepted-baseline probe establish the inherited
reference-C layout limit; that baseline probe checked C emission, not execution.
No local-shape reference-C expansion is accepted here.

Both harnesses have separate injected guard probes: first-admission denial exits
86 with one guard and zero compiler-spy/executable calls; pre-execution denial
exits 86 with two guards, one compiler-spy call and zero executable calls. No later
shell marker is reached. These serial control-flow probes are not real C
executions or guarantees about already-running parallel children.

## Environment, native checks and remaining scope

The verified environment uses Rust/Cargo 1.99.1, GCC 14.2.0, Clang 19.1.7 and
SDL3 3.2.10. All current-source checks use target generation 2 exclusively, normal
Rust flags, two Cargo jobs, zero dev/test debug information, disabled incremental
compilation and serial tests. The fixed guards require 2 GiB free disk and
3 GiB available memory. No actual resource floor failed. Earlier target artifacts
and logs are preserved; no baseline checkout was built into generation 2.

Real direct SDL3 ran in both test modes. Generated SDL3 bindings and configured
O0/O2 SDL profiles ran in importer mode. No new captures, Windows execution, Zig,
AArch64, hardware-GPU or physical-input evidence follows from these checks.

Evidence is retained in `integrated-81c8c15144d9-20261104T143759Z`, including raw
logs, immutable source/ref snapshots and `VERIFIED_SUMMARY.json` (SHA-256
`81fa89817ce84c787a8bb698fd82c991b786a4344d87ec93384ed24f22eae4d7`).
The summary retains individual log hashes and separates preliminary launcher
failure from the successful matrix.

Kinds/skolems, structural records, broader annotated interfaces, unknown physical
storage, owned records, broader owner/native combinations and broader optional
provenance remain outside this acceptance. Existing accepted owner/native tests
still pass; owned records remain unsupported. The bounded projection task is
complete, while the larger PLAN gates remain open.
