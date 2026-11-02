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

`run` uses the reference evaluator for pure Rill programs. Source native dependency
directives select a compiled native run automatically; `run --native` or
compiler/link arguments also select native execution.
`build` invokes `cc` to compile C11 output.
Set `CC` or pass `--cc <executable>` to choose a C compiler; `--cc` takes
precedence over `CC`. Use `--zig-cc` to invoke `zig cc`. The two compiler
options cannot be combined. Pass compile flags with repeated `--c-arg` and
linker arguments with repeated `--link-arg`, for example:

```sh
cargo run -- build examples/factorial.rill -o /tmp/factorial --zig-cc --link-arg -L/path/to/lib --link-arg -lm
```

Arguments are passed directly to the compiler without shell parsing. `emit-c`
writes the C source to standard output. The evaluator and built program print
the result of the zero-argument `main` function followed by a newline.

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
trap. See [examples/u8x16.rill](examples/u8x16.rill).

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

Nominal records have named fields. Construction evaluates fields in the order
written. Functions can annotate parameters when field access needs a known
record type:

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
inference. The current implementation copies
an `inout` field into a temporary and writes it back on normal return; the C
backend may copy the containing record. Record fields currently use Copy
semantics.

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
owners in the same state. Borrowed non-owner or enum parameters, borrowed owner
returns, owned records, foreign declarations and vectors remain unsupported
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

Constructor names begin with an uppercase letter. C output supports inline
algebraic values and specializes generic calls with concrete types. Unresolved
values and some polymorphic local bindings are rejected with `E-C-ADT`; see the
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
There is no automatic declaration import or effect/unsafe checker yet.

The [JSON syntax validator and bounded decimal and string serializers](examples/json/README.md)
demonstrate static string and buffer operations.

The [health and shield example](examples/records/README.md) uses a record
with disjoint field loans.

The [raylib click challenge](examples/raylib-click/README.md) and
[SDL3 collector](examples/sdl3/README.md) use small C shims for native graphics.
Their build instructions require the corresponding development libraries.

Run the tests with `cargo test`.

The [direct SDL3 click game](examples/sdl3-direct/README.md) calls SDL3 without
a C shim. Its integration test builds and runs a short headless version against
the real library when SDL3 development files are available.
