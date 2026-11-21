# Closed parameter constraints in recursive groups

The checker installs every existing closed parameter annotation in a recursive
strongly connected component before checking any body in that component. It
keeps the established order: monomorphic placeholders, all closed result
constraints, all closed parameter constraints, body inference, then ordinary
rank-one generalization. Components still traverse functions in source order.
The annotations use the existing `declared_type` vocabulary with an empty
variable environment. They are equality constraints, not polymorphic recursive
schemes, casts, default types, new syntax or a new layout mechanism.

This intentionally changes parameter timing. Previously each function applied
its own parameter annotations immediately before its own body. For example:

```rill
type P = { x:i32 }
fn f(p) -> i32 { g(p); p.x }
fn g(p:P) -> i32 { if false { f(p) } else { 0 } }
fn main() { f(record P { x=42 }) }
```

Previously this order rejected with E-RECORD while placing `g` first accepted.
Both orders now accept and evaluate to `42`. The
[runnable example](../examples/recursive-signatures) uses the same constraint
with shared access and a computed read temporary. Legacy `let` parameter
annotations participate without adding legacy result-annotation syntax.

## Bounds and intentional compatibility changes

Every parameter and result annotation is admitted through the existing borrowed
tree budget before recursive declaration resolution or owner classification.
Unused declarations and legacy functions participate. Module loading shares one
source-spelling allowance across the complete graph, then shares a separate full
allowance for exact resolved spellings before cloning or rewriting functions.
Ordinary, snapshot and source-mapped loaders use that same path.

Each pass permits depth 128, logical weight 16 MiB per tree, 64 MiB across its
whole program/graph and 4,000,000 node visits. Each node costs 64 units plus its
UTF-8 name bytes. This is not a whole-compiler resource guarantee. The diagnostic now reads
`E-TYPE-COMPLEXITY: signature annotation input budget exceeded` for both parameter
and result inputs. This intentionally replaces the former result-only wording
while preserving the diagnostic code and budget semantics. Exact-text consumers
need to migrate; the retained result tests explicitly adopt the generic wording.

This deliberately bounds previously unbounded public parameter AST inputs and
shares their cost with results. Existing programs with oversized parameters or
combined annotations can newly reject. Earlier parameter resolution and
constraints can also change which error is reported first in multiply-invalid
programs. Rejection is preserved where tested; universal acceptance equivalence
or identical diagnostic wording/order is not claimed.

Raw `ptr`/`const_ptr` subtrees remain outside Rill name rewriting, preserving
exact C pointee spelling and constness. Their complete original trees, including
malformed children, are still charged. Existing conservative owner-name
classification, including `ptr<SystemBytes>`, is unchanged. Raw pointers acquire
no checked lifetime or automatic resource cleanup.

## Preserved boundaries

Parameter modes, value snapshots, computed read temporary lifetime, inout
original-slot mutation, left-to-right effects, loan checking, the value
restriction and post-component generalization remain unchanged. Nominal identity,
module privacy and unused-declaration validation still apply. The existing
Parameter/Annotation/TypeArgument source paths remain distinct from FunctionResult;
no source role or backend representation was added.

At this signature-only checkpoint, an unannotated callee-result projection or
still-unconstrained intermediate parameter could fail before later body inference
supplied its nominal type. The subsequent [deferred-projection increment](deferred-projections.md)
intentionally migrates compatibility case 5 and the intermediate-parameter example.
The remaining canonical fingerprints retain actual quantified-variable membership.
Neither increment establishes universal inference-order independence.

Kinds, skolems, higher-rank interfaces, polymorphic recursion, owner records,
effects, unknown physical storage and native ABI expansion remain unsupported.
The [author record](verification/2026-11-04-recursive-signatures-author.md)
distinguishes its focused checks from subsequent gates. The
[independent review](verification/2026-11-04-recursive-signatures-independent.md)
passes 715 selected tests and 112 separately counted GCC/Clang C executions.
[Integrated acceptance](verification/2026-11-04-recursive-signatures.md) on source
`31cac927` passes 1,065 default and 1,113 importer-enabled tests, formatting,
both strict lint modes and both Windows GNU compile-only modes. These aggregate
counts are distinct from the earlier selected suites and C-execution matrices.
This increment does not establish P1, C02 or full PLAN section 19 completion.
