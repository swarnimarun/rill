# Explicit generation hooks

Run `cargo run -- run examples/reload/main.rill`. It prints `13`, using
`std.reload` to distinguish first load, an ordinary update and a new generation.
The generation values are explicit example inputs, not filesystem events.

Run `cargo run --example reload_host` to replace a checked interpreter hook
between calls while keeping integer state in the Rust host. It prints
`generation 1: state 43`.

For native application editing use `cargo run -- project watch examples/project`.
See [reload contracts](../../docs/hot-reload.md) for the exact boundaries.
