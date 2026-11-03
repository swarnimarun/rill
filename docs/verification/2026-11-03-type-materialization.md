# Bounded public type materialization

This is a focused P1/C02 compiler-resource increment reconstructed from local
base `7502a01`. It is not recovery of missing pre-outage bytes, completion of
P1/C02, or a whole-compiler memory/stack guarantee. Public `Type`, `FunctionType`,
`CheckedProgram`, and checker entrypoint signatures remain unchanged.

## Policy

Each checker/solver instance has a deterministic budget shared by normalization,
instantiation/substitution, final expression/signature export, and checker copies
of existing public types (including local environments and declaration-parameter
substitution). A successful output costs **64 units per expanded Type node**, plus
UTF-8 bytes in every copied nominal name or raw-pointer pointee name. Repeated
occurrences count repeatedly, even when their input is one shared DAG node.

Limits are inclusive:

- One materialized/copied tree: **16,777,216 weight units**.
- Cumulative materialized/copied output: **67,108,864 weight units**, never refunded
  when an output is dropped or consumed.
- One output tree: **128 levels**, counting its root as level one.
- Cumulative preflight work: **4,000,000 units**.

These weights are architecture-independent policy units, not allocator-byte
measurements. Work charges each traversal task, each child before scheduling it,
each raw head, and each resolved head/parent/binding lookup. A memo hit still costs
a task; its full expanded weight and depth are added at every use. Caches are
per operation, so later solver bindings cannot reuse stale summaries. Budget
arithmetic uses checked operations; the depth comparison uses saturation.

All failures return the fixed diagnostic:

```
E-TYPE-COMPLEXITY: type materialization budget exceeded; simplify the program
```

No placeholder type or successful checked result is returned. Internal signatures
propagate `Result`; public entrypoint types are unchanged. Ordinary solver
mismatches now report the fixed `E-TYPE: incompatible types`, without expanding
and recursively formatting the mismatched DAG merely to describe an error.

## Construction and cleanup

An iterative postorder preflight records borrowed heads and memoized expanded
sizes before allocating any output Type/name. Width is checked before scheduling
children. Summaries count replacement trees with multiplicity, including names.
Replacements retain the existing one-shot semantics: find the canonical solved
variable, then copy its replacement verbatim without resolving or substituting
inside that replacement. Ordinary checker copies use the same budget without
normalizing their variables.

Construction uses that completed borrowed plan. It does not re-walk potentially
long aliases for every expanded occurrence, and has no fallible budget operation
after reserving the whole output. A failed preflight owns only flat worklists,
borrowed references and scalar summaries, not a partly expanded recursive tree.
Previously successful outputs have bounded depth for ordinary cleanup. Private
50,000-deep import/free-variable/occurs/level operations still run on a 128 KiB
thread stack; exporting that depth is now an intentional diagnostic rather than
constructing a public tree requiring a custom iterative destructor.

The separately charged source-pattern cursor and its read-only, per-lookup
contract are unchanged. Materialization uses its lookup primitive with a distinct
budget; pattern analysis does not consume the materialization allowance.

## Measurements and intended rejections

The original independent review's 160-byte source declares `Pair<a,b>`,
`f0(x) = Pair(x,x)`, then five functions `fi(x) = f{i-1}(f{i-1}(x))`, with
an unrelated `main() = 42`. Both the recovered baseline and reconstructed solver
previously aborted under a 192 MiB address-space cap. The new checker returns the
fixed resource diagnostic under the same cap, with 5-second CPU, 7-second wall,
8 MiB stack and disabled core dumps. The 140-byte four-layer form still succeeds,
as do the 1k/5k/10k/20k independent-parameter probes. Repeated uses of the
four-layer function also terminate with the resource diagnostic under those caps.

The four-layer result has 131,071 Type nodes, 65,535 four-byte names and weight
8,650,684, below the per-tree limit. A temporary, uncommitted measurement build
recorded 17,562,228 total output units and 11,630 preflight work units for its
complete check. The existing source-solver suite's 136 analyses peaked at 5,889,167
output units and 272,049 preflight work units. These margins informed the limits; the thresholds are not
a claim that every otherwise legal or practically compilable program is accepted.

Resource-dependent rejections are deliberate: oversized expanded trees, overly
deep public output, and sufficiently many individually small copies can fail.
Every fresh analyze/check call starts a new allowance. Successful tractable types
and schemes retain their previous meaning; no truncation or fabricated value is
used to stay within a budget.

