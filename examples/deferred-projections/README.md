# Deferred nominal projections

Both examples print `42` through the checked interpreter and production C:

```sh
cargo run -- run examples/deferred-projections/main.rill
cargo run -- build examples/deferred-projections/main.rill -o /tmp/rill-projection
/tmp/rill-projection
cargo run -- run examples/deferred-projections/fields.rill
```

`main.rill` projects an unannotated recursive callee result before that callee's
body determines its nominal type. The completed recursive group retains ordinary
polymorphism, so `project` is used at both bool and i32. `fields.rill` resolves an
inout field assignment through an intermediate unannotated parameter in the same
group. Field declaration order differs from construction order.

These are bounded SCC-local equality constraints, not structural records or
caller-driven type guessing. A standalone `fn project(p) { p.value }` still needs
a nominal constraint inside its own component. See the
[contract](../../docs/deferred-projections.md).
