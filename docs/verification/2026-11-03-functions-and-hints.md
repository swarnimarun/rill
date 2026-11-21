# Functions and editor hints verification

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](2026-11-04-ernie-integration.md).

This increment implements first-class Rill function values, anonymous functions,
immutable Copy closures, higher-order calls, currying/partial application,
whitespace and expression application, pipelines/composition, inferred inlay
hints and signature help. It does not complete the language or tooling gates in
PLAN.md section 19. See [syntax and boundaries](../functions.md).

## Architecture and semantics

The existing rank-one solver now traverses callable signatures and concrete
environments in unification, free-variable discovery, occurs checks,
substitution, level lowering and bounded materialization. Named function value
references participate in declaration dependency ordering.

Logical types use curried unary chains, preserving explicit nullary invocation
layers. Source declarations retain their natural arity. Checked conversion
specializes reachable functions, stores capture values in ordinary inline
records, and emits direct calls. It rechecks the owned converted AST rather
than reusing expression-pointer metadata after a rewrite. Physical environment
identity is separate from logical callable signature, so generalized aggregate
values can retain one environment across bool/integer uses.

The source reference evaluator executes the original callable AST independently
of conversion. It snapshots capture values and applies supplied arguments in
source order. Converted typed execution and C output are compared against that
oracle at `-O0` and `-O2`. Captures, partial arguments and composition operands
execute once at construction; unused initializer effects remain observable.

Module resolution preserves imported symbol identity/privacy, lambda parameter
scopes and annotations, local function bindings and qualified function values.
Actual imported aliases retain their existing qualified-call priority; ordinary
local record callback fields use indirect application, including in CLI loads.

## Focused evidence

- `tests/functions.rs`: 27 differential cases across source reference,
  converted execution and C at both optimization levels. Generated C fixtures
  assert that callable construction adds no allocator calls.
- `tests/function_reference.rs`: 7 source-oracle cases, including call-depth
  parity between ordinary named calls and calls through named values.
- `tests/function_inference.rs`: 6 cases covering higher-order inference,
  polymorphism, infinite types, invalid application, capture/mode/ABI rejection,
  constructor resolution and callable-containing main results.
- `tests/function_modules.rs`: 4 cases covering returned callable privacy,
  opaque private capture representations, mapped lambda/import origins and
  actual CLI record-field calls.
- `tests/function_syntax.rs`: 6 cases covering CST/origin coverage, precedence,
  lambda syntax, expression calls, whitespace adjacency, formatting and bounds.
- Callable solver regression covers environment/parameter/result traversal,
  substitution, occurrence checks and parameter-convention mismatches.
- The unchanged 2 MiB source-mapping/rewriting regression passes. A small
  recursive inference dispatcher and expression helpers avoid retaining the
  large general match frame on deep binary and let chains; no stack threshold
  was raised.

The [runnable example](../../examples/functions/README.md) checks, formats and
returns `42` through the interpreter and an optimized native build.

## Editor evidence

The compiler-backed LSP provides local/parameter/result type hints, argument
names and signature help. It preserves type-variable relationships, nullary
function-return layers, UTF-16 positions, lexical shadowing, open overlays and
entry-root context. Incomplete calls use a checked declaration snapshot; source
tokens locate the call and active argument without inventing inferred types.

The final focused LSP run passes 32 tests: 14 analysis, 10 hint/signature and
8 actual framed stdio cases. These include incomplete let initializers and
imported helpers, fresh polymorphic instances, explicit annotation omission,
range/parameter errors and disabled native compiler execution.

Real Helix 25.07.1, with a copy of the user's configuration and the release
compiler, passed the extended PTY smoke test:

- A matching `textDocument/inlayHint` response contained 7 hints; the terminal
  rendered `answer: i32`, absent from the fixture's source text.
- Typing `(` and `,` generated signature requests with active parameter 0 then
  1. The decoded terminal showed `fn add(x: i32, y: i32) -> i32`, and the
  parameter highlight background moved from `x` to `y`.
- Existing parser/query health, five rendered highlight scopes, live diagnostic
  recovery and clean server shutdown checks passed. Actual SDL3 source bytes,
  fixture bytes and configuration files remained unchanged.
