# Equality and bounded search

These examples accompany [the search guide](../../docs/search-and-equality.md).

```sh
rill run examples/search/first-token.rill
rill run examples/search/substring.rill --interpret
rill run examples/search/compare-values.rill
```

`first-token.rill` prints `1`, the first position of `"one"` in a small fixed
token provider. Its result records progress and skipped suffix work so the
loop's behavior can be tested. `substring.rill` prints `2`, a zero-based byte
offset. Neither example needs process argv, runtime string allocation or a
`break` statement. `compare-values.rill` prints `true` using explicit field
comparisons and nested enum matches.
