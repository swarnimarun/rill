# Reachable typed generic instances: cloud checkpoint

This continues the [Copy-pattern increment](2026-11-03-typed-copy-patterns.md).
It is bounded P1 foundation work, not phase completion or a stable language/ABI.

## Implementation and contract

Generic source functions over supported scalar, static-string, fixed-buffer and
finite Copy-enum values now specialize onto the shared checked typed evaluator
and C backend. A deterministic worklist begins at `main`, visits each reachable
body and deduplicates instances before expanding recursive calls. Each key
contains the resolved source function identity and its full concrete parameter
and result types. Result-only and phantom type variables therefore cannot
collapse distinct instances with identical arguments.

Each body, constructor, local, pattern and call is lowered under its complete
instance substitution. The concrete nominal registry includes the resulting
signatures and body types. Emission orders source functions deterministically,
with same-function instances retaining deterministic discovery order. Fresh
local identities remain distinct across instances, and the existing independent
IR verifier rechecks call signatures, bodies, pattern coverage and ownership.

Source inference still validates generic bodies before specialization and still
checks unused declarations. Unused bodies are not instantiated or emitted.
This is not optimizer-dependent type checking: ill-typed unreachable functions
remain errors. No generic owner support, non-owner borrowed parameters, record
layout or foreign ABI support is inferred from this increment.

The worklist has a 1024 concrete source-function-instance limit, including
reachable monomorphic instances, and a 128-level concrete-type nesting limit.
Both return compiler diagnostics. The complete type is checked after inserting
a substitution, so wrapping an already nested argument cannot evade the depth
limit. These are experimental resource limits, not runtime traps or a guarantee
that arbitrary source input cannot exhaust compiler resources.

Unresolved polymorphic local bindings retain their explicit unsupported typed
lowering result. The checked Copy AST evaluator can still interpret such source
where applicable; native C emission preserves its `E-C-ADT` rejection rather
than guessing an instance. Owner programs retain the source checker's existing
monomorphic and acyclic requirements.

## Evidence

The full default suite passed 317 tests and the feature-enabled suite passed
335, both with zero failures or ignored tests. This includes direct and
generated-binding integration against real SDL3
3.2.10. Formatting, strict all-target Clippy in default and `c-import` modes,
and diff whitespace checks passed. The cloud toolchain is Rust 1.99.1,
GCC 14.2 and Clang/libclang 19.1.7. This worktree's feature suite predates
integration of the parallel importer process-isolation increment; the combined
checkpoint requires its own aggregate run.

The focused default run passed 101 library tests plus five new specialization
integration tests. It covers multiple instances, forward declarations, recursive
and mutually recursive functions (including swapped independent type variables),
return-only and phantom results, generic pattern bodies and mutable locals,
module-qualified nominal collisions, byte/string snapshots, side-effect and
trap ordering, unused declarations, supported call-depth boundaries and explicit
unsupported cases. Generated C is compiled as strict C11 at `-O0` and `-O2`,
and compared with the independent checked AST evaluator and production execution.

A test-only hook into the older uniform layout analyzer compares its exact
reachable source-instance set against the new worklist. Separate tests compare
unresolved-local failures, deterministic output, deduplication at the instance
limit and full inserted type-depth checks. Forged calls to both a wrong-parameter
instance and a same-parameter/wrong-phantom-result instance are rejected before
interpretation or C emission.

Independent review ran 24 additional AST/typed/production/C differentials,
all seven worklist/verifier unit tests and real 1024/1025-instance boundary
cases. The 1024-instance program compiled and ran
at both C optimization levels; the 1025th instance produced the documented
compiler error in typed lowering, production interpretation and C emission.
The independent AST oracle can still evaluate the latter program because it is
not subject to code-specialization limits.

An additional 2 MiB host-thread stress probe found a remaining evaluator limit:
at 32 active source calls, typed execution succeeds with 14 nested Copy lets
per call but overflows the host stack with 15 or 16. The AST oracle already
overflows at 10 in the same probe, so this is not a specialization migration
regression. The preceding eight-let regression still passes. Removing host
recursion for sequential let chains is the next bounded robustness step; these
results do not establish host-stack safety for arbitrary accepted programs.

The existing byte-buffer fallback audit now actually calls its borrowed helper.
An unused generic helper no longer forces fallback, so merely declaring one
would stop testing the intended alternate backend.
The first aggregate run exposed one stale C-text assertion that expected the
uniform `rl_make` representation for a generic identity call. The assertion now
checks scalar literal order and explicitly compares typed C and AST execution;
the complete feature suite passed after this test-only adjustment.

## Open work

The current path still does not cover records, non-owner `read`/`inout` loans,
foreign values, vectors or general polymorphic local specialization. Stable
resolved source IDs, lossless source tooling, the complete semantic MIR,
initialization/drop flags, partial moves, escaping/CFG loans, owned records,
effects and allocator capabilities remain separate requirements of PLAN.md.
