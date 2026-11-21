# Deterministic recursive-declaration diagnostics

This narrow correction keeps the existing nominal dependency graph and
`E-TYPE: recursive type '<name>' requires indirection` rejection. When several
declarations participate in cycles, it reports the first cyclic declaration in
program declaration order, not randomized hash iteration order. For a flattened
module program, that is the existing resolved declaration order. A declaration
that merely depends on a cycle is not itself the witness.

## Source evidence

The previous checker iterated a `HashSet` both while computing dependency closure
and while choosing a cycle witness. Independent analyses of the ordinary Copy
fixture below could report either `A` or `B`:

```rill
type A<a> = A(B<a>)
type B<a> = B(A<a>)
fn main() { 42 }
```

The original failure was observed during phantom-product testing at
`14f12c6712b816d98d8e43bb2d22f10dad4bfc5c`: the checked analysis and interpreter
reanalysis returned those different valid witnesses. The author retained that
failure and did not classify it as a product-layout proof defect. This correction
is based on accepted source `706a29a5736ad621ec9e31ab241549404af08a92`, independently
of that product implementation.

The existing dependency check is isolated in a private helper at the same point
in declaration validation. Its traversal and witness selection now use the
program's declaration vector. Hash tables still serve membership and union;
their iteration order no longer determines the witness. Dependency extraction,
including type-parameter shadowing and generic argument traversal, is unchanged.
Earlier declaration validation and later function validation keep their order.

## Regression coverage and execution status

The private unit regression constructs one exact name set, reads its iteration
order, and deliberately places mutually recursive declarations in reverse order.
The original witness loop would disagree with the expected first declaration for
every hash seed. This is a deterministic adversarial case, not a probabilistic
retry test.

`tests/type_cycle_diagnostics.rs` additionally checks all six declaration orders
of a three-member cycle, generic and record cycles, separate cycles, acyclic
dependents, existing declaration/function diagnostic precedence, and finite
declarations. Repeated public analysis, checking, interpretation, reference
interpretation, C emission, reference emission, and source compilation must
return the exact same diagnostic. The tests do not invoke a C compiler.

Integrated candidate `04b88ab8007c035a337a3ea4d531eeee3098b3ed` compiled and
passed the six public tests and the private regression as part of 298 library
tests. Formatting and strict default all-target Clippy also passed. Independent
execution subsequently required the exact first cyclic name across all seven
entry points. The complete combined matrix passed 762 default and 810 importer
tests, formatting, both strict Clippy and both Windows compile modes; see the
[integrated evidence](2026-11-03-phantom-products.md). No ownership behavior
or full phase gate is claimed by this change.
