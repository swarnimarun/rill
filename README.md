# Rill

Rill is an experimental compiler for the language described in [PLAN.md](PLAN.md).
It currently implements a small sequential subset. It is not yet a stable
language; see [phase status](docs/phase-status.md) and [remaining work](docs/blockers/remaining-phases.md).

The `v1` checkpoint includes a [cloud handoff](docs/CLOUD_HANDOFF.md) and
[continuation plan](docs/CONTINUATION_PLAN.md).

Install a Rust toolchain and a C11 compiler, then run:

```sh
cargo run -- check examples/factorial.rill
cargo run -- run examples/factorial.rill
cargo run -- build examples/factorial.rill -o /tmp/factorial
/tmp/factorial
```

`run` uses the checked typed evaluator for supported pure Rill
programs. Other supported Copy programs use the checked reference evaluator
while their typed lowering is implemented. Source native dependency
directives select a compiled native run automatically; `run --native` or
compiler/link arguments also select native execution.
For differential testing, `backend::emit_reference` explicitly emits the checked
Copy subset using the uniform C representation. It rejects owner programs and
does not change the default backend or provide a new CLI mode.
`build` invokes `cc` to compile C11 output.
Set `CC` or pass `--cc <executable>` to choose a C compiler; `--cc` takes
precedence over `CC`. Use `--zig-cc` to invoke `zig cc`. The two compiler
options cannot be combined. Pass compile flags with repeated `--c-arg` and
linker arguments with repeated `--link-arg`, for example:

```sh
cargo run -- build examples/factorial.rill -o /tmp/factorial --zig-cc --link-arg -L/path/to/lib --link-arg -lm
```

Arguments are passed directly to the compiler without shell parsing. `emit-c`
writes the C source to standard output. A normally returning zero-argument
`main` prints its result followed by a newline. A proved nonreturning main has
no result to print; its actual computation and traps are preserved.

Conservative source formatting is available without loading dependencies:

```sh
cargo run -- fmt examples/blocks/main.rill
cargo run -- fmt examples/blocks/main.rill --check
```

`fmt` writes validated output to stdout; `--check` exits unsuccessfully when
formatting is needed. Neither mode edits the input. Tokens, literal spellings,
comments, braces and semicolons are preserved. See the
[source and formatting contract](docs/source-format.md) for the current limits.
The opt-in `syntax::SyntaxTree` API exposes lossless grammar nodes, structured
diagnostics and bounded tooling recovery. Erroneous trees expose no parsed AST;
compiler commands and `fmt` retain strict rejection. Its optional
[AST origin map](docs/ast-source-origins.md) provides validated structural paths
and explicit origins for both source and parser-generated nodes. The optional
[source-preserving module loader](docs/module-source-origins.md) retains owned
snapshots and origins through resolution, renaming and native coalescing.
The opt-in [function-instance origin index](docs/typed-function-origins.md)
retains resolved definitions, closed parameter/result types and every requesting
source call for reachable typed functions. It is not a complete typed-node or
generated-C source map. A separate [Copy-body origin API](docs/typed-copy-node-origins.md)
now covers pure typed expressions, operands, patterns and local capture/shape
elaboration with explicit generated and erased provenance. Native, owner-cleanup
and generated-C byte-range coverage remain separate work.

The primary syntax uses functions, braces and semicolon-separated statements:

```rill
fn main() {
    let mut value = 0;
    while value < 6 {
        value <- value + 1;
    };
    if value == 6 { value * 7 } else { 0 }
}
```

A block's final expression supplies its value. Empty blocks and blocks ending
in a semicolon return unit `()`. Every non-tail expression needs a semicolon,
including `while`, `if` and nested blocks. Newlines do not separate statements.
Functions take comma-separated parameters with optional type annotations.
Function return annotations, local type annotations and `if` without `else`
are not supported yet. Legacy `let name arg = expression`, `let ... in`,
`if ... then ... else ...` and `while ... do ...` remain accepted for compatibility.

