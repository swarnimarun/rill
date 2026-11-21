# Reconstructed AST origin mapping, 3 November 2026

This code was reconstructed after filesystem loss over grammar checkpoint
`d140133`. The missing earlier AST-origin commit was not recovered and its tests
are not claimed for this code. See [the API contract](../ast-source-origins.md).

The new optional post-parse map covers every structural AST node family in the
accepted parser subset. Typed structural paths validate roles and bounds;
origins retain source spans and explicit source/desugaring provenance. An error
returns no partial map and does not affect strict parser acceptance. No AST,
checker, typed IR, interpreter, backend or importer representation changed.
The only parser seam adds missing grammar markers for legacy function parameters.

## Fresh focused verification

Rust/Cargo 1.99.1 on x86_64 Linux, isolated target directory:

```sh
cargo test --locked --test ast_source_map --test syntax_tree --test parser_stack
cargo test --locked --lib source_map::tests
cargo test --locked --lib parser::tests
cargo test --locked --test source_format --test format_cli --test brace_syntax
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
```

Passed 11 AST-origin tests, 9 grammar-tree tests, the constrained-stack parser
test, one internal mapping-failure test, 18 parser tests and 22 formatter/brace
regressions: 62 focused tests. Tests independently count all structural AST
nodes over 38 repository source files and verify exactly one origin for each.
They also cover every origin category, foreign slots, nested types/patterns,
legacy and braced parameters, synthesized block bindings/discard units, unary
zeros, empty metadata-only modules, UTF-8, invalid paths, stable result ordering,
accepted token mutations and 1,000 generated expression combinations.

The mapping test runs on a 2 MiB stack with near-limit operator/type nesting and
block bindings. An intentionally mismatched internal AST produces a deterministic
mapping error while the original strict parser result remains unchanged.

Independent review of the frozen origin checkpoint and combined full regression
remain integration gates owned by the parent continuation. No phase completion,
resolved/module source preservation or generated-code mapping is claimed here.
