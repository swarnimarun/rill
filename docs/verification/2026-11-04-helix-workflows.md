# Helix workflow verification

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](2026-11-04-ernie-integration.md).

This increment improves the existing compiler-backed LSP and Tree-sitter
queries for normal Helix editing, including incomplete source. It does not
complete a PLAN.md phase or change the callable resource boundaries described
in [functions](../functions.md).

## LSP changes

- Callable, local and parameter completion/hover retain inferred types and
  shared type-variable names. Definition navigation follows function values in
  whitespace application and operator expressions.
- Record completion follows checked nominal identities, including generic
  nested records and callback fields. Unsaved modules and visibility retain
  the ordinary compiler semantics. Raw foreign pointers do not gain guessed
  record fields.
- Recovery preserves completed bindings in nested blocks and lambdas while an
  identifier, member or call is incomplete. Partial field names also recover
  when parsing succeeds but checking fails.
- Incomplete callback and immediately curried/returned/anonymous calls use a
  checked complete callee. Whitespace application does not steal signature
  help from a later parenthesized call.
- Discarded unfinished bindings receive no fabricated type. For example,
  `let outer = 1; let value = val` retains `outer: i32` without inventing
  `value: unit`. Grouped local callables do not borrow parameter names from a
  shadowed global function.

The final affected LSP run passes 52 tests: 14 analysis, 10 hint/signature,
18 workflow regressions and 10 framed stdio protocol tests. Independent review
reproduced and rechecked the nested callback, scope, currying, unfinished
binding and shadowing cases, with no remaining actionable finding.

## Syntax queries and real editor workflows

Generic local-reference queries now capture expression names instead of every
identifier. Field labels, type names and module members keep their colors when
a parameter has the same spelling. Helix adds lambda function textobjects and
continuation indentation for pipelines, application and multiline local
values. Inside-function selections include the body block, following Helix's
bundled Rust convention.

Tree-sitter verification passes 11 corpus cases and 51 repository sources,
generated ABI-15 parser reproducibility, highlight/locals/tag queries, actual
highlight classes, incremental edits, reserved/contextual names, rejected
forms and recovery. New query tests check capture ranges, nested lexical
scopes and negative field/type/module color collisions. No grammar production
change was needed for these query corrections.

The real Helix 25.07.1 workflow test uses the rebuilt release compiler, an
isolated copy of the user's settings and private temporary source files. It
verifies matching LSP response IDs and decoded terminal output for:

- Definition jumps through generic identity, curried functions, a pipeline
  local and an imported function, including actual target buffers/ranges.
- Typed hover, symbol-picker filtering and acceptance, and completion
  acceptance for locals, imported functions and nested record fields. Typing
  `state.inner.va` offers `value: i32` and accepts `state.inner.value`.
- LSP formatting applied to an unsaved buffer and five functional syntax
  highlight scopes.
- Exact `maf`/`mif` selections for named functions and empty/nested lambdas,
  followed by undo restoring the original buffer.
- Pure Tree-sitter indentation: eight spaces for pipeline continuations and
  split local values, twelve for a multiline lambda body.

Fixture and configuration bytes stay unchanged, no `didSave` is sent, and
both editor and server exit cleanly. A separate run with default Helix settings
verifies the default space-then-`k` hover binding; the copied user configuration
uses `K`.

The final workflow proof is recorded locally in
`/tmp/rill-hx-polish-final-workflow-proof/summary.json`. The headless Neovim
0.11.5 smoke test also passes with the current release compiler and shared
queries. Seven installer tests, five terminal decoder tests and two workflow
helper tests pass.

Three fresh smoke runs, including an independent run after installation,
verify actual SDL3 buffer diagnostics and clearing, seven inferred hints with
rendered `answer: i32`, and signature help advancing from parameter 0 to 1.
The popup's parameter emphasis changes too. The tests verify source/config
preservation and clean editor/server shutdown. The independent proof is in
`/tmp/rill-hx-polish-installed-final-smoke/summary.json`; two simultaneous runs
are in `/tmp/rill-hx-polish-frozen-smoke-proof4/` and
`/tmp/rill-hx-polish-frozen-smoke-proof5/`.

The smoke test exposed a Helix 25.07.1 idle-timer race when opening a buffer
through the command prompt. If the timer expired during the prompt, the client
sent no hint request. An acknowledged normal cursor motion rearms the timer.
A forced-expiry reproduction failed before this test correction and passed
afterward. Both editor helpers also recheck their completion predicate when
observing process exit, avoiding a race during successful shutdown. No server
response or terminal render assertion was weakened.

## Installed configuration

The reviewed stage and installed `hx --health rill` use the release compiler
directly. Installation refreshes only Rill's language entry, parser and four
query files, with dated backups. The installed query bytes and ABI-15 language
metadata match the reviewed stage. The user's `config.toml`, all parsed
language settings and existing compiler symlink remain unchanged.
The backup suffix for this refresh is `20261104T140335773744Z`.
Existing Helix sessions must restart to load the new server and queries.

## Compiler and native verification

The release build, `cargo fmt --all -- --check` and strict all-target Clippy
with both default and `c-import` features pass. The separate feature-enabled
affected set passes 343 tests across 15 targets with zero failures. It covers
the library/CLI, callables, all LSP tests, module overlays, selected C import,
SDL3 framebuffer comparison and exact lifecycle traces. Real direct,
generated-binding and Zig SDL3 executions pass against installed dependencies.
Native importer checks set the macOS SDK root explicitly.

The complete default inventory uses Cargo's
`test --locked --no-run --message-format=json`, then runs every discovered
test executable with two test threads and three targets in parallel, followed
by doc tests. Separate target directories keep ordinary and `c-import` builds
from replacing binaries used by concurrent tests.
The final inventory passes 686 tests across all 99 targets with zero failures;
doc tests also pass.

Two default invalid-byte path subcases and one feature-enabled path subcase
retain explicit filesystem/sandbox capability skips. Separately developed
voxel GPU tests report native GPU unavailable and the opt-in 256-cubed capacity
skip. These skips do not stand in for SDL3 evidence; the direct/generated/Zig
SDL3 checks execute successfully.

Local logs include `/tmp/rill-hx-polish-frozen-full-default.log`,
`/tmp/rill-hx-polish-frozen-feature-tests.log`,
`/tmp/rill-hx-polish-frozen-clippy.log`,
`/tmp/rill-hx-polish-frozen-clippy-import.log`,
`/tmp/rill-hx-polish-final-tree-sitter.log` and
`/tmp/rill-hx-polish-frozen-neovim.log`.

## Scope and remaining limits

The server continues to advertise only its implemented requests. References,
rename, code actions, debugger mapping and incremental semantic caching remain
open. Recovery only offers types obtained from checked source; it can omit
results when a complete callee or enclosing context cannot be checked.
Tree-sitter handles syntax and editor structure, while the compiler determines
types, ownership, visibility and accepted programs. Editing never executes
native dependency tools or SDL3.
