# Functions and editor hints implementation plan

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](../../verification/2026-11-04-ernie-integration.md).

> For agentic workers: use the delegate workflow with independent file ownership and review each delivered artifact.

Goal: implement inferred first-class functions, Copy closures, currying and partial application, application/composition operators, inlay hints and signature help across the existing compiler and editors.

Architecture: check source callables with the existing rank-one solver, retaining each concrete closure environment in its type. After checking, specialize and convert callables into ordinary nominal captures and direct calls, then independently check the converted program before typed execution and C emission. The independent source reference evaluator executes the original callable AST. Editor queries consume source types and origins before conversion.

Tech stack: Rust compiler, existing C11 backend and checked typed evaluator, stdio LSP, Tree-sitter ABI 15 and Helix.

Spec: user request following the editor checkpoint, PLAN.md sections 3, 5, 8, 16 and 19, and current ownership/native boundaries in AGENTS.md.

## Constraints

- Preserve all existing changes, including the separately developed voxel example.
- Keep braces and semicolons primary and existing legacy declarations accepted.
- Preserve left-to-right evaluation, once-only capture/partial-argument evaluation and existing observable resource behavior.
- Store concrete Copy captures inline. No hidden heap boxing or dynamic erasure.
- Diagnose unsupported owner/borrow captures and incompatible concrete closure representations. Do not silently copy an owner or extend a loan.
- Recheck converted owned programs; never use expression-pointer metadata after cloning or rewriting an AST.
- No native tools or source writes during editor analysis.

## Architecture phases

- [x] Ground current named-call AST, solver, module rewriting, source origins, checked lowering and LSP query flow.
- [x] Compare two complete designs: extend every runtime/typed representation with callables, or check callables then convert them into existing records/direct calls. Record the selected contracts.
- [x] Freeze AST, callable type, conversion and editor interfaces before their consumers change.
- [x] Implement against those contracts with separate frontend, semantics/conversion and editor ownership.
- [x] Revisit the design if repeated workarounds violate the contracts; retain only the implementation that passes acceptance evidence.

## Interfaces

```rust
// Source AST, before callable conversion.
Expr::Lambda(Vec<Param>, Box<Expr>)
Expr::Apply(Box<Expr>, Vec<Expr>)
Expr::FunctionOp(FunctionOp, Box<Expr>, Box<Expr>)
// FunctionOp: PipeForward, PipeBackward, ComposeForward, ComposeBackward.

// Checked source callable: signature plus concrete inline environment identity.
Type::Callable {
    environment: Box<Type>,
    params: Vec<Type>,
    result: Box<Type>,
    modes: Vec<ParamMode>,
}

// Own the converted AST, then use fresh checking metadata for execution.
pub fn functions::needs_lowering(program: &Program) -> bool;
pub fn functions::lower(program: &Program) -> Result<Program, String>;

pub fn analysis::inlay_hints(uri: &str, source: &str,
    bytes: Range<usize>, overlays: &BTreeMap<String,String>) -> Vec<Value>;
pub fn analysis::signature_help(uri: &str, source: &str,
    byte: usize, overlays: &BTreeMap<String,String>) -> Option<Value>;
```

## Implementation tasks

- [x] Frontend: `fn(x,y){...}`, generic postfix and whitespace application, `|>`, `<|`, `>>`, `<<`, precedence and nested generic delimiters; source-map paths and formatter parity. Test malformed inputs and unchanged old syntax.
- [x] Semantics: callable inference, function references, lambda lexical scopes/captures, higher-order generic specialization, under/over application, occurrence checks and value restriction; function dependencies in SCC order and module identity/privacy.
- [x] Conversion: evaluate captures and bound arguments once; emit concrete environment declarations and direct wrapper calls; preserve call modes, recursion, returned closures and argument order. Independently recheck converted programs.
- [x] Execution: route functional programs through conversion for interpreter, reference oracle and C generation; retain the checked owner path and reject unsupported capture forms explicitly.
- [x] Editor: inferred local/parameter/result hints and argument-name hints, range filtering, typed signature help with active argument and incomplete-call recovery; unsaved imports and local shadowing tests through actual framed stdio.
- [x] Highlighting: grammar, captures, corpus and generated parser for the new syntax; real Helix highlighting and LSP hint/signature behavior, preserving other user settings with backups.
- [x] Examples/docs: runnable higher-order, closure/currying and operator examples, exact inferred types and supported boundaries; update editor feature lists and phase status without claiming full language completion.
- [x] Verify: positive and negative inference tests; exact effect/capture traces; interpreter/reference/C agreement at `-O0`/`-O2`; owner/borrow rejections; full regression suite, real SDL3, formatter, strict Clippy, Tree-sitter and actual editor smoke tests; independent final review.

## Acceptance examples

```rill
fn add(x, y) { x + y }
fn twice(f, x) { f(f(x)) }
fn make_adder(x) { fn(y) { x + y } }
fn main() {
    let increment = add(1);
    let add_ten = make_adder(10);
    let pipeline = increment >> add_ten;
    twice(pipeline, 10)
}
```

Check that this returns 32, that `10 |> increment |> add_ten` returns 21,
and that `add 20 22` and `add(20)(22)` both return 42. Test fresh polymorphic
identity lambdas at both bool and i32, captured values after their defining
function returns, shadowed function names and imports, and capture side effects.

## Architecture decision and review findings

The chosen design adds callable structure to the existing checker and solver, then converts checked concrete environments into existing records/direct calls. A separate higher-order preprocessor was rejected because it would duplicate inference and could accept source declarations only after specializing away errors. Extending every runtime with a new dynamic callable object would duplicate layout/ABI work and introduce an unnecessary dispatch representation.

Logical signatures are curried unary chains, with zero-argument functions retaining an explicit invocation layer. Source declarations keep their natural arity, so fully supplied calls execute one body. Physical environment identity is distinct from callable signature identity; this permits generalized aggregate fields to share one once-evaluated environment at bool and integer call sites.

Review found and regression tests addressed lexical capture shadowing, erased effects in unused callable initializers, constructor precedence, private types in callable interfaces, two misleading type displays, generic aggregate environment identity, and the CLI record-field/module-call boundary. The source oracle does not reuse conversion metadata. The unchanged 2 MiB near-limit source-map regression required a small recursive inference dispatcher and expression helpers rather than a larger stack.

## Final acceptance evidence

All listed tasks have current verification in [the report](../../verification/2026-11-03-functions-and-hints.md). The final complete default run passed 666 tests across 98 targets and doc tests; feature-enabled affected checks passed 323 tests. Strict formatting/Clippy, independent source/C comparisons, real SDL3, Tree-sitter and real Helix hints/signature smoke tests passed. Helix parser/queries were refreshed with backups and existing user settings preserved. No language phase is declared complete.
