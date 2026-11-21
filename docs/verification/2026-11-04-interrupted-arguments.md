# Interrupted eager arguments: integrated acceptance

The exact tested source is `fe2c26c5eb594f8ca9a711cffbdacc35ead45542`, tree
`1849e04801c9b81e0a2dfddbaa4535dbe1527f4b`. Production/private focused changes are
`6a695b1`, author public tests are `5580441`, and the frozen author checkpoint is
`c1c1c95`. The independent `fe2c26c` increment changes only tests, test-module
declarations and its verification record. This acceptance record and status
updates are documentation-only changes after the tested source.

## Full matrix

All seven stages passed on 4 November 2026, from 02:30:23 to 02:34:37 UTC:

- Repository formatting.
- Default tests: **892 passed**, zero failed or ignored.
- Default strict all-target Clippy, with warnings denied.
- Importer-enabled tests: **940 passed**, zero failed or ignored.
- Importer-enabled strict all-target Clippy, with warnings denied.
- Windows GNU all-target compile check, default and importer-enabled.

There were no dependency-skip notices in either complete test run. The existing
actual SDL3 integration ran with SDL3 3.2.10; this checkpoint did not produce a
new visual capture set. The previously recorded software-rendered captures do
not establish physical input or hardware rendering. Windows checks compile
only; they are not Windows runtime evidence. No Zig or AArch64 runtime claim is
added by this matrix.

The checkout was clean at the exact source before and after all stages. A
byte-preserving timestamp refresh forced compilation from that checkout rather
than reusing another worktree's package artifact. Logs identify the actual
compile path. The environment used Rust/Cargo 1.99.1, GCC 14.2.0, Clang/libclang
19.1.7, two build jobs, no incremental compilation and zero dev/test debug
information. The established storage and memory guards remained enabled.

## Independent evidence

The [author record](2026-11-04-interrupted-arguments-author.md) documents 556
affected tests and 68 GCC/Clang C executions. The separate
[independent record](2026-11-04-interrupted-arguments-independent.md) documents
407 unit tests including 12 independent private/relation probes, five public
tests under each compiler, and **76 C11 O0/O2 executions**.

The independent matrix covers fourteen positive sources, a native C-only
control, four exact main-inclusive event traces, checked-source negatives,
representation/mode exclusions and contextual-native rejection. Prefix effects
occur once; later arguments and the outer callee do not execute. Real recursion
and boundary arguments retain their existing depth-guard order. Mutations of
barriers, signatures, identities, suffix evidence and source relations reject.
The retained tests cover quantifier integrity, exact work bounds and 128/129
structural-depth boundaries.

Two independent Clang fixtures emit the known `-Wparentheses-equality` style
warning. Exact diagnostics remain retained and only that category is permitted;
the matrix is not warning-free. Attribution to the existing emitter is based on
unchanged source, not a separately executed baseline comparison. C11 and
pedantic errors remain enabled. Initial harness failures and the pre-freeze
read/inout compatibility correction are recorded without dropping test cases.

## Accepted scope and next work

The [bounded contract](../blockers/interrupted-arguments.md) admits ordinary
all-value calls stopped by a separately proved nonreturning argument. It retains
representable prefix effects, actual barrier computation, full checked source
syntax/signatures and independent source/IR correspondence. It creates no
unknown value/storage, fabricated return or never-entered executable callee.

Normally returning discarded unknown results need a separate demand-aware
representation. Used/mutable layoutless bindings, genuinely unknown physical
payloads, unsupported local shapes and new owner/native combinations remain
outside this increment. Existing fallback is not retired. No full P1, P2, P5 or
other PLAN phase, stable ABI, deployment or release is claimed.