Rank-one inference uses a private union-find equality solver with levels and an
independent environment check for generalization. Immutable local lets generalize
only eligible nonexpansive values; call, conditional and match results remain
one monotype. Top-level functions retain rank-one polymorphism. See the
[value restriction and migration examples](docs/decisions/0003-local-value-restriction.md)
and [solver scope and verification](docs/verification/2026-11-03-type-solver.md).
Eligible immutable local value schemes also share typed interpreter/C execution:
[concrete captures and static shapes](docs/decisions/0004-local-value-shapes.md)
retain one initializer evaluation and specialize each whole-value or field use
at its exact type. Unused phantom shapes need no arbitrary default type;
unconstrained real uses and weak monotypes can still lack a concrete layout.
The [local-value example](examples/local-values/README.md) demonstrates immutable
snapshots and independent whole-record/field instances.
All-nullary enums can retain unresolved phantom arguments as frozen logical
identities while using verified tag-only layouts. Dynamic tags, weak/mutable
locals, generic calls and root loans use that ordinary typed path without a
default type. See the
[bounded K1 contract and verification status](docs/typed-nullary-phantoms.md).
The accepted [K2 phantom-product increment](docs/typed-phantom-products.md)
supports complete finite Copy records/enums with unresolved logical arguments,
including `Option<Flag<opaque>>`; genuine unknown physical payloads remain out
of scope. Independent and full aggregate acceptance are recorded in that contract.
The bounded [nonreturning-result representation](docs/blockers/nonreturning-results.md)
now preserves bare recursive results and unused immutable divergent lets without
fabricating storage or a return value. The [integrated K3 checks](docs/verification/2026-11-04-nonreturning-results.md)
pass 860 default and 908 importer-enabled tests. The subsequent bounded
[interrupted-argument increment](docs/blockers/interrupted-arguments.md) represents
ordinary value calls whose eager argument cannot return, preserving prefixes,
real recursion and the complete checked source without a dummy argument slot.
Used/mutable layoutless bindings, returning unknown payloads and broader
interrupted-call modes remain unsupported. Its
[integrated verification](docs/verification/2026-11-04-interrupted-arguments.md)
passed 892 default and 940 importer-enabled tests, both strict lint and Windows
compile modes, plus independent source/proof/event checks.

The subset includes signed 32-bit integers, booleans, unit, arithmetic,
comparisons, short-circuit `&&` and `||`, generic algebraic types and exhaustive
pattern matching. `//` starts a comment. Evaluation is left-to-right; integer
overflow and division by zero trap. Calls currently trap above 32 active Rill
function calls, including `main`. This temporary guard does not prevent every
possible host stack exhaustion. Names and types are checked before execution
or C emission. Syntax and behavior may change as the plan is implemented.

```rill
type Option<a> = None | Some(a)
fn main() { match Some(42) { Some(x) => x | None => 0 } }
```

Source exhaustiveness proofs have a deterministic 100,000-work-unit limit per
match. Exceeding it reports `E-MATCH-COMPLEXITY`, rather than guessing whether
cases are missing. Catch-all patterns do not enumerate their type's values.
See the [coverage work policy](docs/verification/2026-11-03-exhaustiveness-reconstruction.md).

Ordinary string literals are immutable UTF-8 with program lifetime.
`byte_len(s)` returns the byte length and `byte_at(s, i)` returns a byte as
`i32`, trapping on an out-of-range index. Embedded NUL is allowed in these
strings. They are distinct from foreign C strings. The `str` type names these
static strings in ADT fields.

Fixed-size byte buffers provide limited owned storage. `bytes_new(N)` creates
`N` zero bytes for a literal `N` from 0 through 256. `bytes_len(b)` returns its
length; `bytes_get(b, i)` returns a byte; and `bytes_set(b, i, value)` returns a
new buffer with the selected byte replaced. Indices outside the buffer and
values outside 0 through 255 trap. Buffers have value semantics and cannot be
resized. They are not an allocator-backed vector or a borrowed slice.

