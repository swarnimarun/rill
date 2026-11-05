# Rust migration feature proposals

These are proposals for discussion, not accepted syntax or completed phases.
The [migration guide](../rust-to-rill.md) documents today's alternatives. Prefer
features that remove repeated work in real programs while retaining checked
ownership, traps, evaluation order and independently verified lowering.

## Control flow

`break`, `continue` and early `return` would simplify scans, parsers and error
handling. Boolean loop conditions and guarded bodies cover short loops today,
but become difficult to maintain when many operations follow a stop condition.
These additions have value; they should share an explicit control-flow design
rather than arrive as unrelated parser shortcuts.

| Proposal | Benefit | Required decisions and acceptance evidence |
| --- | --- | --- |
| Unit `break` in `while` | Stop at the matching item without nested guards | Identify nearest loop; reject outside loops; model cleanup and loan release on every exit; compare interpreter and O0/O3 C traces |
| `continue` in `while` | Skip the remaining work in a scan | Preserve condition re-evaluation and prefix effects; handle owner state at the loop backedge; test nested loops and evaluation order |
| Early `return value` | Guard clauses and error exits | Check result constraints, owner transfer and cleanup exactly once; distinguish statement exits from nonreturning computation proofs |
| Value-producing `loop`/`break value` | Express search results without sentinel variables | Join exit types and ownership states; define zero-exit/nonreturning loops and layoutless results; verify source/control correspondence |
| Labels | Exit nested scans clearly | Define scope and target resolution; reject jumps across functions/closures; test all cleanup paths |

Start a future implementation with unit exits from structured loops. Decide
whether exit forms are statements or expressions and whether the checked IR
needs explicit terminators. Existing nonreturning-result proofs are not a
substitute for a verified source exit. Do not silently lower an exit to a flag
if later statements, borrows or destructors would still execute.

Acceptance cases must include owners created before/inside a loop, moves on
one or multiple branches, disjoint/overlapping loans, nested loops, empty loops,
reachable traps, failed verification of malformed exit targets and exact cleanup
traces in optimized C. Benchmarks should measure real scans after correctness.

## Other useful increments

| Area | Value for a Rust port | Current alternative | Prerequisites for implementation |
| --- | --- | --- | --- |
| Local type annotations | Smaller inference surprises and better errors | Annotate closed function parameters/results | Share annotation budgets, kinds and rigid-variable checks; preserve value restriction |
| Result propagation | Remove repetitive match nesting | Explicit Result matches or Copy-only `and_then` | Early-exit semantics, error conversion policy and affine payload transfer |
| `for` and iterators | Readable traversal and reusable adapters | Counter `while` loops and `std.range` cursors | Protocol/coherence design, borrowing iteration, exhaustion and no hidden allocation |
| Growable Vec/String | Practical file, CLI and text tools | Explicit owned bytes with bounded capacities | Allocator/capacity/overflow contracts, reallocation failure, moves and borrowed views |
| Process arguments and file I/O | Standalone utilities | Host-supplied static tokens and explicit C bindings | OS capability/error model, owned runtime strings, foreign resource cleanup |
| Methods and field shorthand | Familiar data-oriented APIs | Ordinary functions with explicit record arguments | Name resolution, call modes, ambiguity diagnostics and coherent desugaring |
| Traits and associated types | Reusable generic interfaces | Inferred generic functions, concrete callbacks and enum matches | Coherence, scheme/effect contracts, specialization and representation decisions |
| Dynamic dispatch | Heterogeneous implementations | Tagged enums or concrete callable environments | Explicit storage/cost, vtable lifetime, owner/drop and reload rules |
| Const generics and comptime | Reusable layouts and generated kernels | Supported compiler specialization and literal capacities | Bounded evaluation, deterministic effects, target layout and checked expansion |
| Hygienic macros | Reduce repeated source without capture bugs | Small functions and external source generation followed by checking | Source provenance, hygiene, expansion budgets and diagnostic origins |
| Wider numeric operations | Systems and graphics calculations | Checked i32, transport/conversion types and explicit C calls | Overflow/conversion semantics and differential scalar/SIMD tests |
| Slices and stored references | Borrowed data APIs without copying | Call-scoped `read`/`inout` | Region/lifetime relationships, projection provenance and escape checks |
| Owned record fields | Normal resource aggregates | Supported owner-bearing enums and separate owners | Initialization/drop trees, partial moves and cleanup verification |
| Concurrency and async | Responsive tools and parallel work | External host coordination | Effects, thread/alias safety, cancellation and explicit runtime/storage costs |

There is no commitment to implement all Rust features or use identical spellings.
For each candidate, bring a small real Rill program, the current verbose form,
the proposed shorter form and the exact semantic difference. Prioritize a
bounded independently testable change over a broad compatibility claim.

## Diagnostics work

Contextual rejection help and bounded spelling suggestions can help immediately
without expanding semantics. Keep suggestions advisory, deterministic and limited
to accessible names in the relevant namespace. A private declaration must not
appear in another module's hint. Ambiguous near matches should not choose an
arbitrary winner. Source recovery must still withhold a compilable AST on errors.

Next useful diagnostic work includes complete semantic source spans, related
locations for borrow conflicts and duplicate declarations, clearer inferred
types, missing-import suggestions from an explicit public catalogue and safe
editor fixes. Automated fixes need exact source ranges and a validation pass;
a plausible string replacement alone is insufficient.
