# Interrupted eager value arguments

Status: bounded K3b acceptance is complete on `fe2c26c5`. Independent checks and
the full default/importer/lint/Windows compile matrix passed; see the
[integrated verification](../verification/2026-11-04-interrupted-arguments.md).
No full PLAN phase gate or stable ABI is complete.

```rill
fn forever() { forever() }
fn ignore(x) { 42 }
fn main() { ignore(forever()) }
```

This call checks normally but never supplies a runtime argument or enters
`ignore`. Its argument executes real recursion until the existing call-depth
trap. The implementation preserves that behavior without an unknown value slot,
a guessed unit/integer, a fake return, or a dummy executable `ignore` instance.

## Bounded representation

Ordinary lowering still runs first. The existing bare-unresolved-result failure
permits contextual control lowering. An explicit `InterruptedCall` computation
represents an ordinary Rill call with all-value parameters, a prefix yielding representable Copy values,
and the first argument whose conservative checked-source normal-return fact is
false. The prefix retains its mutations and traps and executes once in written order,
followed by the real barrier
computation at the current caller depth. Later arguments and the outer callee
never execute. Source `main` remains a counted frame; argument evaluation still
precedes entry/guard checks for real calls.

The barrier can have either a concrete existing Copy result annotation or K3's
bare opaque result. Thus `ignore(ignore(forever()))` and `id(forever())` need no
fabricated payload. The outer result must also have an existing supported Copy
layout or a proved bare opaque no-normal result. No unknown physical nominal
result layout is admitted by this extension.

Both IR expression families have an explicit interrupted-call node. The
interpreter returns no normal value on that path and defensively errors if a
barrier unexpectedly returns. C emits prefix/barrier operations without an
interrupted-result temporary. No outer call or new frame is emitted. Concrete
function signatures keep their existing ABI; K3 bare results retain their void
ABI. No unsupported noreturn promise is attached to source functions.

## Source and verification boundaries

Source checking still covers every declaration, suffix, dead branch, loan and
pattern. Runtime instance and layout discovery omit only the never-entered edge
and suffix. The canonical witness still retains the complete syntactic call
graph, including those functions and operands. Unused native declarations and
source dependency configuration remain present.

Each interrupted node separately retains its branded source occurrence, outer
source identity, exact full logical argument/result signature, checked callee
signature, complete call syntax and canonical binding/pattern evidence. These
are compared structurally against the frozen source. The full source signature
is independently instantiated; a never-passed generic parameter is not a
runtime type. Omitted-runtime suffix coverage is granted only after that
comparison and independent source/IR barrier nonreturn reconstruction succeed.
A suffix such as `None` in an unknown payload family may remain source-only;
this does not admit that family into ordinary storage or weaken K2's complete
physical-field proof.

Source and actual IR flow use separate positive Boolean circuits. The new tag
never contributes a false seed. IR reconstructs prefix AND barrier flow,
including a concrete nested barrier. Canonical arena edge ownership and source
call-graph reachability remain checked even for source with no executable
counterpart. Ordinary type/scope/mutability/loan validation still runs.

The frozen-witness relation is not proof of which original program was supplied.
Lowering separately verifies that witness against the actual CheckedProgram,
just as K3 requires. Optional origin APIs reject the new representation only
after ordinary lowering and verification succeed.

All new signature expansion, evidence copying/comparison, arena scans and
lookups use the existing hard control work limits. Retained call evidence uses
a conservative whole-witness reservation per copy, deliberately bounding the
quadratic work possible with nested calls. Source and IR structural depth remain
128 with iterative depth preflight before recursive consumers. Budget failure
is a hard complexity diagnostic, not fallback or proof of nonreturn.

## Deliberate exclusions

New layoutless outer read/inout/native/owner calls, used or mutable layoutless
bindings, genuinely returning discarded unknown payloads and unsupported local
shape elaboration remain outside this increment. Existing representable outer
read/inout calls keep their ordinary K3 path even when another dormant branch
activates contextual lowering. The new interrupted representation is not used
to reject those previously accepted calls. Local-shape elaboration remains
unsupported even in an omitted suffix. Owner/native mixing, generalization,
cleanup and foreign ABI rules are unchanged.

## Author checks

The new public suite checks direct/mutual/nested barriers, bare outer results,
selected/unselected branches, unknown-layout suffixes, unused payload/native
declarations, source error precedence, origin API behavior and retained negative
boundaries. Instrumented C retains real source main and observes exact entries,
attempts, depth, prefix/argument order, zero suffix/body events and argument traps
before the next guard attempt. Separate malformed-IR tests alter the barrier,
prefix order, identity, signature, retained suffix and proof metadata, and check
independent flow plus exact resource boundaries.

See `tests/typed_interrupted_arguments.rs` and the private
`typed::control::interrupted::tests` cases. Final author checks passed 395 library unit tests and 161 selected public
regressions (556 total), default strict all-target Clippy and formatting. The
eight-test public suite passed GCC and Clang C11 O0/O2, including 68 C executions.
The [author verification](../verification/2026-11-04-interrupted-arguments-author.md)
records revisions, controls and retained initial failures. These focused results
are not the full independent acceptance matrix.

The initial unit run found a diagnostic-order regression in the new canonical
arena checks: malformed source depth was rejected structurally before the
existing complexity check. Depth preflight was restored first, and all 389
unit tests then passed. Source review also prevented an accidental rejection
of existing concrete read/inout calls. Test-only compile/style failures were
corrected without exposing private IR or skipping cases.