The explicit `u8x16` type contains sixteen byte lanes. `u8x16_load(buffer, offset)` loads a
checked 16-byte window; `u8x16_lane(vector, index)` reads a
checked lane as an `i32`. `u8x16_and`, `u8x16_or`, `u8x16_xor` and `u8x16_eq`
operate lane by lane. Equality returns `0` or `255` in each lane; `u8x16_any`
and `u8x16_all` treat a nonzero lane as true. Invalid windows or lane indices
trap. These values and all eight operations use the shared checked typed path,
including concrete generic functions, enums, records and Copy root/field loans.
C stores a vector in exactly sixteen bytes. Copy vectors can coexist with the
restricted owner subset. See [examples/u8x16.rill](examples/u8x16.rill).

Generated C defaults to portable scalar operations. `--simd neon` on
`rill build` explicitly selects AArch64 NEON intrinsics for the four bitwise
and equality operations; it does not add architecture flags or perform runtime CPU
feature detection. The caller must select a compatible target and C compiler.
An unsupported compiler target fails the C build. For `emit-c`, compile the
output with `-DRILL_SIMD_NEON=1` to make the same explicit selection. Rill does not
implement automatic vectorization or guarantee performance; see
[SIMD limitations](docs/blockers/simd.md).

Functions can take exclusive access to an initialized mutable local with an
`inout` parameter:

```rill
fn bump(inout x) { x <- x + 1 }
fn main() {
    let mut value = 41;
    bump(inout value);
    value
}
```

An `inout` argument must name a mutable local or a forwarded `inout` parameter.
The loan lasts through the call. Later arguments cannot read, write or borrow
the same root; earlier value arguments are evaluated before the loan. The
compiler rejects overlapping root loans.

A `read x` parameter takes shared access. Pass an initialized local or
parameter or record field as an ordinary expression, or pass a computed
expression to borrow a temporary value for the call:

```rill
fn inspect(read x) { x + 1 }
fn main() { let value = 41; inspect(value) }
```

Multiple `read` parameters may borrow the same root, and later arguments may
read it. A write or `inout` borrow conflicts with an active shared loan. A
`read` parameter cannot be assigned or forwarded as `inout`. Both loan kinds
last until the call returns. Computed expressions evaluate at their argument
position; their temporary remains alive through the call. A bare local or
field, even in parentheses, borrows that place.

Root loans of scalars, static strings, fixed byte buffers, finite Copy enums
and finite Copy records use the shared checked typed interpreter/C path,
including concrete generic
function instances. The IR independently checks modes, mutability and loan
conflicts during later argument evaluation. Copy record field loans share this
path, including inside restricted owner programs. No borrowed address can
escape a call.

Nominal records have named fields. Construction evaluates fields in the order
written. Functions can annotate parameters when field access needs a known
record type. Finite Copy records use the shared checked path for construction,
field reads and assignment, and root or field loans. Compact C structs
have no enum tag; empty records use a legal dummy byte. Field assignment
evaluates its right-hand side before updating the original root slot,
preserving other fields and whole-record replacements. Unresolved locally
polymorphic record layouts still need separate elaboration. For example:

```rill
type Point = { x: i32; y: i32 }
fn bump(inout p: Point) { p.x <- p.x + 1 }
fn main() {
    let mut p = record Point { y = 0; x = 41 };
    bump(inout p);
    p.x
}
```

An `inout p.x` argument borrows a field. Field paths may contain up to 16
segments, such as `inout p.position.x`. A root overlaps every field, and a
field overlaps its descendants; sibling fields can be borrowed independently.
This subset has no escaping references, partial moves, or general borrow
inference. On the shared typed path, an explicit field loan retains the original
slot and projection path; C passes the address of the compact struct field.
Forwarding composes paths without holding a Rust cell borrow across a call.
Value arguments and computed read temporaries remain independent snapshots.
Record fields currently use Copy semantics.

The hosted `system_box_i32_abort(value)` builtin is a restricted non-Copy
allocation. Bind it directly to a local, read it with `box_read(owner)`, update
a mutable owner with `box_write(inout owner, value)`, and transfer it to a
local or value parameter with `move owner`. Normal scope exit frees a live box.
A trap, including allocation failure, does not promise Rill cleanup. Owner
programs support acyclic monomorphic functions with value parameters and
`read` or `inout` owner parameters. Functions may return a concrete owner with a tail `move owner`, or forward
an owner-returning tail call. Returned owners can initialize locals or enum fields. Owner
programs remain monomorphic; generic owner identity functions are rejected.
Finite enums, including concrete generic `Result` instances, can contain owners.
Construct them with explicit moves, then consume them with `match move result`.
Unconsumed payloads are freed in reverse field order when the selected arm ends.
Owner-valued `if` and `match` expressions require all arms to leave enclosing
owners in the same state. Borrowed Copy roots/fields and computed read
temporaries are supported. Borrowed owner-bearing enum parameters, borrowed owner returns,
owned records and foreign declarations remain unsupported
in owner programs. See
[owned allocation limits](docs/blockers/owned-allocation.md) and
[the runnable examples](examples/owned/README.md).

