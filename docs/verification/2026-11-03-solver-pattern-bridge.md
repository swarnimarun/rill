# Charged solver cursor for source coverage

This checkpoint integrates the reconstructed private rank-one solver with the
separately reconstructed bounded source-pattern proof. It builds on main
`2e7eaef` and imports the standalone proof and its hidden-alias correction before
adding the bridge. The only cherry-pick conflict was adjacent module declarations;
both `solver` and `type_cursor` are retained. The public checker and typed-kernel
representations are unchanged.

## Private seam and exact accounting

`check/type_cursor.rs` defines a borrowed private cursor with public-tree and
private-arena forms. `TypeHead` distinguishes canonical variables, atoms and
nominal heads. Nominal children remain borrowed public slices or private term-ID
slices. Their length and indexed access do not resolve or allocate descendants.
The source proof charges the number of child cursors before scheduling them.

`Checker::type_head` delegates to a dedicated `Solver::type_head` that accepts a
fallible charging callback. This is deliberately separate from the ordinary
solver's `representative`, `head` and `view` helpers. It does not use those helpers
and does not compress paths, intern terms, cache results or mutate solver state.

The callback is invoked **before** each of these operations:

1. Inspecting one public type head or one private arena term.
2. Looking up each variable's parent, including the representative's self-parent.
3. Looking up the representative's binding, separately from its parent lookup.

A public atom or nominal head costs one callback. An unbound public root costs
three: head, root parent and root binding. A root bound to an atom costs four,
including the private atom. Each additional nonrepresentative parent adds one.
Starting from a private variable term charges that initial arena lookup instead
of a public-head lookup. A failed callback returns its error immediately, before
the corresponding lookup. Neither successful nor failed inspection changes the
solver, even when a parent path could have been compressed.

Source coverage supplies its existing 100,000-unit budget. Its own task visits,
child scheduling, arena retention and matrix work remain separately charged.
The unit of work is an explicit implementation counter, not elapsed time. Hidden
alias paths therefore cannot consume unlimited work behind a shallow head.
An invalid internal parent or binding cycle terminates when that budget runs out.
This is defensive internal behavior, not permission to construct invalid cursors
or pass a callback that never imposes a limit.

## Fresh bridge regressions

The source-proof fixtures now construct solver variables and bindings through
`fresh`/`unify`, rather than accessing the removed binding vector. They preserve:

- An 80-layer shared binary DAG that stops at `E-MATCH-COMPLEXITY` without
  materializing its expanded public tree.
- A wildcard or binding catch-all that needs exactly one work unit and does no
  type inspection or proof-arena construction.
- Resolved booleans, open variables, aliases and ordered nominal children,
  including direct public and bound private nominal heads.
- Exact head-lookup charges and repeated legitimate rank-deep aliases which
  exhaust the proof budget rather than charging only their final boolean heads.

Six additional private cursor tests cover callback failure at every lookup
boundary; state snapshots before and after both success and failure;
rank-induced root changes and nonrepresentative public/private aliases; mixed
public/private child trees and child order; failure before deliberately invalid
lookups; no inspection of 100,000 public nominal children during a one-unit head
read; a 200,000-parent adverse chain stopped by a 100,000-unit budget; 1,000
visits to a rank-12 leaf costing exactly 16,000 callbacks; and bounded termination
of deliberately malformed parent and binding cycles. The adverse-chain fixture
bypasses ranked union only inside its private test.

## Verification

Fresh checks used Rust/Cargo 1.99.1 and GCC 14.2.0 on x86_64 Linux, with the
restored optional toolchain environment and an isolated build directory.

```sh
cargo test --locked --lib --test exhaustiveness_budget --test type_solver \
  --test typed_copy_patterns --test typed_copy_loans --test typed_specialization \
  --test adt_interpreter --test public_entrypoints --test call_graph --no-fail-fast
cargo test --locked --features c-import --lib check::
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo fmt --all -- --check
```

The affected default run passed **170 tests**: 132 library, six source coverage,
nine source solver, five typed Copy-pattern, four typed root-loan, five typed
specialization, one ADT interpreter, four public-entrypoint and four SCC tests.
The feature-enabled checker run passed **48 tests**. Both strict Clippy modes,
formatting and diff whitespace checks passed. The typed differential fixtures
execute C at `-O0` and `-O2`.

Independent bridge review and the combined repository-wide default/feature gate
remain separate steps. The earlier standalone proof and core-solver counts are
not reused as proof of this integration. No new SDL3, Zig, Windows or AArch64
execution claim is made by this focused bridge checkpoint.

## Remaining resource boundary

This bridge bounds source coverage's view of inferred types. It does not impose
a global compiler resource policy. Existing metadata export, normalization,
diagnostic formatting and public-tree consumers can still materialize a very
large tree from a compact generic DAG. A separate core audit reproduced a
160-byte generic-duplication program exhausting a 192 MiB limit in both the
recovered baseline and the new core solver. That inherited limitation is not
fixed or made safe by this cursor; no general resource-safety claim is made.

Kinds, rigid/skolem escape checks, higher-rank syntax and C03 value restriction
are outside this bridge. The typed Copy-match verifier keeps its own independent
proof and budget. Complete P1/P2 acceptance remains open.

The subsequent [public-materialization budget](2026-11-03-type-materialization.md)
adds a fallible size/depth/cumulative policy for these expansion and checker-copy
paths, including the tiny generic-duplication failure. It does not establish
whole-compiler resource safety.
