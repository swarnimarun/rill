# Tree-sitter for Rill

This package parses and highlights the compiler's current experimental syntax:
functions and blocks, closed function result annotations, semicolon statements, records, generic algebraic types,
matching, imports/exports, native declarations, ownership/loan spellings,
`fn(x) { x }` lambdas, calls through expressions, whitespace application,
`|>`/`<|` pipelines, `>>`/`<<` composition, `not`/`!`, braced `if` with optional
`else`, and the legacy `let ... in`, `then` and `do` forms. It is an editor
parser. The Rust compiler still decides type, ownership, ABI, module ordering
and resource-limit validity. Proposed syntax in `PLAN.md` is not implemented
here as a future language dialect. A braced `if` without `else` has unit type;
the compiler requires its then-branch to return unit. The editor parser can
parse type-invalid branches while they are being edited. Legacy `if ... then`
still requires `else`.

Braced named functions accept `fn answer() -> i32 { 42 }` and qualified or
generic result types. Result annotations are not part of legacy `let`
declarations or lambdas. The compiler checks whether an annotation names an
accepted closed type and agrees with the function body.

The generated parser exports `tree_sitter_rill` and uses language ABI 15, which
provides the compiler's global reserved-keyword behavior. Only the lexer's
20 keywords are reserved; contextual words such as `import`, `as` and `ptr`
remain identifiers where the compiler permits them. Direct named calls allow
one module alias segment; generic postfix calls can use field paths or other
expressions as their callee. There is no external scanner. Queries use conventional Tree-sitter capture names:
`queries/highlights.scm`, `queries/locals.scm` and `queries/tags.scm`.

Whitespace arguments preserve the compiler's adjacency rule. `f x(y)` passes
the result of `x(y)` to `f`, while `f x (y)` passes two arguments. A first call
still accepts `f (x)`, and postfix calls can chain as `f(1)(2)`. Pipelines bind
below `||`; composition binds between `&&` and equality. `<|` associates to the
right, and the other function operators associate to the left. Composition's
angle brackets must be adjacent; nested generic type closers still parse as
separate brackets. Successful syntax parsing can include type errors such as
`1 2`. The compiler decides whether a value is callable.

## Build and use

The repository's [Helix configuration](../helix/README.md) provides the primary
editor setup. A [Neovim configuration](../neovim/README.md) also loads the parser,
queries and `rill lsp` together. For another Tree-sitter consumer, compile the
parser as a shared library and load it as language `rill`:

```sh
cd editors/tree-sitter-rill
cc -std=c11 -shared -fPIC -I src src/parser.c -o /tmp/rill.so
```

On macOS the shared-library extension may also be `.dylib`. The consumer must
support language ABI 15. Configure `.rill` files with scope `source.rill` and
load `queries/highlights.scm`. Loading a grammar/query is editor-specific;
this package does not install or modify an editor's global configuration.

The Tree-sitter CLI can render the actual highlighted game:

```sh
cd editors/tree-sitter-rill
tree-sitter highlight --grammar-path . --html ../../examples/sdl3-direct/main.rill > /tmp/rill-sdl3.html
```

## Develop and verify

Generation was exercised with Tree-sitter CLI 0.27.1 and Node 26.8.1. Parser
consumers need only the generated C and headers; Node and the CLI are development
tools. Regenerate after editing `grammar.js`:

```sh
cd editors/tree-sitter-rill
tree-sitter generate --abi 15
python3 scripts/verify.py --check-generated
```

The generated `src/parser.c`, `src/grammar.json`, `src/node-types.json` and
`src/tree_sitter/*.h` are intentionally part of the grammar package. They allow
editors to compile the parser without a JavaScript toolchain or generator.
Shared libraries, caches and temporary highlight output are not source files.
When upgrading the generator, review the generated-source diff along with the
grammar change.

`python3 scripts/verify.py` needs Python 3, the Tree-sitter CLI and a C compiler.
It uses a temporary cache and a parser configuration limited to this checkout,
so an installed grammar from another checkout cannot satisfy its checks. It
runs the corpus, parses every repository example, bundled library and Rill
fixture, compiles all queries, checks real HTML highlight classes, compares an
incremental edit with a fresh parse, rejects representative unsupported forms,
and checks recovery into a later declaration after malformed input. The
`--check-generated` option also regenerates into a temporary directory and
requires byte-identical generated sources, using the documented generator version.

Corpus expectations include exact precedence trees; changes to parsing must be
reviewed rather than accepted merely because an input has no error nodes.
Neither editor error recovery nor successful highlighting proves a program
passes compiler checking or executes safely.

The grammar and query format follow the official [grammar DSL](https://tree-sitter.github.io/tree-sitter/creating-parsers/2-the-grammar-dsl.html)
and [highlighting documentation](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html).
