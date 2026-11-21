# Bare nonreturning results: bounded accepted representation

Status: bounded K3 acceptance is complete on `bdd878fe`. Independent execution
and the full default/importer/lint/Windows compile matrix passed; see the
[integrated verification](../verification/2026-11-04-nonreturning-results.md).
No full PLAN phase gate or stable ABI is claimed.

A checked recursive computation can have an unresolved result class without
any finite normal return:

```rill
fn forever() { forever() }
fn main() { let unused = forever(); 42 }
```

The recursive call still executes, including arguments and the existing
active-call depth guard. The concrete main result remains `Value(i32)`. Its
unused initializer has no storage, and its concrete continuation remains in
verified IR even though execution cannot reach it.

## Representation and admission

Runtime `Type` and value-only `ExprNode` are unchanged. Function results use
`Value(Type)` or `NoNormal(OpaqueId)`; bodies use a separate `Computation` sum.
A no-normal node carries an exact branded logical result and canonical source
occurrence. No-storage bindings carry source identity, never a fake local ID.
No unknown/never runtime value, unit default, omitted call or invented result
is introduced. Complete instance keys retain all logical arguments and the
exact result identity even when two results share C's void ABI.

Ordinary discovery runs first. Only its specific bare-unresolved-layout error
permits the contextual retry. All parameters and actual arguments still need
admitted Copy layouts, including accepted finite products with opaque logical
arguments. A bare result is admitted only when conservative checked-source
normal-return analysis is false. Prefixes, every branch and match arm, and
unused immutable divergent-let continuations remain present and checked.

The initial grammar admits direct/tail recursion, mutual recursion, concrete
prefixes and branch/match inputs, unused immutable divergent lets, and a bare
main result. It deliberately rejects eager layoutless operands or arguments,
used or mutable layoutless bindings, unknown payload layouts, local shape
elaboration without a source relation, and new owner/native combinations.
`ignore(forever())` is addressed separately by the subsequent
[interrupted-argument increment](interrupted-arguments.md); it was not part of
this K3 acceptance. `let x = forever(); x` remains unsupported.
A literal-false concrete base return retains the ordinary value contract.

## Independent proof boundaries

Checked source and actual exact-instance IR build separate positive Boolean
circuits. Neither uses claimed NoNormal annotations, traps or literal branch
conditions as false seeds. A bounded worklist recomputes a least fixed point;
false excludes a finite normal-return derivation, while true does not promise
termination. Each analysis is limited to 100,000 work units and 1,024 reached
source definitions or instances. Exhaustion is an error, never a false fact.

A canonical witness retains reached bodies/signatures/quantifiers and every
syntactic child, literal, operator, call, mode, argument order, place, pattern,
lexical binding and use. It also retains all type/native declarations, native
configuration, allocation metadata and source function names/order. Unreached
Rill bodies and signatures are not a byte-for-byte whole-program snapshot.

Instance substitutions are reconstructed from canonical quantifiers and exact
call signatures, including weak classes and branded scope/ordinal identity;
stored substitution maps are compared only afterward. Structural correspondence
checks every retained source occurrence, including collapsed places and buffer
literals, dead continuations and skipped branches. Canonical arena ownership
and coverage reject orphan nodes, patterns and bindings.

The standalone verifier first runs bounded actual-tree and witness preflight,
then verifies full correspondence to the frozen witness and independently
recomputes IR return facts. Missing proof metadata rejects every extension tag.
Ordinary type, scope, local identity, mutability, loan, pattern exhaustiveness,
native and logical checks still run. Original-source correspondence is a
separate required lowering check against the actual `CheckedProgram`; a frozen
witness alone cannot establish which original program was supplied.

Structural source-witness and actual-IR depth is limited to 128, root depth zero,
including retained unreachable trees. Text parsing independently caps syntax
nesting at 64. Extension-only preflight conservatively reserves initialization,
local snapshots, loan paths and joins before recursive consumers. Pattern
analysis shares a cumulative budget and precharges matrix copies/expansions.
Ordinary closed-program budgets are unchanged.

## Consumers and focused evidence

The evaluator uses an uninhabited normal result for NoNormal computations and
performs real argument preparation and guarded recursive calls. C uses internal
void signatures only for bare results, emits defensive trap fences after calls,
and emits no bare-result temporary or printer. It makes no unsupported noreturn
assumption about source functions. Concrete no-storage-let functions preserve
their existing value ABI. Optional function/node origin APIs reject this new
metadata only after ordinary lowering and verification succeed.

Focused implementation checks passed on 2026-11-04:

- 266 typed unit tests, including separate control/source relation mutation checks,
  malformed arena metadata, depth/snapshot/matrix bounds and public-consumer
  rejection of altered values/modes/returns/brands and missing proof metadata.
- Default all-target Clippy with warnings denied and repository formatting.
- Ten source acceptance tests with both GCC and Clang at O0/O2, comparing typed,
  ordinary and reference interpretation with emitted C.
- Exact C instrumentation preserves the real source main: 32 admitted entries,
  31 recursive prefixes and boundary arguments, the 33rd entry attempt after
  argument evaluation, zero normal leaves and zero unreachable-tail events.
  The paired trapping boundary argument prevents that 33rd entry attempt.

Real fixture failures were retained and corrected: a missing metadata field,
an invalid vector test enum, an over-deep text fixture rejected by the existing
parser, an incorrect assumption that source loading bypassed checks, private
IR access from an integration test, and retained temporary-name collisions
across isolated process namespaces. These were test harness issues, not silently
skipped acceptance cases. The integrated verification records independent
execution, the migrated K1/K2 expectations and all final aggregate results.
