# Rill

A zero-host, mutable-value-semantics and Verse-effect native systems language,
compiled from Rust through MLIR. This repository currently contains the **M0
front end**: the lexer, the lossless parser, the CST, the typed AST and the
computation-builder desugaring pass. See `PLAN.md` for the full architecture and
the M1–M6 roadmap.

## Status

| Milestone | Scope | State |
| --- | --- | --- |
| **M0** | `rill-span`, `rill-diagnostics`, `rill-lexer`, `rill-syntax`, `rill-parser`, `rill-ast` — lossless CST, adjacency rule, `.` flow, builder desugaring | **done** |
| M1 | `rill-hir`, `rill-ty` — HM(X) inference, Rémy effect rows, trait constraints, arity uncurrying | next |
| M2 | `rill-mir`, `rill-borrow` — place-CFG MIR, exclusivity checking, slice splitting | planned |
| M3 | `rill-mlir`, `rill-driver` — MLIR lowering, native static binaries via `lld` | planned |
| M4 | `<transacts>` — SSA promotion, undo trails, scoped `<alloc>` arenas | planned |
| M5 | `<suspends>` — `sync`/`race`/`rush`/`branch`, `simd`/`seq` builders | planned |
| M6 | Self-hosting bootstrap | planned |

M0 verification (see `crates/rill-ast/tests/`): every Rill example from
PLAN.md §2–§11 parses into a lossless CST **and** a desugared AST with zero
diagnostics, and the CST round-trips byte-for-byte.

## Layout

```text
crates/
├── rill-span/         interned symbols, byte positions, spans, source map (0 deps)
├── rill-diagnostics/  labeled errors with ariadne-style rendering
├── rill-syntax/       SyntaxKind, lossless CST, operator precedence table
├── rill-lexer/        hand-written cursor lexer (trivia-preserving)
├── rill-parser/       resilient Pratt + recursive-descent parser (events -> CST)
├── rill-ast/          AST nodes, CST -> AST lowering, builder desugaring
└── rill-driver/       `rillc`, the front-end driver binary
examples/showcase.rill  the PLAN.md §11 program, parsed by the test suite
```

## Usage

```console
$ cargo run -p rill-driver -- --stats examples/showcase.rill
examples/showcase.rill: 7 items (7 declarations), 567 tokens, 0 errors, 0 warnings

$ cargo run -p rill-driver -- --dump=cst examples/showcase.rill   # lossless CST
$ cargo run -p rill-driver -- --dump=ast examples/showcase.rill   # desugared AST
```

`rillc` exits non-zero when any phase reports an error, printing the rendered
diagnostic with its source line and caret:

```text
error: expected an expression
  --> broken.rill:1:9
  |
1 | let x = );
  |         ^
```

Library entry points:

```rust
use rill_ast::parse_and_lower;          // lex -> parse -> lower -> desugar
use rill_parser::parse_source;          // lex -> parse (CST only)
use rill_span::SourceMap;

let mut source_map = SourceMap::new();
let file = source_map.add_file("example.rill", source);
let frontend = parse_and_lower(&source_map, file);
assert!(frontend.is_ok());
```

## Design decisions worth knowing

* **Lossless by construction.** The parser emits `Start`/`Token`/`Finish`
  events; trivia is never consumed by the parser and is interleaved into the
  tree when the next real token is claimed. Every token appears exactly once,
  in source order, so `cst.text(&source_map) == source` always holds — even for
  malformed input, where recovery emits `Error` nodes instead of dropping
  tokens.
* **Adjacency rule (PLAN §2.5).** `xs[i]` / `f[x, y]` are bracket applications
  (`IndexExpr`) because the `[` touches the preceding atom; `xs [i]` is an
  application to an array literal. `f(x)`, `f (x)` and `f x` all splice their
  arguments into one `CallExpr` (`rotate_point (inout a, b)` passes two
  arguments), and `e?` is a postfix try.
* **Postfix binds tighter than application.** The plan's precedence table lists
  postfix (level 12) below application (level 13); taken literally the
  adjacency rule would be dead syntax (`f x[i]` could never index `x`), so
  postfix attaches to the immediately preceding atom and application spines
  apply to the result. See `rill_syntax::bp`.
* **`and` / `or` / `not`** are contextual: they lex as ordinary names and act
  as operators only in operator position, so user code can still bind those
  names elsewhere.
* **Builder desugaring (PLAN §6.1)** rewrites `b { ... }` for declared builders
  and for the standard `seq`/`simd`/`gpu`/`arena`/`query` builders into the
  `bind` / `try_bind` / `yield_val` / `for_each` / `combine` / `delay` / `ret`
  protocol, and expands `and`/`or` into `if`. `option`/`one`/`all` and
  `sync`/`race`/`rush`/`branch` are *built-in* forms whose semantics belong to
  the compiler (§3.3, §5.3), so they stay as `Handler`/`Concurrent` nodes
  instead of method calls.
* **`when` guards** have no row in the §6.1 table. Inside a builder block a
  guard expands to `combine (if (cond) { () } else { fail }) (delay { rest })`,
  using §5.1's failure primitive; inside built-in handlers the guard is kept for
  the handler to interpret.
* **Errors are data.** Nothing in the front end panics on malformed input:
  unexpected CST shapes lower to `Error` nodes with a diagnostic, guaranteed
  progress rules keep the parser advancing, and `rillc` reports and exits.

## Development

```console
$ cargo test --workspace     # lexer, parser, diagnostics, lowering, desugar
$ cargo clippy --workspace --all-targets
$ cargo fmt --all
```