`system_bytes_zeroed_abort(length)` creates a fixed-length owned byte buffer
using the hosted system allocator. The length is a runtime `i32`, and the bytes
start at zero. Use `owned_bytes_len(owner)` and `owned_bytes_get(owner, index)`
to read it, and `owned_bytes_set(inout owner, index, value)` to write it.
Negative or unrepresentable lengths, invalid indices, values outside 0 through
255, and allocation failure trap. The buffer can be moved or borrowed under
the same restrictions as the owned box; it cannot yet be resized or viewed as
a string or slice.

For recoverable hosted allocation, import `std.alloc` and `std.result`.
`alloc.box_i32(value)` and `alloc.bytes_zeroed(length)` return ordinary affine
`Result` values containing an owner or `alloc.AllocError`. Handle them with
`match move`; errors are `InvalidLength`, `SizeOverflow` and `OutOfMemory`.
See [the allocation API](stdlib/README.md) and [fallible example](examples/owned/fallible.rill).
The `_abort` builtins retain their trapping behavior.

File-based commands support small local modules. Imports must precede
declarations and resolve from the entry file's directory:

```rill
// math.rill
fn increment(x) { x + 1 }
export fn answer(x) { increment(x) }
```

```rill
// main.rill
import math as m;
fn main() { m.answer(41) }
```

Run the included example with `cargo run -- run examples/modules/main.rill`.
Nested imports such as `import helpers.math;` load `helpers/math.rill` and
use `math` as the default alias. All local paths resolve from the entry directory.
The compiler also bundles `std.json_input`, `std.json_output`, `std.result`,
and `std.alloc`.
Use `import std.json_input as input;` explicitly; no library files need to be
copied. The `std` root is reserved, and unknown names are errors. See
[the library APIs](stdlib/README.md) and
[examples/stdlib/main.rill](examples/stdlib/main.rill).
Only exported declarations are visible across files. `export type` also
exports its constructors. Imports are confined to the entry directory and
cycles are rejected. Imported files cannot declare `main`. Binding modules can
expose `export extern fn` and `export extern const`, or keep foreign declarations
private behind exported Rill wrappers. Their native dependency directives
aggregate into the entry program, including automatic native `run` selection.
This is an early module subset; see [module limitations](docs/blockers/modules.md).
For a bare `p.x`, a local `p` takes precedence over an import alias; a
qualified call such as `p.f(...)` refers to the module alias.

Constructor names begin with an uppercase letter. Concrete finite enums use
compact typed tagged unions in C, including nested and literal Copy patterns.
Nullary enums store only a tag. These programs use
the same independently checked typed representation for interpretation and C
emission. Copy matches evaluate the scrutinee once and choose the first matching
arm, including in restricted owner programs. Owner-bearing patterns still use
the restricted consuming form described above.

Reachable generic functions specialize to concrete typed functions, including
recursive groups, generic enum results and distinct types at different calls.
Instance identity includes both parameters and result, preserving phantom and
return-only type arguments. Every source declaration is still checked, while
only reachable bodies are instantiated. The implementation caps this worklist
at 1024 concrete source-function instances and type nesting at 128, reporting a
diagnostic if exceeded. Owner programs retain their monomorphic restrictions.
Eligible immutable local schemes use concrete captures and statically known
shapes, with separate exact instances for whole-value and field uses. The local
elaborator permits 100,000 work units and 256 concrete capture leaves per
function instance, with shape height capped at 128. Unresolved real uses and
weak monotypes can still be rejected by C output with `E-C-ADT`; see the
[C ADT layout blocker](docs/blockers/c-adt-layout.md).

