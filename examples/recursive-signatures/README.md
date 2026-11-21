# Closed parameters in recursive groups

```sh
cargo run -- run examples/recursive-signatures/main.rill
```

The program prints `42`. `peer` constrains its `Point` parameter before either
body is inferred, so `inspect` can learn that record type from its call before
projecting `point.x`. Reordering the two declarations preserves this result.
The computed `read` argument is a call-scoped temporary; no borrowed address
escapes. Existing value/read/inout rules are unchanged.

This is a bounded signature-order improvement. An unannotated callee result or
unannotated intermediate parameter can still be unknown when a body projects a
field. See the [contract](../../docs/recursive-signatures.md).
