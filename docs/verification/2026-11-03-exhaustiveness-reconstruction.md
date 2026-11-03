# Reconstructed bounded pattern coverage

This report concerns new code reconstructed after the cloud filesystem loss,
from the durable recovery checkpoint. Missing commits and earlier review/test
results are not evidence for this reconstruction. This is a focused P1
compiler-resource correction, not completion of P1 or the entire Rill plan.

## Defect and implementation

The prior source coverage check recursively enumerated constructor products,
even for a total row. Five nested `Pair<a> = Pair(a, a)` layers over `bool`
can induce 2^32 Boolean combinations in this tiny unused function:

```rill
type Pair<a> = Pair(a, a)
fn ignored(x: Pair<Pair<Pair<Pair<Pair<bool>>>>>) {
    match x { _ => 42 }
}
fn main() { 42 }
```

The replacement in `src/check/exhaustiveness.rs` uses explicit worklists,
interned pattern/type nodes, total-row shortcuts, removal of columns wild in
every row, sorted/deduplicated matrices, and memoized obligations. Binding
names normalize to wildcards for coverage only. Original AST patterns and arm
order remain intact, preserving first-match execution. All arms are still
inferred and checked before coverage, including arms after a catch-all.

The private `type_cursor` seam returns borrowed type heads and child cursors.
Coverage never calls public type export or recursively substitutes/clones a
solver DAG before budgeting it. Child counts are charged before scheduling
children. Every hidden alias/head lookup is charged before it executes. The
standalone seam follows the existing binding table iteratively;
replacing that table with the separate union-find solver requires the planned
private-cursor integration and fresh combined verification. That integration
is not implied by this standalone report.

Recursive inline nominal types and empty variant declarations remain rejected
before coverage. Bool and unit are finite domains. Other primitives and open
type variables retain the existing default-row rule; integer literal patterns
alone do not establish exhaustiveness. This proof adds no nesting limit.
Existing parser, type-inference, typed-layout and original-AST cloning limits
remain separate; this is not a whole-compiler stack or memory guarantee.

## Deterministic resource policy

Each source match receives exactly **100,000 work units**. Consuming the final
unit succeeds. A later positive charge or a multiplication overflow fails with:

```text
E-MATCH-COMPLEXITY: pattern analysis exceeded 100000 work units; simplify the match
```

This is not a claim that the match is semantically incomplete. Proof exhaustion
never becomes `false`, a fabricated witness, or an assumed exhaustive match.
The units are deterministic accounting weights, not milliseconds or CPU
instructions. The weights in this increment are:

- Initial scan: arm count. A top-level wildcard/binder succeeds immediately
  without inspecting its type or building either arena.
- Pattern, declared-type, and source-cursor traversal: one unit per task.
  Resolved source-head lookup additionally charges one per alias/head inspection,
  before each lookup; long alias chains cannot hide work inside one cursor call.
  Scheduling children and collecting child IDs each charge their arity.
  Declared-type name lookup additionally charges its byte length.
- Interning, including cache hits: `3 * (1 + key_width)`. Nominal/constructor
  key width is name byte length plus child count; primitive width is zero.
- Each matrix: one unit plus `rows * (columns + 1)` for row inspection.
  Unconstrained-column removal charges `(rows + 1) * columns`.
- Sorting/deduplication/memoization:
  `rows * (remaining_columns + 1) * (bit_length(rows) + 4)`.
- Nominal specialization: name bytes plus argument count. Each constructor
  charges one plus its name bytes, parameter/field counts and parameter-name
  byte lengths. Field conversion uses the traversal/interning weights above.
- Each specialized matrix: `(rows + 1) * (new_columns + 1)` before allocation,
  including candidate rows later rejected. Constructor-pattern comparisons
  additionally charge the pattern constructor name's bytes.