Native C programs declare dependencies and selected signatures explicitly:

```rill
@include("SDL3/SDL.h");
@link("SDL3");
@pkg_config("sdl3");
extern const SDL_INIT_VIDEO: u32;
extern fn SDL_Init(flags: u32) -> bool;
extern fn SDL_Quit() -> unit;
fn main() {
    let ready = SDL_Init(SDL_INIT_VIDEO);
    SDL_Quit();
    if ready { 0 } else { 1 }
}
```

`check` and `emit-c` validate native declarations against the actual headers
with the selected C compiler. `build` also links dependencies; `run` builds
and executes them. Rill checks inference and moves; the target C compiler
checks signature compatibility. Header checking does not prove foreign pointer
lifetimes, effects or resource cleanup. See [C interoperability](docs/c-interop.md)
for supported exact types, raw pointers, compiler flags and manual linking.
Existing native calls/constants, `u8`/`u32`/`u64`/`f32` values and exact nullable
raw pointers use explicit checked typed operations when the surrounding Copy
program has a supported layout. This includes conversions, null tests, equality,
concrete generics, records/enums, root/field loans and once-only local constant
captures. Every native declaration and header check remains present, including
unused signatures. Pointer-slot loans preserve pointee spelling and constness;
they do not prove foreign lifetimes or pointee independence. Unrelated unsupported
Copy layouts retain the checked uniform fallback. The evaluator never invokes
C: it rejects reached native work while preserving pure paths and the reference
error order. See the [bounded raw-transport verification](docs/verification/2026-11-03-typed-native-transport.md).

Selected declaration import is available with the optional `c-import` feature;
see [the importer workflow](docs/c-interop.md#import-selected-declarations).
An experimental explicit `--config-out` / `--config` workflow can save a bounded
recipe and materialize its binding module separately. `check`, `emit-c`, `build`
and `run` accept `--native-config` plus an explicit compiler to freshly validate
that recipe and exact binding source. It is not an implicit sidecar or success
cache; omitting the option retains legacy behavior. See
[explicit configurations](docs/c-interop.md#explicit-native-configurations-experimental).
On Unix, the CLI uses an isolated worker with a 30-second default deadline
(`--timeout-ms` overrides it), bounded IPC and cleanup on failure or cancellation.
The library importer remains in-process; this is not a memory/security sandbox.
There is no effect/unsafe checker yet.

The [JSON syntax validator and bounded decimal and string serializers](examples/json/README.md)
demonstrate static string and buffer operations.

The [health and shield example](examples/records/README.md) uses a record
with disjoint field loans.

The [raylib click challenge](examples/raylib-click/README.md) and
[SDL3 collector](examples/sdl3/README.md) use small C shims for native graphics.
Their build instructions require the corresponding development libraries.

Inference bounds expanded public type materialization and copying. Excessive
size, depth or cumulative work returns `E-TYPE-COMPLEXITY`, including compact
generic programs whose inferred trees would grow exponentially. See the
[resource policy](docs/verification/2026-11-03-type-materialization.md) for exact
limits and scope. This is not a whole-compiler memory guarantee.

Run the tests with `cargo test`.

The [direct SDL3 click game](examples/sdl3-direct/README.md) calls SDL3 without
a C shim. Its integration test builds and runs a short headless version against
the real library when SDL3 development files are available.


To reproduce actual headless SDL3 framebuffer captures of both games, see
[`scripts/capture-sdl3.sh`](scripts/capture-sdl3.sh) and the
[native SDL3 verification report](docs/verification/2026-11-03-sdl3-native.md).
These captures use the real SDL software renderer; they do not simulate
physical keyboard or mouse input.

The accepted bounded implementation of [normally returning discarded results](docs/blockers/discarded-results.md)
adds a separate logical-only result for unused immutable bindings, with actual
calls and constructor operand effects. It does not admit unknown stored payloads
or observable unresolved main results. Its [integrated verification](docs/verification/2026-11-04-discarded-results.md)
passes 939 default and 987 importer-enabled tests, strict lint and Windows compile
checks, plus independent proof/event/C cases. Direct layoutless records remain open.
