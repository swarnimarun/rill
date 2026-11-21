# Concrete captures for immutable local value schemes

Status: provisional, 3 November 2026. This is a bounded extension of the
shared typed execution path, not completion of C03, P1, source mapping or MIR.
The [conservative value restriction](0003-local-value-restriction.md) is unchanged.

## Representation

An eligible generalized initializer has only immutable value references,
literals, validated enum constructors and record construction. Its constructor
tags and record structure are statically known. The only runtime information in
that structure is its concrete leaves. Lowering represents the value by:

1. A compiler-only constructor/record shape, with resolved nominal names and
   field indices. Immutable aliases share this shape as a bounded DAG.
2. One immutable, compiler-generated capture record containing the concrete
   leaves, evaluated exactly once in their original written order at the source
   binding. The record is retained even if the value is unused or its uses are
   unreachable. A shape without concrete leaves needs no runtime storage.

This is the representation of the **one evaluated initializer**, not a deferred
copy of its AST. Calls, control flow, field reads, mutable/read-parameter roots,
allocation and trapping expressions cannot enter the shape grammar. A
previously evaluated immutable snapshot can be captured, but the original
computation or mutable storage is never reread at a later use. Shapes contain no
owner payloads, including owner dependencies hidden in inactive variants or
phantom type arguments.

Capture records have explicit `AdtOrigin::LocalCapture` metadata and a reserved
compiler identity that source declarations cannot use. Their identity arguments
are exactly their concrete field types. Equal capture products may share a
layout; their storage `LocalId`s remain distinct, including across shadowing and
function instances. They use ordinary independently verified record operations
and dependency-ordered layouts, count against the existing layout/field/frame
limits, and introduce no runtime heap allocation. Parsed and module origin maps
are not rewritten; this is not a replacement typed/backend source-map system.

A single flat capture record is important. Synthesizing a nested `let` for every
leaf increased verifier recursion with constructor width and overflowed a test
thread at 256 leaves. The flat representation adds no binding depth beyond the
original source `let`; both 256-leaf acceptance and 257-leaf rejection are tested.

## Concrete use sites

Every whole-value use must have an exact closed inferred type. Lowering matches
the stored nominal shape against that type and reconstructs its concrete
constructor/record nodes from immutable capture fields. Different uses can have
different nominal type arguments. Existing concrete IR verification independently
checks every constructor payload, nominal identity, field path, scope, Copy
operation and call signature. That verifier does **not** prove that reconstruction
preserved the source program: separate origin, snapshot/order, alias-sharing,
exact-use-type and generated-case differential tests cover that lowering seam.

A field use selects its checked path from the shape before materializing the
selected value. Unrelated unresolved phantom arguments need no representation.
For example, a record with an unused phantom parameter and a concrete integer
field can expose that field without assigning an arbitrary type to the phantom.
A projection into a concrete captured record uses an immutable Copy temporary
and the original source path. The internal capture field does not consume one
of the source's sixteen permitted field steps.

A generalized immutable `read` argument becomes a concrete read temporary.
There is no single monomorphic source root address for its multiple instances.
This is valid for this subset because the source value and captures are immutable,
source loan checking still checks the original binding identity and full
argument-order interval, and read addresses cannot escape or be compared through
foreign/first-class-reference operations on this typed path. Later-argument
conflicts and prohibited generalization under active loans remain source errors.
This does not extend borrowing or authorize writes through a generalized root.

## No phantom defaults

An unused `None`, a pure alias of it, or an unused record/enum shape may retain
quantified variables indefinitely: its known shape and concrete captures are a
complete representation. No unit, integer, boolean or arbitrary nominal type is
substituted for an unresolved phantom parameter.

A genuinely unresolved **use** is still rejected. Examples include matching a
`None` at an otherwise unconstrained `Option<a>` type, returning such a value from
`main`, or storing an unconstrained field projection or call result in a weak
monomorphic local. Those expressions are not silently generalized or erased.
The independent checked AST interpreter may still support such Copy programs;
that fallback does not supply a concrete typed/C layout.

## Bounds and remaining scope

Per reachable concrete function instance, local elaboration permits:

- At most 100,000 work units. Initializer visits, projection steps, expanded
  concrete nodes and Copy-dependency visits are charged before traversing or
  allocating their next work. Aliases share shapes without expanding them.
  Successful Copy-dependency checks are cached for the immutable type registry.
- At most 256 concrete captured leaves, including unused bindings.
- Shape height and recursive shape traversal of at most 128. Height is recorded
  at construction without expanding shared subgraphs.

Exhaustion reports `E-TYPED-LOCAL-COMPLEXITY`, not an unchecked-backend fallback.
The existing 1,024 function-instance/layout limits, concrete type-depth checks,
record/path validation and conservative C frame limits also apply. These are
resource policies, not a general host-stack safety proof.

Top-level generic specialization and the source value restriction are unchanged.
Weak variables, effectful initializers and mutable/read roots are not newly
polymorphic. Owner-bearing records, generic owner programs, foreign forms outside the existing typed subset, closures, full effect inference, complete MIR
coverage and typed/backend source mapping remain separate work. See the
[verification record](../verification/2026-11-03-typed-local-values.md).
