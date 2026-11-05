# Examples for Rust developers

All four programs print `42` on successful allocation. They accompany the
[migration guide](../../docs/rust-to-rill.md).

```sh
rill check examples/rust-migration/basics.rill
rill run examples/rust-migration/control-flow.rill
rill run examples/rust-migration/results.rill --interpret
rill build examples/rust-migration/ownership.rill -o /tmp/rill-ownership
```

`control-flow.rill` uses a loop condition to stop searching and a guarded body
to skip even values. It contains no `break` or `continue`. `results.rill`
demonstrates explicit propagation decisions, not general overflow-safe division.
Its `divide` function still traps on the i32 minimum divided by minus one.
