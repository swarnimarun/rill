# Recursive closed signatures: author verification

4 November 2026. **Focused author checks pass.** Tested executable source is
`a895550642aae999287f7bd6f241c3a99e3e4dc5`, tree
`4ba403586275fc1f348e1399427bc962f9cac423`, based on accepted
`5008db43c9f78245bc86072131dddddaf5cfba01`. The following documentation-only
checkpoint changes no source, tests or examples. This records the author stage;
subsequent [integrated acceptance](2026-11-04-recursive-signatures.md) is recorded
separately. Source identities and counts below remain those of this stage.
This is not completion of P1, C02 or PLAN section 19.

## Scope and prechange reproduction

The [contract](../recursive-signatures.md) describes the intentional earlier
parameter constraints and expanded annotation admission. Results still constrain
all monomorphic placeholders before parameters; bodies and generalization follow.
No SCC traversal, parser, public type representation, backend or origin-role
change was made. Existing mode, loan, value-restriction, nominal and owner
boundaries remain.

Before source changes, the preserved accepted compiler rejected the f-first
defining fixture with E-MODULE-CHECK / E-RECORD and accepted the identical g-first
fixture, printing `42`. Both inputs, process exits, clean source/tree and
compiler/environment hashes are retained. Both orders now pass. The six
three-member permutations, mixed result/parameter constraints and legacy
parameter syntax are also covered.

The diagnostic code E-TYPE-COMPLEXITY is retained with accurate generic
`signature annotation input budget exceeded` wording. Exact-text result tests
were explicitly migrated. Previously unbounded parameter trees, or combined
parameter/result totals above the shared allowance, can newly reject. Earlier
parameter resolution can change diagnostic precedence in multiply-invalid
programs. Universal acceptance, wording or inference-order equivalence is not
claimed. The sixteen-case canonical baseline retains actual quantifier membership
and its historical unannotated-result projection reject/accept split.

## Final focused evidence

The selected serial GCC run passes **708 tests**: 497 library and 211 integration,
with zero failures, ignored tests, SKIP notices or warnings. Default strict
all-target Clippy, formatting, diff/local-link checks and the example output `42`
pass. The clean source identity remained unchanged throughout this final run.

The new C harness covers 22 source fixtures using typed/default and reference
C11 at O0/O2 with `-pedantic-errors`, freshly repeated after the diagnostic and
guard changes. **176 C executions pass**, 88 per compiler across GCC and Clang.
The final selected GCC run repeats its 88 executions; repeated and older-harness
runs are not added to 176. The direct typed C is byte-identical to default output.
All three evaluators agree on values, snapshots, computed read temporaries,
original-slot inout mutation, forwarding and left-to-right argument effects.

New checks cover:

- Exact canonical recursive signatures and quantified-variable membership;
  unsupported polymorphic recursion, occurs, mutable-polymorphism and
  exhaustiveness failures remain rejected.
- Conflicting modes/loans, incompatible annotations/bodies/calls, nominal identity,
  primitive arguments, unknown names, arity, holes, raw pointee spelling/constness
  and the inherited ptr<SystemBytes> owner-classification boundary.
- All six public checker/evaluator/emitter consumers at parameter and mixed
  annotation depths 127/128/129/10,000, including unused and legacy functions.
  Deliberately deep caller-owned trees are dismantled iteratively.
- Private borrowed-budget tests at depth 128/129/20,000, UTF-8/wide trees, per-tree
  weight, cumulative graph weight/work and malformed raw-pointer descendants.
  Module tests charge exact resolved-name growth across symbol-ID digit changes
  before cloning, with distinct full input and resolved-name allowances.
- Ordinary, snapshot and mapped module equality; exact nested annotation spans,
  Parameter/Annotation/TypeArgument versus FunctionResult roles, closed instance
  signatures, private import/export leaks, phantoms and alias shadowing.

The selected regression set also includes earlier result annotations, source
origins, local values, discarded results/records and accepted owner regressions.
Owner-record support is unchanged.

## Resource and process controls

Builds use two Cargo jobs, zero dev/test debug information, disabled incremental
compilation and serial tests. Fixed guards require two GiB filesystem space and
three GiB MemAvailable. New C cases check before both compilation and generated
binary execution. Selected legacy compiler commands also use guarded wrappers.
No actual resource guard failed.

The new harness exits 86 on any failed guard, stopping the whole serial process.
Intentional compile-stage and execution-stage failures verify respectively one/two
guard calls, zero/one compiler-spy calls and zero generated-binary executions;
no later case runs. These isolated control-flow probes are not real C executions.
They establish serial stopping, not cancellation of parallel or external processes.

The early local `ed2e7ca` checkpoint used historical result-only diagnostic wording.
Before broader checks, that wording and the execution guard were corrected. The
in-flight pre-migration broader compilation was interrupted with exit 130 and is
not counted as a passed run. Twenty affected focused tests passed after migration;
pre-migration and repeated logs remain separate rather than inflating final totals.

Toolchains: Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7 on x86_64 Linux.
Frozen environment SHA-256:
`6ad4935f5be1872a6725f741d733d0a4a30f7394d660a4b39d25e0610933c3c6`.
The evidence set `rill-signature-author-20261104T121331Z` retains reproduction,
commands, process diagnostics, new fixtures, guards, source identities and hashes.
Key final files are `25-selected-gcc.log`, `26-clippy.log`, `27-final-fmt.log`,
`28-example.out`, `20-migrated-gcc.log`, `21-migrated-clang.log`,
`latch/verification.txt`, `author-summary.json` and `author-evidence.sha256`.

## Reproduction and remaining gates

```sh
CC=gcc cargo test --locked --lib \
  --test recursive_signatures --test recursive_signature_origins \
  --test recursive_signature_execution --test result_annotations \
  --test result_annotation_origins --test result_annotation_execution \
  --test result_annotations_independent --test type_solver \
  --test value_restriction --test module_compile --test module_origins \
  --test typed_copy_loans --test typed_field_loans --test typed_records \
  --test typed_source_origins --test typed_node_origins --test typed_local_values \
  --test typed_discarded_results --test typed_discarded_records \
  --test returned_owner --test owner_adt --test owner_borrows --test owner_calls \
  -- --test-threads=1 --nocapture
CC=clang cargo test --locked --test recursive_signature_execution \
  -- --test-threads=1 --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -- run examples/recursive-signatures/main.rill
```

Subsequent independent review and the complete default/importer, strict lint and
Windows GNU all-target checks have [integrated acceptance](2026-11-04-recursive-signatures.md).
They do not replace this author-stage evidence or its counts.
No new Windows execution, SDL3 capture, Zig, AArch64, GPU or physical-input evidence
is claimed. Kinds, skolems, higher-rank types, owner records, effects, unknown
physical storage and native ABI expansion remain outside this increment.
