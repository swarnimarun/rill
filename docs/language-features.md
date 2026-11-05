# Language features and program development

Rill is an experimental native language implementation. The Rust compiler
checks source and emits C11; the selected C compiler builds the executable.
This page describes accepted Rill features and the work still needed for the
requested systems programming, polymorphism and metaprogramming features.
It does not establish feature parity with another language. Completion still
uses the gates in [PLAN.md section 19](../PLAN.md#19-dependency-ordered-delivery-plan).

## Current feature boundary

| Area | Available | Remaining work |
| --- | --- | --- |
| Syntax and inference | Braced functions and blocks, semicolon statements, expressions, mutable locals, loops, rank-one inference and conservative let generalization | Local annotations, kinds, rigid variables and broader annotated interfaces |
| Records | Nominal finite Copy records, ordered construction, compact C structs, field reads and assignment, root and nested field loans | Owned records, partial moves, user representation contracts and layout queries |
| Enums | Generic algebraic types, exhaustive nested/literal patterns, compact tagged unions, tag-only nullary enums and checked phantom products | Recursive owning containers, GADTs and general existential packages |
| Static polymorphism | Generic functions and types, concrete reachable specialization, immutable local value schemes and specialized callbacks | Traits, coherent implementations, associated types, higher-rank APIs and const generics |
| Functions | Named function values, lambdas, immutable Copy captures, currying, pipelines and composition | Mutable/owned/borrowed captures, affine call capabilities and explicit dynamic erasure |
| Numbers and vectors | Checked `i32` arithmetic, booleans, explicit `u8x16` operations, portable scalar C and selected AArch64 NEON operations | General integer/float arithmetic, explicit wrapping/saturating operations, target-sized integers and broader SIMD |
| Memory and borrowing | Hosted owned `i32` boxes and fixed-length owned bytes, recoverable allocation, restricted moves/returns, owner-bearing enums, call-scoped `read`/`inout` and disjoint field loans | Custom allocators, growable vectors/strings, borrowed slices, stored/returned references, general initialization and cleanup |
| C interop | Target-header checks, automatic referenced-header imports, explicit externs, scalar calls/constants, exact nullable raw pointer transport and experimental explicit i32 C exports | Pointer dereference/arithmetic, aggregate ABI adapters, callbacks, wider/stable C exports and checked foreign lifetime/resource contracts |
| Modules and libraries | Local file modules, explicit exports, 15 bundled modules covering checked math, Option/Result, text, buffers, binary data, ranges, allocation, JSON, CLI and reload, local project manifests and reusable native bindings | Growable containers/runtime strings, OS services, dependency packages, opaque interfaces, canonical interface serialization and stable ABI versions |
| Metaprogramming | Bounded compiler specialization and generated C declarations from supported header inputs | Source-level constant evaluation, hygienic macros, reflection, checked builders and bounded `comptime` |
| Effects and concurrency | Explicit supported allocation and mutation operations | Effect checking, transitive allocation policies, tasks, atomics, thread safety, cancellation, transactions and search |

Unsigned and floating values currently serve native transport, explicit
conversion from `i32` and same-type equality. The `u8`/`u32`/`u64`/`f32`
types do not yet provide a general arithmetic vocabulary. Raw pointers have
no checked Rill lifetime, dereference or automatic foreign-resource cleanup.
See [C interop](c-interop.md) before extending that boundary.

Function values retain their concrete environment identity. Two different
closure environments cannot join through implicit runtime erasure merely
because their parameter/result types match. Owner programs and function-value
conversion also have different supported subsets. See
[functions](functions.md) and [resource-capturing functions](blockers/function-resources.md).

Normal scope exit cleans up supported live owners. Aborting traps, including
allocation failure in an `_abort` operation, do not promise cleanup. Recoverable
allocation returns the ordinary affine `std.result.Result` type; it does not
add general unwinding. See [the library APIs](../stdlib/README.md).

## Build different programs

From the repository root, use the same checked source for native execution,
an executable, or supported evaluator execution:

```sh
cargo run --locked -- check examples/functions/main.rill
cargo run --locked -- run examples/functions/main.rill
cargo run --locked -- build examples/functions/main.rill -o /tmp/rill-functions
/tmp/rill-functions
cargo run --locked -- run examples/functions/main.rill --interpret
```

These commands use the existing functions example, which prints `42`.
`--interpret` cannot execute reached native calls. Native `build` and `run`
default to `-O3`; use `--opt-level 0` to compare an unoptimized build.
`--cc` selects a C compiler executable and `--zig-cc` selects `zig cc`.
Compiler and linker arguments pass directly to processes. See
[native builds](native-builds.md) for exact options and limitations.

Choose an example according to the work the program needs:

| Program | Starting point | What it demonstrates |
| --- | --- | --- |
| Small computation or command result | [Factorial](../examples/factorial.rill), [blocks](../examples/blocks/main.rill) | Inferred functions, control flow and a printed `main` result |
| Reusable source library | [Modules](../examples/modules/main.rill), [library catalogue](../stdlib/README.md) | `import`, aliases, `export fn`, explicit bundled `std` imports |
| C-facing scalar library | [Scalar exports](../examples/c-exports/README.md) | Explicit i32 C exports, generated headers and interface manifests |
| Data model and mutation | [Health/shield](../examples/records/README.md) | Nominal records and disjoint mutable field loans |
| Generic functional code | [Functions](../examples/functions/README.md), [local values](../examples/local-values/README.md) | Generic callbacks, closures, currying and local schemes |
| Owned buffer processing | [Owned examples](../examples/owned/README.md), [JSON result](../examples/json/result.rill) | Allocation policy, moves, borrows, typed errors and cleanup |
| Bounded parsing/serialization | [JSON examples](../examples/json/README.md), [bundled library example](../examples/stdlib/main.rill) | Static UTF-8 and runtime owned bytes with explicit capacities |
| Native windowed application | [Direct SDL3](../examples/sdl3-direct/README.md) | Header imports, native libraries and explicit foreign resource destruction |
| Graphics experiment | [Voxel sandbox](../examples/sdl3-voxel/README.md) | A bounded Rill CPU reference plus external Metal compute shaders |

A normally returning zero-argument `main` prints its result followed by a
newline. That printed value is not a user-selected process exit code. Existing
examples demonstrate accepted subsets; they do not imply general file I/O,
runtime strings, arbitrary recursion or a freestanding entry point.
The SDL3 example needs installed development headers/libraries, pkg-config
and libclang. The Metal example has its own platform requirements.

A simple reusable abstraction uses an exported function in a local file:

```rill
// arithmetic.rill
export fn twice(value) { value + value }
```

```rill
// main.rill
import arithmetic;
fn main() { arithmetic.twice(21) }
```

Save both files in the same directory, then run `rill run main.rill` with a
built compiler. Imports resolve from the entry directory; imported modules
cannot define `main`. This example's addition constrains `value` to `i32`.
The existing [module example](../examples/modules/main.rill) exercises this
workflow in the regression suite.

## Reload boundaries

File watching can rebuild and restart a project after source changes. A
restart gives the replacement process new program state. An embedded host can
also own a checked program and explicitly choose when to replace its function
bodies, within the signature and value restrictions of its reload API.

Native per-function replacement in a running generated executable requires a
separate versioned plugin ABI, stable function identities, an explicit state
schema/migration protocol and proof that active jobs/callbacks no longer use
retired code. Changing code must not silently reinterpret live owner layouts
or foreign pointers. These are requirements from
[PLAN.md section 14.5](../PLAN.md#145-practical-development-loop), with current
evidence and next steps in the [hot-reload blocker](blockers/hot-reload.md).

## Dependency-ordered compiler work

1. Finish the common checked sequential representation and owner-place model.
   Cover owned records, initialization joins, partial moves, cleanup transfer
   and escaping-borrow rejection before allowing those forms in more APIs.
2. Add closed effects/resource summaries and allocator capabilities. Build
   growable buffers, strings and slices with allocation-failure cleanup and
   invalidation rules. Include destruction and foreign calls in summaries.
3. Add constrained generic interfaces, trait coherence and ownership-aware
   closures. Then define explicit borrowed/owning dynamic dispatch and public
   interface serialization; preserve conventions and destruction behavior.
4. Expand low-level operations against explicit numeric, layout and unsafe
   contracts. Verify integer/float rules, target widths, aggregate ABI adapters
   and callback retention with target C fixtures.
5. Add deterministic, bounded compile-time evaluation and hygienic expansion.
   Check generic bodies under declared constraints; generated code must return
   through ordinary name, type, ownership and effect checking.
6. Build native reload on explicit exports, interface identities and state
   migration. Add concurrency only with frame ownership, joins, cancellation
   and quiescence rules that the reload protocol can use.
7. Expand optimization, target profiles and releases against the conformance
   and reproducibility gates. Later syntax remains rejected until its
   consumers implement the corresponding checks.

These steps summarize dependencies; the authoritative work packages and
acceptance cases remain in [PLAN.md sections 19–21](../PLAN.md#19-dependency-ordered-delivery-plan).
They are outstanding engineering work, not completed features.

## Performance evidence

[Build scaling](build-performance.md) documents module splitting, indexed callable
lookups, retained watch inputs and the remaining separate-compilation work.

Compact aggregates, monomorphization, C optimization and a SIMD instruction
selection are implementation choices, not a whole-language speed result.
The historical buffer measurements in [native builds](native-builds.md)
describe a particular reference-emitter snapshot, compiler and host. They
do not establish current performance for every program.

For each performance change, keep the workload, input, compiler/target flags
and arithmetic policy fixed. Check independent expected outputs, mutated
state, allocation/failure behavior and cleanup events before timing. Compare
debug and optimized execution and the supported independent evaluator/C
consumers. Record warmups, repetitions, median/tail times, variance, allocation
counts, peak memory, code size and clean/incremental compilation costs when
relevant. Document regressions and capability skips. A source watch loop or
reload host also needs measured edit-to-accepted-update latency.

Follow the [project verification commands](../AGENTS.md#verification) and
report which checks actually ran. A skipped toolchain or platform integration
is not evidence that it works.