- Seven setup/backup tests and five terminal-screen decoder tests passed.

The final proof includes response IDs, raw terminal/proxy trace, decoded screens
and parameter styles under `/tmp/rill-functions-helix-final-proof/`. The updated
parser/queries were installed with dated backups. Installed `hx --health rill`
passes, ABI-15 language metadata and queries match the reviewed stage, and the user's language settings,
`config.toml` and existing compiler symlink remain unchanged. The user's inlay
and automatic signature preferences were already enabled.

Tree-sitter verification passed 11 exact corpus cases and 51 repository
sources, generated ABI-15 reproducibility, real highlight/locals/tag queries,
incremental negation/application edits, reserved/contextual names, rejected
forms and error recovery. Actual Neovim 0.11.5 passed parser loading, incremental
recovery and LSP diagnostics/navigation/shutdown. VS Code's real TextMate/client
tests passed all 10 cases and a VSIX was packaged; no VS Code GUI host was
available. These editor packages share accepted source syntax without making
highlighting a type or ownership proof.

## Full and native verification

Final full/default verification passed 666 tests across 98 test targets with
zero failures; doc tests passed. The separate feature-enabled affected set
passed 323 tests across 14 targets with zero failures.

The complete default inventory uses Cargo's `--no-run --message-format=json`,
then all discovered test executables with two threads each and three targets in
parallel, followed by doc tests. Separate target directories prevent ordinary
and `c-import` builds from replacing the binaries used by concurrent tests.
Native importer checks set the macOS SDK root explicitly.

The feature-enabled affected set includes library/CLI tests, callable and LSP
checks, overlays, selected C import, real SDL3 direct/generated/Zig execution,
framebuffer capture and exact cleanup traces. The full direct SDL3 game was
built with the release compiler at `-O2` and completed its dummy-video round,
printing `0` with empty runtime stderr.

Formatting and strict all-target Clippy with default and `c-import` features
passed after integration, as did parser generation and editor checks. Two
default-test invalid-byte filename subcases and one feature-test filename
subcase retain the filesystem/sandbox capability skips. Separately developed
voxel GPU tests reported a native-GPU skip and an opt-in 256-cubed capacity
skip. SDL3 direct/generated/Zig checks ran against installed dependencies.

Local artifacts include `/tmp/rill-functions-final-full-default.log`,
`/tmp/rill-functions-feature-tests-final.log`,
`/tmp/rill-functions-clippy-final.log`,
`/tmp/rill-functions-clippy-import-final.log`,
`/tmp/rill-functions-tree-sitter.log` and
`/tmp/rill-functions-helix-final-proof/summary.json`.

## Independent review and corrected failures

Independent review produced executable reproductions for capture shadowing,
effects erased from unused weak callable bindings, constructor/local call
precedence, private types escaping through callable parameters/results,
generic lambda result labels, and nullary return layers missing from signature
help. A subsequent pass found false constructor captures and generalized
aggregate environment mismatches. Each has a regression test and was rechecked
against the source oracle, converted/native execution or actual LSP output.
The final reviewer rechecked the corrected cases through actual CLI/module and
native paths and reported no remaining actionable finding.

The first full run also exposed historical parser assumptions about whitespace
application and a changed excess-argument diagnostic. Fixtures now test actual
missing statement delimiters, and non-callable results receiving additional
arguments retain `E-ARITY`. Plain non-function application remains `E-TYPE`.

## Remaining representation and resource boundaries

Callables currently capture immutable Copy values. Owner/borrow/mutable and
polymorphic-local captures, borrow-mode function values, builtin/foreign
function values and joins of different concrete environments retain explicit
diagnostics. Functional programs containing checked owners cannot use the
copying fallback. See the [resource obligations](../blockers/function-resources.md).
These restrictions do not remove existing direct calls, native SDL3 support,
rank-one inference or checked owner execution for its existing subset.

`main` rejects callable-containing output instead of printing synthetic capture
records. General dynamic erasure, affine closure call capabilities, effect-row
inference, debugger mappings, rename/references and incremental semantic caches
remain separate work. No phase-completion or stable-language claim is made.
