# Bounded all-nullary phantom values

This increment targets an existing source-language representation gap. An enum
such as `type Flag<a> = Off | On` has a finite tag-only representation even when
inference leaves `a` unresolved. Ordinary typed lowering can retain that argument
as logical identity without choosing an arbitrary concrete type for it.

This is a bounded representation extension, not new syntax, broader
generalization, a complete polymorphic runtime, or completion of P1. The
acceptance gates in [PLAN.md section 19](../PLAN.md#19-dependency-ordered-delivery-plan)
remain open.

## Evidence status

Focused author verification passed on semantic source
`24c1a519f9c2b13e34e4c25abc0ef67fb7ade387`: 175 typed-library tests in each
configuration, 148 default and 160 importer-enabled focused integration tests,
both strict Clippy configurations, both Windows GNU compile configurations and
formatting. The new integration target passed all 19 groups in each mode,
including strict C11 `-O0`/`-O2` differential execution.

See the [exact commands, coverage and limits](verification/2026-11-03-nullary-phantoms.md).
Independent acceptance subsequently passed 18 probes, 40 GCC/Clang O0/O2
executions and the combined configured-native seam. The complete final matrix
passed 715 default and 763 importer-enabled tests, both strict Clippy modes,
both Windows compile modes and formatting. Exact source and limits are in the
report; this is not completion of the full P1 gate.

## K1 admission boundary

In K1, an unresolved runtime value type is admitted only when its outer nominal
declaration is an enum whose **complete variant set is nullary**. The same
rule applies to entry results, function parameters/results, evaluated
initializers, local reads, assignments, and match scrutinees. The constructor
currently selected by execution is not enough evidence.

The admitted logical argument list retains its entire structure. A nested
nominal application is not collapsed into one anonymous unknown. Known
primitive types, nominal definition identities and arities, raw-pointer pointee
spelling and constness, owner dependencies, and unresolved binder identities
remain meaningful even though no argument bytes are stored.

The first slice intentionally kept **open runtime roots** narrower than that
logical argument grammar. Payload-bearing products containing symbolic nullary
values were unsupported in K1. For example, the following was an ordinary
typed rejection control rather than an implicit product extension:

```rill
type Flag<a> = Off | On
type Link<a> = Link(a,Flag<a>)
fn tag(x) { match Link(x,Off) { Link(_,f) => f } }
fn main() { tag(Off) }
```

The `Link<Flag<a>>` value is a physical payload-bearing root. The accepted
[K2 increment](typed-phantom-products.md) moves this exact fixture into its
positive suite using complete field-schema reconstruction. K2 also
includes fixed-field Mark/Stamp products and `Option<Flag<opaque>>`, while
`Option<opaque>` remains unsupported. K2 has separate focused, independent and full aggregate acceptance
evidence. Neither slice permits losing full nested
identity merely because an argument is carried only logically.

Already accepted closed finite aggregates and phantom-owner cases retain their
existing behavior. No new arbitrary witness such as `unit`, `i32`, or a fabricated
pointer type is selected to force an unresolved program into that closed path.

## Executed examples

An unresolved observation and returned tag need no runtime argument layout:

```rill
type Flag<a> = Off | On
fn main() {
    let f = Off;
    match f { Off => 42 | On => 0 }
}
```

Observed result: `42` through ordinary typed evaluation and typed C output.
Adding an unrelated `type Unused = Extra(i32)` declaration must not change that
acceptance. The regression specifically prevents an unused declaration from
selecting the old uniform-C layout gate.

```rill
type Token<a> = Token
fn main() { Token }
```

Observed result: `Token`. Its logical argument remains unresolved; its printed
value is not replaced with a primitive or an invented payload.

Dynamic tags and mutable slots remain real runtime values:

```rill
type Flag<a> = Off | On
fn choose(inout count) {
    count <- count + 1;
    if count == 1 { Off } else { On }
}
fn main() {
    let mut count = 0;
    let mut f = choose(inout count);
    f <- choose(inout count);
    match f { Off => 0 | On => count + 40 }
}
```

Observed result: `42`, with two calls in order and a single monomorphic mutable
slot. This is not static-shape substitution or permission to generalize the
call result. An unused call result still evaluates its call, including mutation
and traps.

## Logical identity and independent checking

The source checker remains authoritative for inference, value restriction,
nominal equality, exhaustiveness, ownership, and loan legality. Frozen unresolved
arguments describe checked equivalence classes after inference; they are not
new unification variables and cannot be used as standalone expression types.

The representation contract requires:

- Every opaque identity is branded to its compilation; handles from another
  compilation are rejected, while cloning a typed artifact preserves its brand.
  Entry and source-function instances have distinct binder scopes. Repeated
  references to one checked class in a scope reuse one logical identity.
- Callee substitution forwards caller identities. It must not create a fresh
  unknown every time a parameter or result is forwarded.
- Function-instance keys include the resolved source definition and complete
  logical parameter/result types. Equal C layouts do not merge `Flag<i32>` with
  `Flag<bool>`, separate weak slots, or locals belonging to different source
  instances.
- Recursive calls with the same logical signatures deduplicate before body
  expansion. Existing instance, nesting and work limits remain enforced;
  alpha-renaming must not turn a finite recursive graph into an endless sequence
  of new instances.
- Canonical resolved declaration metadata establishes nominal identity, arity,
  constructor identities/order, and every field schema. Symbolic admission
  checks all variants, not just those constructed or matched on a reached path.
- Independent typed verification rechecks the tag-only layout against that
  schema, argument well-formedness and scopes, and exact logical instance
  equality in calls, returns, assignments, patterns and root loans.
- An opaque argument cannot occur as a physical field, primitive operand,
  foreign ABI operand, independently materialized layout, or bare runtime value.
  Both evaluation and C emission must reject malformed typed artifacts before
  consuming them.

These checks establish the representation claim relative to retained canonical
source schema. They are not a theorem that every source-to-IR transformation
preserves meaning. Changes that consistently rewrite a schema and every use
can describe another well-typed program; the relationship to the original
checked source remains a separate validation obligation.

## Representation work bounds

Symbolic discovery retains at most 1,024 function instances and the existing
128-level type nesting bound. Frozen equivalence classes and symbolic instance
keys share a 100,000-unit work budget. Symbolic substitutions, canonical schema
validation, the accumulated symbolic registry, and independent logical
verification each have a 100,000-unit bound, charged before new type trees, complete constructor tables or
retained edges are allocated. Independent verification charges each retained
variant/field/argument edge, even when many instances share one source schema. Duplicate call requests retain the same identity
and still consume traversal work. Logical complexity has an explicit
`E-TYPED-LOGICAL-COMPLEXITY` diagnostic; it does not trigger a guessed layout.
Closed substitutions keep their prior limits and conversion behavior. These
abstract units do not measure string bytes or allocator overhead and are not a
whole-compiler memory guarantee.

Private capture products may store already proved symbolic enum values. Their
field types and identity arguments must remain exactly equal under the existing
capture verifier; binder visibility is propagated through those fields. This
allows a generalized local record shape to return `Flag<Flag<a>>` without ever
materializing its open source record layout. The regression suite includes that
source construction, separately from the originally rejected physical
`Link<Flag<a>>` case now assigned to K2 above.

## Source rules and native/owner boundaries stay intact

Immutable nonexpansive constructors can still generalize when the existing
source rules permit it. Calls, conditional/match results, mutable slots, aliases
of weak values, and values depending on borrowed storage do not acquire new
polymorphism because their representation happens to be small.

Shared `read` and exclusive `inout` forwarding use the original root slot.
Value arguments retain snapshot semantics; computed read arguments evaluate
once at their source position. Later conflicting arguments remain rejected.
Tag-only storage neither removes those intervals nor justifies `restrict` or
new foreign lifetime guarantees.

Raw-pointer identity and ownership dependencies remain visible through all
logical nominal arguments. Native-origin exclusion and owner/native-mixing
checks must not inspect only physical fields. Existing accepted closed
`Phantom<Owned>` transport stays accepted without implying that generalized
owner-dependent locals or generic owner functions are now legal. Existing
allocation-trace regressions in [`tests/owner_adt.rs`](../tests/owner_adt.rs)
remain the separate no-allocation check for those closed owner controls.

No native declaration, unused header assertion, native configuration, or
reached-only evaluator rejection is removed. This work does not extend C ABI,
resource ownership, FFI effects, stored references, or owner recursion.

## Optional origin APIs remain closed

The [function-instance API](typed-function-origins.md) and
[Copy-body API](typed-copy-node-origins.md) continue exporting closed types.
Ordinary source checking, lowering and independent verification run first.
If the resulting representation contains any opaque logical argument, the
optional API then returns an explicit unsupported-mapping diagnostic:

- `E-TYPED-UNSUPPORTED: function origin index does not map opaque logical arguments`
- `E-TYPED-UNSUPPORTED: Copy node origin index does not map opaque logical arguments`

No partial successful index is published, no concrete type is invented, and
no instance/node is silently omitted. This includes opaque metadata confined
to a body when the function's exported result is a concrete integer. Ordinary
semantic errors retain precedence. Existing closed pure-Rill mapping results
remain supported, while native types hidden inside nested phantom arguments
remain outside these APIs' native boundary.

## Explicit exclusions and regression inventory

The suite distinguishes ordinary typed rejection from source rejection and
from the independent reference/uniform backend's capabilities. In particular:

- `None | Some(a)` is not all-nullary, even when execution selects `None`.
  K2 admits `Option<Flag<opaque>>` by proving both variants, not by erasing
  Some's payload; genuinely unknown `Option<opaque>` remains rejected.
- `Mark<a> = Mark(i32)`, `Stamp<a> = { number: i32 }` and finite physical
  wrappers around symbolic nullary values move to the accepted
  [K2 acceptance assertions](../tests/typed_phantom_products.rs). They were
  intentionally excluded from this first slice; K1 evidence does not certify K2.
- A bare unresolved result of `fn forever() { forever() }` is not a phantom
  value. Dead branches containing that result still receive ordinary static
  representation checking. No termination analysis or discarded-result rule
  is added.
- Incompatible uses of weak/mutable values, nonexhaustive matches, aggregate
  equality, nominal mismatches and conflicting root loans remain source errors.

Positive fixtures cover direct/returned tags, constructor aliases and
shadowing, the unused-declaration metamorphic case, dynamic tags, loops,
once-only calls and matches, argument order, root-loan forwarding, snapshots,
repeated calls, distinct generic source instances, module identity and existing
closed nested/owner behavior. Negative fixtures preserve the boundaries above
and exact nested nominal/raw-pointer type distinctions. Origin fixtures cover
ordinary-success/optional-rejection, closed controls and semantic-error
precedence.

The differential helper requires ordinary typed lowering and independent
verification, compares typed/default interpretation with the explicit reference
oracle for pure Copy programs, and compares ordinary C output byte-for-byte
with typed C. It rejects `rl_value` and `RL_CAP` in default output and checks
deterministic repeated lowering. Its native gate compiles strict C11 at both
`-O0` and `-O2`, checking values or the exact runtime-trap outcome.

Recursive forwarding is tested at the existing 32-active-call boundary,
including `main`. Separate pure-divergence fixtures only lower, verify and emit
C; they deliberately do not execute. Internal malformed-artifact tests in
[`src/typed/logical_tests.rs`](../src/typed/logical_tests.rs) cover schema, binder,
instance, physical-field and loan corruption;
source-only integration tests cannot substitute for those verifier tests.

The focused, independent and full aggregate commands and exact sources are
recorded in the verification report. Later changes must repeat the affected
and full checks required by [AGENTS.md](../AGENTS.md), retaining exact revisions,
commands, pass/fail counts and skips.