## Verification

Fresh verification used Rust/Cargo 1.99.1 and GCC 14.2.0 on x86_64 Linux.

- The affected default run passed **185 tests**: 143 library tests plus 42 source,
  typed/reference/C, cursor, public-entrypoint and SCC integration tests.
- With `c-import`, **59 checker tests** and **19 source tests** passed.
- Strict all-target Clippy passed in both default and `c-import` modes.
- `cargo fmt --all -- --check` and `git diff --check` passed.
- The prior independent review's generated-source harness was freshly rebuilt
  against this implementation and exact baseline `e0bc124`: all **30,000** shuffled
  modules agreed (21,884 accepted alpha-equivalent schemes and 8,116 matching
  rejection classes), plus five targeted cases and AST identity checks.
- A separate bounded direct comparison of duplication layers zero through four
  matched 132,249 public type nodes exactly modulo variable renaming and retained
  original AST identities. Its initial harness omitted builtin atomic cases;
  restricting that comparison to the intended source functions corrected the
  harness, with no production change.

Permanent regressions include eleven private budget tests and four source tests.
They cover exact inclusive limits, arithmetic overflow, UTF-8/pointer name bytes,
shared-DAG multiplicity, cumulative copies, raw replacement/alias semantics, changed
representatives, cached-subtree depth, malformed private cycles, fixed mismatch
errors, small-stack cleanup and the bounded historical OOM. Tractable unreachable
and reachable duplication fixtures agree across AST/reference evaluation, checked
typed evaluation and compiled C at `-O0` and `-O2`.

```sh
cargo test --locked --lib --test type_materialization --test type_solver \
  --test public_entrypoints --test call_graph --test exhaustiveness_budget \
  --test typed_specialization --test typed_copy_patterns --test typed_copy_loans \
  --test adt_interpreter --no-fail-fast
cargo test --locked --features c-import --lib check::
cargo test --locked --features c-import --test type_materialization \
  --test type_solver --test exhaustiveness_budget
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo fmt --all -- --check
```

Independent review and full combined default/feature aggregates remain the
integrator's separate gates. No new SDL3, Zig, Windows or AArch64 execution is
claimed by this focused checkpoint.

## Remaining boundaries

This bounds specified expansion/copy operations, not all allocations or work in
the compiler. Input source/AST storage, source-directed type skeleton creation,
solver arena/interning growth, unification/free-variable work, declaration maps,
quantifier vectors, other source validation, parser/AST consumers and downstream
public-type consumers have separate limits or still need them. Public callers can
construct arbitrarily deep/large Types or ASTs and invoke their derived operations;
this increment does not make those APIs generally stack-safe. A 192 MiB probe is
evidence for the tested inputs, not a memory sandbox for arbitrary programs.

Kinds, rigid/skolem variables, effects, ownership extensions and C03 value
restriction are not implemented by this increment.

## Narrow C03 integration

A separate integration applies the frozen author checkpoint `ebc63cb` to
`00b0b9d`, which already contains the reviewed value restriction, source-origin,
record and field-loan increments. The author branch remains unchanged. The only
textual cherry-pick conflict was the initial type in `record_field`: integration
retains C03's per-use `instantiate(binding)?`, rather than copying its generic
scheme. The checker copy helper retains `Scheme.borrowed`.

`has_owner_dependency` now returns `Result<bool, String>` and propagates fallible
borrowed resolution. Its complete declaration/phantom traversal is otherwise
unchanged, as are eligibility, weak-variable lowering, projection behavior and
pattern-cursor accounting. A textual comparison verified identical eligibility
and identical owner traversal after normalizing only this Result syntax.

Fresh integration checks passed **270 affected default tests**, including all
17 value-restriction tests, owner paths, records, field loans, materialization and
pattern regressions. With `c-import`, **59 checker** and **50 source/integration**
tests passed. Formatting, diff checks and strict all-target Clippy in both modes
passed. The 30,000-module source comparison was freshly rebuilt against exact
integration base `00b0b9d`; accepted schemes/rejection classes, five targeted cases
and AST identities agreed. These are integration-specific results, not reused
preintegration evidence. Independent original/integration reviews and the full
combined aggregate gate remain separate.
