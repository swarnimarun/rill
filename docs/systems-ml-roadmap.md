# Systems ML implementation order

This branch begins at the empty jj root. Its compiler sources were seeded from the tracked
Rust/C11 implementation at v1 revision 951c9f38. Use the old revision to compare behavior
and inspect the rationale for copied code. The independent ancestry lets us
reconsider the surface language while running the original regression corpus.
The [phase gates](../PLAN.md#19-dependency-ordered-delivery-plan) still govern acceptance.

## The first user experience

Write a braced function, omit local types, call functions with parentheses, and
bind the result of a pattern match. A value parameter transfers an owner. A
`read` parameter lends shared access during the call. An `inout` parameter lets
the caller see exactly which place may change. Keep inferred lifetimes local to
such calls. A stored view will need an expressible relationship to its origin.
For pipelines, partial application and closure capture, see the
[working function examples](../examples/functions/README.md).
The motivating precedent for projections is [Hylo’s binding model](https://hylo-lang.org/docs/user/language-tour/bindings/).

Represent absence with the bundled nominal `Option<a>` and recoverable errors
with `Result<a,e>`. Keep the same call spelling for both. Match handles every
case and makes discarded errors visible. Use the two-branch pattern
conditional for the common one-pattern case. For example, this extracts a coordinate from a lookup returning an optional position:

```rill
if let option.Some(pos) = lookup(id) { pos.x } else { 0 }
```

Evaluate the lookup once. Bind names only in the success arm. Both arms have
one result type. Ordinary effects already performed by the lookup remain
observable. Verse motivates composable failure, but rollback needs named
participants, an undo log and separate effect rules. A later postfix `?`
experiment should forward None to an Option-returning function, or an Err of
exactly the enclosing Result error type. Start with one function boundary and
no implicit error conversion. Require drop-on-exit evidence before admitting
owners. Until then, use explicit match or std.option_ops.and_then.
[Verse failure contexts](https://dev.epicgames.com/documentation/fortnite/basics-of-writing-code-9-failure-and-control-flow-in-verse)
and [Rust propagation](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-try-propagation-operator).

## Design experiments to run on the examples

| Precedent | Concrete Rill experiment | Decision test |
| --- | --- | --- |
| F# unions and local inference | Model scene attachment as `Root` or `Child(i32)` and write constructor matches without repeating field types. | Constructors constrain payloads, pattern names cannot escape the arm, and the inferred signature is readable in the editor. [F# unions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/discriminated-unions) |
| OCaml value restriction | Compare an immutable `None` used at two payload types with the same uses of a mutable slot or call result. | The checker rejects incompatible writes even through immutable aliases. [OCaml manual, chapter 6](https://ocaml.org/manual/5.0/ocaml-5.0-refman.pdf) |
| Haskell qualified polymorphism | Prototype a constraint on a generic numeric fold over database rows after ordinary rank-one inference is specified. | The generic body checks under its declared assumptions. Two competing dictionaries or an unconstrained numeric result produce an ambiguity diagnostic. [Haskell 2010, chapter 4](https://www.haskell.org/definition/haskell2010.pdf) |
| Goose reference roots | Prototype a borrowed slice of editor text. Infer the document as the origin when the result has one candidate. At a join, retain all candidate roots. | A view into the caller buffer survives a helper call. Returning a view into scratch storage and resizing borrowed storage both reject. Keep Rill's exclusive access rule in this experiment. [Goose specification, section 9](https://aardappel.github.io/goose/docs/goose_spec.html) |
| Vx memory placement | Prototype distinct host and device buffers for the existing voxel renderer, with an explicit transfer operation. | CPU code cannot read device-only bytes. The transfer records size, allocation failure and completion before its destination becomes available. [Vx memory spaces](https://vxlang.org/docs/heterogeneous.html) |

For an optional lookup, choose the form according to the caller's intended action:

| Caller wants to… | Accepted spelling or experiment | Consequence |
| --- | --- | --- |
| Inspect a single case and recover locally | `if let Some(x) = lookup(k) { use(x) } else { fallback() }` | Fallback is lazy. Both arms must type-check at one result type. |
| Transform an optional Copy value | `opt.map(lookup(k), transform)` | Returns another optional value. The callback runs only on Some. |
| Report a distinct error for each failure | Exhaustive `match` on `Result<T,E>` | The error stays available to every caller. |
| Leave the enclosing function on absence | Proposed `lookup(k)?` | Needs a checked early-exit edge, including cleanup, and an Option return context. |

## Work packages and review gates

| Order | Implementation and dependencies | Evidence needed to advance |
| --- | --- | --- |
| 1. Source contract | Extend src/lexer.rs, src/parser.rs and the lossless syntax tree together. Keep strict parse and editor recovery on the same grammar. Represent missing punctuation with a zero-width insertion at the end of the preceding token. Preserve trivia and diagnose invalid tokens in UTF-8 input. | Malformed multi-declaration files, recovery budgets, CRLF/non-BMP ranges, no AST after a syntax error, legacy corpus unchanged. |
| 2. Editor feedback, parallel with 3 | Convert source byte spans using the LSP document version and UTF-16 mapper. Offer only edits justified by a parser diagnostic. Reparse the edited document. Drive requests over framed stdio. | Quick fix round trip in the unsaved buffer, disappearance of the diagnostic, imported-file overlays and stale edits handled. |
| 3. Type oracle | Specify a small calculus with integer/bool options, immutable/weak bindings, explicit moves, reads and branches. Enumerate programs independently of the production AST and solver. Predict acceptance, result or trap first, then compare checker, evaluator and generated C. | Both false acceptance and false rejection fail the test. Record bounds, constructor rules, invalid paths, optimizer levels and total programs. Extend to generated loan paths and recursive SCCs next. |
| 4. Pattern conditional, after 1 and 3 | Elaborate the scrutinee and branch-local bindings through the existing pattern checker. Check result equality, ownership joins, evaluation order and origin paths through code generation. Update editor grammar and examples. | Exhaustive match equivalence at O0/O3, wrong-payload rejection, binder scope rejection, syntax diagnostics and tooling origins. |
| 5. Place and storage contracts | Extend the existing place tree and typed verifier with initialized bits for individual owned fields. Join by intersection of definitely initialized paths. Elaborate edge-specific drops before extending storage forms. Then introduce caller-supplied allocators and checked slices with explicit origins. | Move one field/use its sibling, reject whole reads and overlapping loans, injected OOM after partial construction, exactly-once free trace in both backends, rejection of returned local views. |
| 6. Inference and proof | Mechanize a rank-one calculus with explicit store typing and affine environments. Prove substitution/weakening for allowed zones, preservation and progress up to specified traps. Add region/kind checking and rigid variable escape checks before rank-two APIs. | Machine-checked statements with their assumptions, corresponding compiler fixtures, differential counterexamples minimized and retained. The bounded tests in step 3 are an executable evidence source. |
| 7. Libraries and performance | Use the engine to exercise frame storage, editor to drive string/slice and diagnostic APIs, and database to drive byte layout, streaming I/O, conflict handling and persistence. Add capabilities and closed effects before promising allocation-free paths across FFI. | Reproducible workloads, bytes allocated per operation, measured peak resident memory, incremental sync bytes/latency, and injected failure with state recovery. |
| 8. Advanced ML features | Start with nominal sums/products, rank-one let polymorphism and exhaustive patterns. Add coherent traits last in the rank-one solver, then effect rows and annotated higher-rank callbacks one extension at a time. Tie each extension to a real client requirement. | Reject polymorphic mutable cells, ambiguous instances, skolem escapes, effectful callbacks passed to pure code and escaped loans. Track the exact supported fragment at each gate. |

## Application milestones

* **Scene engine.** First exercise parent/child transforms, hit testing, an update loop and rendering with bounded scene storage. Next separate assets from instances, add typed events and frame allocation, then serialization and a scene editor. Measure traversal and allocations on wide and deep scenes before changing representation.
* **Modal editor.** Open a real file, edit text in normal/insert modes and save. Next add grapheme-aware movement, selections, undo groups, a piece tree and incremental LSP document synchronization. Replay scripted edits and check resulting file bytes and versioned diagnostics.
* **Embedded database.** Start with an on-disk revision log and a resumable changes cursor. Distinguish duplicate replay, forks and gaps. Then add indexed point lookup, a crash-recoverable commit protocol, compacted snapshots with cursor negotiation, and column batches for scans. Gate sync on divergent/replayed streams, reopen after fault injection and a measured large-dataset benchmark.

## Decisions from the initial review

The frontend review found an existing recovery parser and stdio server. Extending
their shared diagnostics keeps repaired syntax out of compilation. The type
review selected a finite independent oracle because rechecking with the same
unifier could reproduce its error. The database review identified the lack of
checked file I/O and the boundary between foreign operations and owned values.
Keep persistent file operations in an explicit host adapter and test it against
real files. Apply the same division to interactive input for the editor and engine.

Run the package-specific tests in each example or feature guide, then the three
repository checks in AGENTS.md. Preserve a failing input before expanding the
accepted subset. Record environmental misses and unproved cases against their
specific gate in docs/blockers/.

## Running the first increment

| Area | Entry point | Focused verification |
| --- | --- | --- |
| Frontend and editor | [Syntax tour](syntax-tour.md), [LSP and editor support](editor-support.md) | `cargo test --locked --test conditional_patterns --test syntax_tree --test lsp_protocol` |
| Inference | [Bounded models](type-models.md), [operand diagnostics](operand-diagnostics.md) | `cargo test --locked --test core_models --test operand_diagnostics` |
| Formal core | [Typing rules, erasure and proof scope](option-core-proof.md) | `lean proofs/OptionCore.lean` with Lean 3.51.0 and `cargo test --locked --test option_core_proof` |
| Kernel IR | [Checked source, CPU evaluation and LLVM lowering to CUDA and Vulkan](gpu-compilation.md) | `cargo test --locked --lib kernel_` and `cargo test --locked --test kernel_cli`, followed by the documented driver runs |
| Scene engine | [SDL3 window, terminal game and reusable scene module](../examples/scene-engine/README.md) | `cargo test --locked --test scene_engine --test scene_sdl` |
| Modal editor | [Terminal editor and save contract](../examples/modal-editor/README.md) | `cargo test --locked --test modal_editor` |
| Embedded database | [File format, replay and benchmark](../examples/embedded-db/README.md) | `cargo test --locked --test embedded_db` |

The fixed-capacity `bytes<N>` type makes document, scene and framebuffer storage
visible in function signatures. A type argument here is one decimal capacity
from 0 through 256. General constant expressions, allocator polymorphism and
resizable storage require separate rules.
