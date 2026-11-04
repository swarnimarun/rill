# Normally returning discarded results

This bounded increment has independent and full aggregate acceptance on the
K3b baseline. The [integrated record](../verification/2026-11-04-discarded-results.md)
reports 939 default and 987 importer-enabled tests, both strict lint modes and
both Windows compile checks. No full PLAN phase is complete.

## Representation and demand

An immutable binding whose resolved lexical identity has no references can
sequence an unrepresented initializer without storing it. For example:

```rill
type O<a> = None | Some(a)
fn absent() { None }
fn main() { let weak = absent(); 42 }
```

`absent` really executes and returns normally. Its result is
`Discarded(LogicalType)`, distinct from both `Value(Type)` and `NoNormal`.
There is no guessed type, unit replacement, unknown local, or aggregate layout.
Observed and discarded calls have different exact instance keys, retaining the
source definition, every complete logical argument/result, and demand. Source
parameter modes remain part of the canonical definition and call proof.

The contextual path starts only at the existing bare-result diagnostic or the
specific physical-opaque-field failure. All source checking runs first. The
ordinary successful lowering and value restriction remain unchanged. A
syntactic unused-binding proof counts hidden dead reads, loans, assignments,
field uses, and shadowed occurrences by identity, rather than name or liveness.

The initial scope covers enum constructors, direct/recursive forwarding calls,
represented immutable/mutable prefixes, conditionals, and matches on represented
Copy inputs. Constructor operands execute once in written order, recursively
under discard demand. Represented call parameters retain the existing value,
read, computed-read-temporary, root-inout and projected-loan rules. A separately
proved bare nonreturning computation can occur inside discarded construction.
Normal discarded prefixes can precede existing K3/K3b computations.

## Mandatory proof and consumers

The frozen source relation includes demand in every instance identity, checks
all source child roles and lexical bindings, and reconstructs full constructor
schema substitution without a physical ADT. Complete declarations, including
inactive variants, stay retained. Logical-only results and children must have
valid schema arity, pure dependencies, compilation brand and opaque scope.
Original checked-source equality remains separate from retained-witness checks.

An independent positive-flow circuit reconstructs constructors, sequences,
branches, matches and actual call edges. A nullary constructor returns normally;
a discard annotation is never a nonreturn seed. Missing evidence, malformed
logical trees and excessive actual-tree depth are rejected before consumers.
The existing cumulative 100,000-work, 1,024-instance and 128-depth limits apply.
The new demand dimension shares the same instance limit.

The evaluator has a separate normal-discard return outcome, outside runtime
values and arguments. C uses real internal void functions with normal guarded
entry and one leave on normal completion. Calls still evaluate arguments before
the guard. Neither consumer allocates the discarded enum or binding. Ordinary
source main keeps an observed result or its existing proven nonreturn contract.
Optional origin APIs explicitly reject this new control representation after
ordinary lowering and verification succeed.

## Still unsupported

- Observed unknown results, including `main = None` or `main = absent()`.
- Used or mutable layoutless bindings, alias reads and tag-only matching of
  unknown-payload enums; actual unknown argument/local/field storage.
- Direct layoutless record discard. Represented records and field loans retain
  their existing rules, but there is no record-construction erasure rule here.
- Generalized local-shape elaboration mixed with the contextual control path,
  including local-shape projections.
- Newly reached native/owner computations or opaque owner/native schemas.
- Discard-demand calls interrupted by eager arguments when their result is a
  non-bare unknown nominal. Existing accepted K3b calls and the separately
  proved bare nonreturning adapter retain their old contracts.

## Preserved development evidence

The first development passes established the example, normal continuation,
forwarding, root/field/read-temporary loans, enum operand effects, branches,
matches, retained negatives and K3/K3b sequencing through explicit reference,
default and typed interpretation and strict C11 O0/O2. Five focused private
mutation tests passed public verify/evaluate/emit rejection. Fresh exact-source
counts and remaining review findings belong in the final verification record.

The initial harness failures were fixture errors: generic close immediately
followed by `=` lexed as `>=`, and `-Wall -Wextra -Werror` rejected existing
unconditionally emitted unused runtime helpers. No failing case was silently
skipped. The standard-strict C gate is `-std=c11 -pedantic-errors`, matching the
accepted control-result suites. Development logs and generated artifacts are
retained outside the source tree. GCC/Clang, exact frame/argument events, broader
regressions and independent malformed-artifact checks subsequently passed; see the integrated record.

## Follow-on corrections before the frozen author gate

The contextual verifier now recognizes logical metadata that occurs only in a
body discard or a retained interrupted-call suffix, even when every executable
function result is represented. This was a real admission defect in the first
local candidate. The regression migration retains the two original local-shape
negatives and observed-main negatives, while promoting unused `make()`, a dead
conditional initializer and a conservatively returning recursive enum body to
explicit positive discard tests.

A discarded call whose earlier eager operand interrupts may use the existing
verified value-computation adapter when its result is represented. This includes
an `i32`-returning `ignore(spin())` inside a discarded enum operand. Unknown
nominal interrupted results still need a separate rule and remain unsupported.
The mandatory relation checks this adapter against the original source call,
complete interruption evidence and independently false barrier flow.

The author tests also retain exact main-inclusive normal/depth traces, ordered
constructor-argument events, mutable caller state after traps, internal-only
inactive owner/native schema mutation, hidden-use recount, and a consistent
IR/witness edit that still fails the separate original-source comparison.
New discard-match C predicates avoid the older redundant-parentheses style
warning. Earlier warning-bearing runs and strict-lint failures are retained;
only fresh final results are acceptance evidence.

Independent source review then identified a proof-only suffix gap: a nominal
used only in a never-executed interrupted suffix could have an inactive field
changed consistently in both retained declaration copies without reaching any
runtime or discarded-node dependency check. An author red/green regression
confirmed and closed it. Frozen verification now independently rebuilds the
pure-source obligation across all retained source nodes and reached-definition
signatures, including omitted suffixes, from the full declaration dependency
graph and foreign symbol set. The paired owner/native schema mutations are
rejected by all public consumers. This check does not reuse lowerer admission.
