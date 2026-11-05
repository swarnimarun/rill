# Rust migration recipes

These complete programs accompany the
[recipes guide](../../../docs/rust-migration-recipes.md).

| Source | Output | Uses |
| --- | --- | --- |
| `range-filter.rill` | `25` | Half-open cursor, mutable loan, guarded work |
| `copy-callback.rill` | `42` | Generic function parameter, immutable Copy capture |
| `count-option.rill` | `8` | Supplied token, typed parse error and range policy |

From the repository root:

```sh
rill run examples/rust-migration/recipes/range-filter.rill
rill run examples/rust-migration/recipes/copy-callback.rill --interpret
rill build examples/rust-migration/recipes/count-option.rill -o /tmp/rill-count
```

The count example parses a literal token. It does not read process argv.
The range example traps if its i32 sum overflows.
