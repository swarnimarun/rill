# Interrupted arguments: independent focused verification

The frozen reviewed candidate is `c1c1c9515b506d1e64c4b34e899e0a865173b2b6`,
tree `9412ebf93f860689acfbc4e9893392bac8be3ec1`, based on `29b6c65`.
Production changes are `6a695b1`; author public tests are `5580441`.
This independent increment adds only tests, test-module declarations and this
record. Full default/importer/platform aggregate acceptance remains separate.
No PLAN phase gate is declared complete.

## Executed checks

On 4 November 2026 the final independent checks passed:

- 407 library unit tests, including 12 independent private/relation regressions.
- Five independent public tests with GCC and again with Clang.
- 76 C11 executions across GCC/Clang and O0/O2, using `-pedantic-errors`.
- Repository formatting and default strict all-target Clippy with warnings denied.
- Git diff checks; production definitions were not edited by the reviewer.

The public matrix contains fourteen ordinary positive sources, one native C-only
control and four instrumented event cases. It also retains five checked-source
negatives, six unsupported representation/mode cases and one contextual-native
rejection. Ordinary pure programs agree with the explicit reference interpreter;
the closed-owner control correctly rejects that reference API while retaining
typed/default/C parity. The native control preserves the exact reached raw-value
interpreter error and runs through the native C path.

C instrumentation retains generated source main. Direct and nested barriers
show 32 admitted entries, 33 entry attempts, 31 barrier-body operations, 31
recursive-argument operations, one earlier prefix, zero suffix/body/leave events.
A trapping recursive boundary argument prevents the 33rd attempt. A trapping
outer prefix prevents entry to the barrier entirely. A private evaluation of the
exact main remainder retains the original mutable slot at 3101: earlier +1,
then 31 actual barrier frames adding 100, with the +10000 suffix never executed.

Private probes remove proof metadata even when only a dormant concrete tag
remains; alter prefix values, scope, barrier flow, foreign compilation evidence,
source occurrence, target, full logical signature and omitted suffix evidence;
and distinguish frozen-source correspondence from the original CheckedProgram.
They retain unused native metadata, exact work-budget checks and source/IR
128/129-depth boundaries. A target-signature test keeps both evidence copies
aligned while forging duplicate or out-of-scope quantifiers, so rejection must
come from canonical target validation rather than a trivial evidence mismatch.

## Findings and retained corrections

Source review found that initial interruption selection rejected already-admitted
concrete read/inout calls whenever a different dormant K3 computation activated
contextual lowering. The author corrected selection to retain the ordinary call
path; independent read and inout controls now pass. Review also requested the
existing bounded duplicate/membership/log-work quantifier checks for skipped
callee definitions, which lack executable instances. The author incorporated
those checks before freezing the candidate.

No additional production defect was found during independent execution.
The first invocation had no Cargo on PATH; sourcing the already-installed
recovery toolchain fixed it without installation. Initial harness failures
incorrectly demanded owner-reference parity, rejected a known Clang style
warning, and allocated a needless Box in a malformed-artifact test. All initial
logs remain retained; corrected tests reran without dropping their cases.

Clang reports `-Wparentheses-equality` for redundant parentheses in the dormant
boolean-match and K1 enum-match control fixtures at both optimization levels.
Their exact compiler stderr is retained per fixture. The Copy-pattern emitter
is source-identical to the accepted baseline; no separate baseline execution
was performed. The final matrix permits only that exact warning category and
is **not warning-free**. C11/pedantic diagnostics were not suppressed.

## Source and environment identity

A byte-preserving timestamp refresh of the crate roots forced a new production
artifact. Cargo JSON names the exact candidate checkout and records `fresh:false`.
Its rlib SHA256 is
`20f2a7f5f06434dc54cb2f26461323861fd21e4d40c64846f2da7b933509a167`.
Separate fresh JSON and hashes identify the independent unit/public artifacts.
Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7 were used, with two build jobs,
no incremental compilation and zero dev/test debug information. Existing sole
RAM build storage was reused; no extra checkout/target, install, cleanup or
remote action was performed. The persistent-space guard stayed above 2 GiB.

The retained tests are `tests/independent_interrupted_arguments.rs`,
`src/typed/independent_interrupted_tests.rs` and
`src/typed/independent_interrupted_relation_tests.rs`.
