# Reconstructed grammar tree, 3 November 2026

The old grammar implementation and its later origin-map work were unavailable
after cloud filesystem loss. This is newly reconstructed code from restored
baseline `e0bc124`, not a recovered old commit. Previous verification is not
claimed for the reconstruction.

The existing lexer/parser now optionally emits exact-byte grammar trees,
structured diagnostics, declaration-level AST source mappings and bounded
(top-level, 64-diagnostic maximum) recovery. Any error suppresses AST/mappings.
Lexical failure retains unlexed bytes with the first exact scanner diagnostic.
Strict parser and formatter behavior is preserved; no semantic backend changes.

Fresh checks on Rust/Cargo 1.99.1, x86_64 Linux:

```sh
cargo test --locked --lib parser::tests
cargo test --locked --test syntax_tree --test parser_stack
cargo test --locked --test source_format --test format_cli --test brace_syntax \
  --test module_graph --test module_compile --test native_modules
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
```

Passed 18 parser unit tests, 9 grammar-tree tests, the 2 MiB constrained-stack
parser test and 45 formatter/brace/module/native tests: 73 focused tests.
Grammar tests check all 38 repository source files, AST equality, formatting
idempotence, exact leaf partitions, deterministic IDs/diagnostics, precedence,
parentheses, Unicode positions, lexical failures, missing delimiters, native
rollback, the diagnostic limit, token mutations and 1,000 random sequences.
The stack test also exercises source recovery for seven 10,000-deep inputs.
Existing formatter tests retain interpreter/C comparisons at `-O0` and `-O2`.

Fresh independent review and the combined full aggregate remain parent-owned
integration gates. Full expression/IR origins, finer recovery and editor APIs
are not implemented by this commit. P1 is not complete.

## Fresh independent grammar review

The frozen grammar checkpoint was independently compared against `e0bc124` over
87,739 cases: 1,181 extracted literals, mutations, 6,750 generated expression
combinations, malformed random streams, Unicode comment lines and large
multi-declaration files. Strict parser/module ASTs or exact errors, lexer and
selected formatter outputs agreed. Independent position computation, exact leaf
partitions and deterministic recovery also passed. The subsequently discovered
missing legacy-parameter grammar markers are fixed and regression-tested in the
AST-origin increment; strict parsing had remained unchanged.
