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
| M1 | `rill-hir`, `rill-ty` — name resolution + HIR (done); inference, effect rows, arity uncurrying | in progress |
| M2 | `rill-mir`, `rill-borrow` — place-CFG MIR, exclusivity checking, slice splitting | planned |
| M3 | `rill-mlir`, `rill-driver` — MLIR lowering, native binaries, no runtime | next |
| M4 | `<transacts>` undo trails; `<alloc>` growable buffers (done) and scoped arenas | in progress |
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
├── rill-hir/          name resolution: Res/LocalId/ExprId tables + HIR
├── rill-ty/           type & effect checking over the HIR
├── rill-cimport/      C header scanner: declarations from an `@include`
└── rill-driver/       `rillc`, the front-end driver binary
examples/showcase.rill  the PLAN.md §11 program, parsed by the test suite
```

## Backend: MLIR -> LLVM IR -> object, no runtime

Code generation is *direct*: typed HIR is lowered to the project's MLIR dialect
(`func`/`arith`/`cf`/`llvm` built from the HIR, with `rill.*` ops for
`<decides>`/`<transacts>`/projections as they land), then `mlir-opt` /
`mlir-translate` produce LLVM IR, and a linker produces the executable.

There is deliberately **no C transpilation and no runtime library**: a program's
observable result is its `main` exit code, so a self-contained binary needs
nothing under it. Only the platform's linker/CRT is involved, and for a fully
static artifact the LLVM IR is compiled for a musl target:

```console
$ /opt/homebrew/opt/llvm/bin/mlir-opt --convert-to-llvm probe.mlir \
    | /opt/homebrew/opt/llvm/bin/mlir-translate --mlir-to-llvmir -o probe.ll
$ clang -O2 -o probe probe.ll                    # native, exit code 42
$ zig cc -target aarch64-linux-musl -static -O2 -o probe probe.ll
$ file probe      # ELF 64-bit ... statically linked
```

Toolchain verified on this workstation: Homebrew LLVM 22.1.8 (`mlir-opt`,
`mlir-translate`, `llc`) with `melior` 0.27.8 binding `mlir-sys` 220.0.2 (MLIR
22), and `zig cc` for static ELF linking.

## Usage

```console
$ cargo run -p rill-driver -- --stats examples/showcase.rill
examples/showcase.rill: 7 items (7 declarations), 567 tokens, 0 errors, 0 warnings

$ cargo run -p rill-driver -- --dump=cst examples/showcase.rill   # lossless CST
$ cargo run -p rill-driver -- --dump=ast examples/showcase.rill   # desugared AST

$ cargo run -p rill-driver -- exec examples/fib.rill              # compile + run
fib(10) = 55
```

`exec` caches the binary and the C source it was generated from under
`~/.rill/cache`, keyed by a checksum of the script, so an unchanged script runs
without invoking the C toolchain again; the process exit code is the program's.

## C headers: `@link` and `@include`

A script talks to an installed C library by naming its header and its library;
there is no build file and no binding generator:

```rill
@link("SDL3");

type SDL3 = @include("SDL3/SDL.h");
use SDL3::*;

let main () -> i32 <io> = {
    SDL_Init (SDL_INIT_VIDEO);
    var win = SDL_CreateWindow ("hello", 640, 360, 0);
    SDL_DestroyWindow (win);
    SDL_Quit ();
    0
};
```

`@include` scans the header (and the headers it includes) and imports its
prototypes as `extern "c"` functions with an `<io>` row, its `typedef`s as
records whose C names are kept, its enums as `i32`, and its `#define`
literals as constants; `use M::*;` preludes their names, and `M::name` reaches
one without a `use`. The header's documentation comes along: SDL3's Doxygen
blocks and raylib's trailing comments are turned into Markdown that the editor
shows when the name is completed or hovered, and the comments on record fields
and enumerators travel with those members. `///` in Rill code works the same
way - a run of `///` lines above a declaration is its documentation:

```rill
/// Doubles a number.
let double (x: i32) -> i32 <computes> = x * 2;
```

Go to definition follows the import back to the header: `InitWindow` opens
`raylib.h` at its prototype, `SDL_FRect` opens `SDL_rect.h` at its `typedef`,
and `SDL_INIT_VIDEO` opens its `#define` - a Rill declaration opens its own
name instead. Find references answers from the resolver's reference table, so
a type name collects its constructor calls and its annotations (`var p: Point`,
`e as Point`) alike, and `M::name` is the same symbol as the preluded name. A
local or parameter answers the same way from either side: the result includes
its uses and its binding, and a shadowed name stays separate from the one it
hides.

`@link("NAME")` makes `exec` and `build` search the
loader's own paths (`LIBRARY_PATH`, `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`,
then the system prefixes) for `libNAME`, link it, and record an `rpath` so the
binary finds it at run time.

The demos under `examples/sdl3/` are three programs written this way - a
bouncing rectangle, a plasma drawn point by point, and a small side-scrolling
game - and `crates/rill-driver/tests/c_import.rs` builds and runs them headless
against a system SDL3.

`examples/raylib/` holds three more, against raylib: the bouncing rectangle, the
same small game, and a match-3 game whose gem sprites, board tiles and background
are generated in code at startup rather than loaded from disk. raylib marks its
prototypes `RLAPI` instead of `extern` and passes records such as `Rectangle` and
`Color` by value, both of which the import reads; the demos hide their window and
run a fixed burst of frames when `RILL_RAYLIB_DEMO` is set, which is how the
tests drive them.

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

## Formatting

`rillc fmt FILE... [--write]` prints the canonical form of each file (or, with
`--write`, replaces it). A directory argument formats every `.rill` file below
it, which is how the checked-in examples are kept canonical:

```console
$ rillc fmt examples --write
```

The formatter decides bracket adjacency from the tree, never from source
spacing: `xs[i]` stays an adjacent call while `xs [i]` stays an application to
an array literal. Parenthesised argument lists are unambiguous, so they are
canonicalised to touch the callee — `f(x)` — to match the `f[x]` form; the one
exception is a list followed by more arguments (`g (a, b) c`), where the space
is what keeps the whole spine one call.

Layout is width-aware. Settings live in a `.rill-fmt` file; the formatter
searches the formatted file's directory and its ancestors, so one file at the
repository root governs everything below it (a subdirectory can override it).
Every key is optional:

```toml
# .rill-fmt
max_width = 100              # line width; lists break one element per line
tab_width = 4                # columns a tab counts as when measuring
single_line_if_width = 80    # wider if/else chains break per block
single_line_block_width = 48 # wider blocks are laid out over several lines
blank_lines_between_items = 1
preserve_blank_lines = true  # keep one source blank line between statements
```

Defaults are deliberately generous: short `if` chains such as
`if (x) { 0 } else { 1 }` stay on one line, top-level declarations are separated
by a blank line (as are trait/impl members), and a block breaks only when its
contents no longer fit.

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
* **Global storage is explicit.** `var` is stack storage and lives inside a
  block. Module-level mutation is `static var` (one copy in the data segment)
  or `static threadlocal var` (one copy per thread); a bare `var` at module
  level and `static` inside a block are errors. Functions may reference a
  static declared later in the file - the resolver binds all module-level
  locals before bodies, so both orders name the same storage.
* **Errors are data.** Nothing in the front end panics on malformed input:
  unexpected CST shapes lower to `Error` nodes with a diagnostic, guaranteed
  progress rules keep the parser advancing, and `rillc` reports and exits.

## Development

```console
$ cargo test --workspace     # lexer, parser, diagnostics, lowering, desugar
$ cargo clippy --workspace --all-targets
$ cargo fmt --all
```
