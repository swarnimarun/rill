# Functions, closures and currying

Run this example from the repository root:

```sh
cargo run -- run examples/functions/main.rill
cargo run -- run examples/functions/main.rill --interpret
cargo run -- build examples/functions/main.rill -o /tmp/rill-functions
/tmp/rill-functions
```

All executions print `42`. The example infers an identity lambda at bool and
i32, partially applies a two-argument function through a generic callback,
returns a closure with an inline integer capture, and composes two closures.

See [function syntax and boundaries](../../docs/functions.md) and
[editor support](../../docs/editor-support.md) for inferred hints and signature help.
