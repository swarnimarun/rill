# Checking inference and owner moves against executable models

Run the two finite models with Cargo and a C11 compiler named `cc`.

```sh
cargo test --locked --test core_models
```

The model derives an expected typing judgment and observable result for each
program before the Rill parser sees its source. Each accepted program runs in
the checked interpreter. Option programs also run in the reference AST
interpreter. The harness compiles batches of accepted functions through the
public C backend at `-O0` and `-O3`. Each function has a separate result check,
with a one-based failure index in the batch. Every rejected program must produce
the expected diagnostic class at checking, interpretation and C emission.
The [test source](../tests/core_models.rs) defines the complete enumeration.

## Optional payloads, specialization and snapshots

The finite payload domain contains `i32` and `bool`. Each program introduces
one of five bindings, performs an operation on it, binds an immutable snapshot,
performs a second operation on the original and observes the snapshot.
Each observation uses an annotated function that exhaustively matches `Some`
and `None`. Mutable bindings also admit writes of either payload.

| Initial binding | Static rule |
| --- | --- |
| Immutable `None` | Each observation may choose its payload type independently. |
| Immutable `absent()`, which returns `None` | One ground payload type must satisfy all uses, including uses through the snapshot. |
| Mutable `None`, `Some(7)` or `Some(true)` | One ground payload type must satisfy the initializer, writes and reads of either binding. |

For a monotype, the model tries both possible ground types. For the value
scheme each use chooses its own specialization. A snapshot copies the current
optional value. Subsequent writes affect the original slot. The result
encodes the three observations as separate digits, so an incorrect payload
or snapshot becomes visible. Of 112 programs, 26 type-check and 86 require
`E-TYPE`.

## Moves through conditional branches

Two hosted `SystemBoxI32` roots start with distinct integers. Each branch
reads the first root, changes it, or transfers either root into a block-local
binding and reads the moved value. The continuation either reads a root,
moves the first root again, or attempts an implicit copy. Both truth values
are enumerated for every branch pair and continuation.

The model represents each outer root as a live integer or an empty slot.
A move takes the integer from a live slot. A copy of an owner is invalid.
The branch join requires equal sets of live roots on both exits. Integer
contents may differ and the selected branch supplies the contents for the
continuation. Of 128 programs, 30 type-check and 98 require `E-OWNED`.

## What the results establish

These 240 generated programs exercise value restriction, snapshots, use after
move and control-flow joins on their stated finite domains. The model assumes
successful allocation and checks ordinary integer results. Cleanup order and
allocation failure have separate [owner fixtures](../tests/owned_box.rs).
The corpus omits recursion, stored borrows, overlapping projections,
higher-rank inference and foreign code.

An unbounded soundness result requires an operational semantics and a proof
that each well-typed step preserves the typing and store invariants and that
every well-typed closed configuration can step, return or take a specified
trap. Each extension should state its additional invariants, add a minimal
compiler regression for any counterexample and rerun the independent model.
Those proof obligations remain open under [PLAN section 19](../PLAN.md#19-dependency-ordered-delivery-plan).
