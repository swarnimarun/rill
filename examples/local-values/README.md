# Immutable local value instances

```sh
cargo run -- check examples/local-values/main.rill
cargo run -- run examples/local-values/main.rill
```

The program prints `42`. One immutable record is used as `Stored<i32>` and
`Stored<bool>`; a field is separately read as `Option<bool>`. Its integer payload
was snapshotted before the mutable source changed. Aliases share that frozen
capture, while each use has its own concrete type.

This uses the conservative value restriction and bounded
[capture-and-shape representation](../../docs/decisions/0004-local-value-shapes.md).
Calls and other effectful initializers do not gain local polymorphism.
