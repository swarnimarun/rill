# Next language increments for discussion

This is a proposed order within language ergonomics, not an implementation
commitment or accepted syntax. The [continuation plan](../CONTINUATION_PLAN.md)
sets the project-wide priority order, including compiler foundations and the
[performance plan](../build-performance.md). The ranking here does not supersede
those dependencies.
The [migration guide](../rust-to-rill.md) describes working Rill today; the
[feature inventory](rust-migration.md) records the broader alternatives.
The five increments below each address a concrete porting problem without
requiring Rust feature parity. None completes a phase in
[PLAN.md section 19](../../PLAN.md#19-dependency-ordered-delivery-plan).

## Shared prerequisite: explain the source obligation

Keep the current strict rejection and bounded spelling hints. Before adding
new annotation or exit forms, record their exact source roles through parsing,
module rewriting and typed lowering. Diagnostics should identify the written
annotation or exit, the affected binding/loop, and a related conflicting site.
Do not locate these sites by searching source text or comparing AST values.

The [module origin API](../module-source-origins.md) already preserves source
snapshots and resolved contributions. [Copy-body origins](../typed-copy-node-origins.md)
cover a bounded subset and explicitly exclude owner cleanup and native ABI
operations. New exits need their own checked origin relationships; an existing
Copy mapping cannot establish ownership coverage. Source-tree recovery must
continue to withhold a compilable AST when syntax is invalid.

## 1. Closed local type annotations

Decision status: recommended first among these syntax proposals, pending a
syntax and inference decision.

The [result-annotation example](../../examples/result-annotations/main.rill)
defines `fn absent() -> Option<i32> { None }`, then initializes mutable `answer`
through that helper. It works today. A local annotation would let a programmer
state that constraint beside an empty constructor, record or mutable slot
without creating a helper solely to constrain its type.

Consider only the closed type vocabulary already admitted on function
parameters/results. A proposed spelling such as `let value: Option<i32> = None;`
is currently rejected. Open type variables, holes, higher-rank schemes and casts
would be separate decisions. An annotation should constrain an initializer;
it must not invent a runtime representation for an unresolved value.

Extend the existing [annotation budgets](../recursive-signatures.md) before
cloning new trees or resolving declarations. Preserve the value restriction and
mutable monomorphism. Current [AST lets](../../src/ast.rs) carry a name, value
and body, so the change needs an explicit annotation representation and source
role rather than a parser-only discarded field.

Acceptance must cover closed primitive/record/enum types, imported nominal
types, unused bindings, mismatches and shared complexity limits. Mutable
polymorphism must still reject; open/rigid annotation variables remain outside
this closed increment. Compare
interpreter/C behavior at O0/O3, and show that errors point at the annotation
and initializer. The current alternative remains a closed function signature.

## 2. Unit exits from `while`

Decision status: valuable, design `break` and `continue` together before coding.

The [control-flow example](../../examples/rust-migration/control-flow.rill)
uses `found < 0` in the search condition and guards odd-number accumulation
inside the loop. These patterns work. In a longer parser, each later operation
also needs a guard after a match is found; overlooking one runs work that a
`break` would have skipped. A `continue` must likewise skip the suffix while
preserving loop progress.

Start with unit exits targeting the nearest enclosing `while`. Leave labels,
value-producing loops and `break value` out of this first increment. Decide
whether exits are statements or expressions and how they appear as checked
terminators. Existing nonreturning-result proofs describe computations that
cannot return; they do not prove an arbitrary jump is valid.

The current [ownership walk](../../src/check/ownership.rs) requires a loop to
preserve its enclosing ownership state. An exit must distinguish its outgoing
state from the backedge state. Owners created inside the loop must be cleaned
on the paths that leave their scope, while enclosing owners must remain valid
on every accepted path. A lexical flag rewrite alone does not establish this.

Acceptance needs nested/empty loops, outside-loop rejection, condition
re-evaluation, once-only prefix effects, skipped suffix effects and exact owner
cleanup traces. Reject malformed targets in the independent verifier. Include
loan conflicts and moved owners at backedges and exit joins, with interpreter
and optimized C agreement. Keep current flag/guard examples as the comparison.

## 3. Early function return

Decision status: follow the shared exit design; do not combine it with `?` yet.

The [CLI example](../../examples/cli/main.rill) nests option-name validation,
integer parsing and error matching inside a token match. Every successful and
failed branch produces the final function value. That is the working form
today. Guard returns would reduce indentation when several validations precede
the useful operation. Extracting a tail-expression helper remains a reasonable
alternative for short functions.

Define return targets at function boundaries, including lambdas, and constrain
every returning value against the function result. Reject exits that target an
enclosing function through a nested closure. Decide unreachable-code checking
without relaxing whole-declaration validation or inventing result storage.

Returning an owner transfers exactly one ownership obligation to the caller.
Other live locals need normal cleanup on that path, and call-scoped loans cannot
escape. Reuse the explicit exit representation from loop work, with separate
verified function targets. Do not equate an early return with a process trap.

Acceptance includes nested matches, annotated/inferred results, recursion,
side-effect order, cleanup exactly once and owner-return rejection cases.
Diagnostics should connect an incompatible return to the result constraint.
Result propagation can be proposed afterward, with explicit error conversion
and affine-payload rules. Existing Copy-only `result_ops.and_then` remains
available; importing it does not make it owner-compatible.

## 4. Token parsing over caller-owned bytes

Decision status: a useful source-library candidate, independent of new syntax.

`std.cli.classify`, `span_equal` and `parse_i32` currently take `str`, which
denotes static text. The [CLI example](../../examples/cli/main.rill) therefore
parses the supplied literal `"--count=42"`. Runtime input belongs in explicitly
owned bytes, but there is no equivalent CLI token parser for those buffers.

The existing [JSON input module](../../stdlib/json_input.rill) already parses
runtime owned bytes. Its document grammar rejects leading zeros and a leading
plus, and accepts JSON whitespace. Reusing that parser as a decimal CLI-token
parser would silently change accepted input. Today a host can parse runtime
arguments itself, or a Rill library can write an explicit bounded byte loop.

A separate monomorphic helper set could take `read SystemBytes` plus a validated
span and return ordinary Copy results. Reuse the [buffer range contract](../../stdlib/bytes.rill)
and preserve CLI sign, span, first-error and overflow behavior. Return byte
offsets or copied values; retain no reference and allocate no hidden storage.
Keep the owner-compatible module separate from generic `_ops` imports.

Acceptance should compare static-string and owned-byte parsers over identical
token bytes, including empty spans, signs, leading zeros, invalid bytes,
integer extrema and overflow. Invalid spans must reject before indexing and
must leave the input untouched. This can be reviewed without committing to
iterators, methods, growable strings or process APIs.

## 5. Bounded hosted process arguments

Decision status: defer implementation until the host boundary is agreed.

`rill new` creates a runnable native project, while `std.cli` only understands
tokens a caller supplies. That leaves a concrete gap for a standalone counting
or conversion utility. The current alternative is a host wrapper or explicit
C bindings. Raw foreign pointers still have no checked Rill lifetime or general
dereference API; declaring an `argv` pointer is not sufficient to expose safe
argument strings.

Start by considering argument count and copying one argument into a
caller-owned buffer with an explicit capacity error. This avoids requiring a
growable Vec/String for the first API. It still needs decisions about entry
capabilities, count/length overflow, buffer mutation on failure, argument-zero
conventions and whether input is captured once or queried repeatedly.

Specify Unix byte preservation and Windows argument/encoding behavior
separately. Do not silently replace invalid data or call foreign memory static
text. The source interpreter needs an explicit supplied-argument environment,
or must reject reached argument operations with a clear diagnostic. The native
entry wrapper must validate and copy through a checked intrinsic boundary.

Acceptance needs real launched-process tests for no/empty arguments, spaces,
option terminators, invalid indices, exact/insufficient capacities and length
overflow. Failed copies must follow the agreed mutation contract. Exercise each
claimed OS, compare parsing with the byte helpers above, and account for every
allocation and cleanup. File I/O, environment variables and process spawning
would remain separate capabilities.

## Discussion before implementation

Choose the smallest current program whose awkwardness justifies a change.
Agree its accepted and rejected forms, written-source diagnostics, ownership
rules and independent verification before adding parser branches. Prioritize
the closed annotation increment and runtime-byte library work for bounded
changes; design the two control-flow increments together. Keep hosted OS input
behind its encoding and resource decisions. Traits, macros, async and broad
Rust spelling compatibility do not need to block these decisions.