Generated matrix/type storage is charged, not only recursive-call count.
Simplifying a match or dividing it into smaller matches can reduce proof work.
An appropriate catch-all is cheap but must preserve the intended behavior.

The separate typed Copy-match verifier is unchanged. It already has an
iterative proof, a total-row shortcut and its own 100,000-unit guard, reporting
`E-TYPED-MATCH: pattern analysis budget exceeded`. Its accounting is independent,
not claimed identical to source coverage.

## Fresh reconstruction checks

```sh
cargo test --locked --lib --test exhaustiveness_budget \
  --test typed_copy_patterns --test adt_interpreter \
  --test public_entrypoints --no-fail-fast
```

Using Rust/Cargo 1.99.1 and GCC 14.2.0 on x86_64 Linux, the fresh
affected-component run passed **124 tests**: 108 library tests,
six exhaustiveness integration tests, five typed interpreter/C differential
tests, four public-entrypoint checks and one ADT interpreter test. The C cases
run at both `-O0` and `-O2`.

Coverage includes:

- All 512 subsets of nine Boolean-product patterns against all four values.
- Another 512 subsets of nested option/bool/unit patterns against six values,
  checked by both source and independent typed proofs, including missing cases.
- Direct accounting tests for the exact budget boundary, overflow, a one-unit
  catch-all with no arena/type inspection, and one memoized repeated suffix.
- An 80-layer shared type DAG: ordinary traversal halts with the exact budget
  diagnostic before expanded-tree materialization; a catch-all does no traversal.
- Cursor traversal through aliases, resolved bools, open variables, and ordered
  nominal children, including both direct and bound nominal roots.
- Five, ten and 24 nested Pair layers with wildcard and binding catch-alls;
  a reachable five-layer value lowers, verifies and evaluates to `42`.
- A 1,024-field unit pattern and an exhaustive five-pigeon/four-hole pattern
  matrix return the deterministic complexity diagnostic. Total-row variants pass.
- Nested missing cases, generic parameter names shadowing primitive names,
  integer defaults, impossible patterns, invalid arities, duplicate bindings,
  invalid unreachable bodies, recursive types, empty matches and empty ADTs.

Potentially explosive cases run in child processes with a 30-second kill/reap
safety deadline. Results and counters determine success; timing is not the
compiler's coverage policy.

`cargo fmt --all -- --check`, `git diff --check`, default strict all-target
Clippy, and `cargo clippy --locked --features c-import --all-targets -- -D warnings`
passed on this reconstruction. Fresh independent review is still pending; no
old review result is claimed. Repository-wide and combined solver verification
are separate checkpoints.

## Fresh review correction: hidden alias work

Independent review found that the reconstructed standalone cursor walked alias
chains without charging each hop. A direct checker fixture with 200,000 links
reused by 2,000 nominal arguments performed 400 million lookups while remaining
under the nominal proof budget. This did not change coverage truth, but violated
the intended bounded-work seam.

The cursor now accepts a fallible charging callback and invokes it before every
alias/head inspection. Coverage supplies its existing match budget and propagates
the same deterministic error. New regressions check the exact two-versus-three
lookup boundary, repeated long aliases, and bounded failure on a malformed
internal alias cycle. The separate private-solver bridge must preserve this
lookup-level budget contract rather than calling an unbounded resolver.

Fresh affected re-verification passed 126 tests: 110 library, six source
exhaustiveness, five typed C differential, four public-entrypoint and one ADT
interpreter test. Both strict Clippy configurations, formatting and diff checks
also passed. Independent reviewer confirmation is pending; the original
124-test result above describes the earlier standalone checkpoint.


## Subsequent private-solver integration

The [charged solver bridge](2026-11-03-solver-pattern-bridge.md) adapts these
fixtures and the borrowed cursor to interned terms and ranked union-find.
It charges each parent lookup and the root binding separately, so its exact
callback counts differ from the standalone vector cursor described above.
The standalone verification counts remain attached to their original snapshots.
