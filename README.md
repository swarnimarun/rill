# Rill

Rill is a statically typed systems language with explicit effects, mutable value
semantics, and failure as ordinary control flow. It takes the failure-and-choice
machinery from Verse, the “mutate a value through a place” style from Hylo, and a
small ML-ish surface.

This repository is the compiler, written in Rust. Today it parses, resolves,
type-checks and emits C, then lets `cc` produce the binary. `PLAN.md` describes
the MLIR backend and the missing middle-end; that is the target, not what this
tree builds. I keep this README about what runs now.

## Status

The compiler builds and runs the examples and the C interop demos, and the
benchmark programs under `bench/` have working Rill versions. It is still a
bootstrap compiler, so the list of things that work is more useful than a
feature grid.

Working now:

- the front end: lexer, lossless CST, parser, AST, builder desugaring, name
  resolution and HIR;
- type and effect checking, including records, enums, tuples, fixed arrays,
  closures, currying and `dyn` trait objects;
- `<decides>`, `|`, `?`, `let?`, `option`/`one`/`all`, `when` guards, and
  rollback of writes in a failed speculative region;
- `Buf` byte buffers, arenas with marks and an ambient `arena_bind` layer;
- `sync`/`race`, stackless coroutines, an I/O executor and a pthread pool;
- `f64x4` SIMD values;
- `@include`/`@link`: read a C header, import its declarations, link the library,
  with no build file;
- `rillc fmt` and a language server (`rill-lsp`, also reachable as `rillc lsp`).

Not there yet:

- the MLIR backend; `rill-codegen` emits C as the bootstrap;
- a separate MIR and borrow checker. `inout` becomes a C pointer today;
- `sink` and `set` parameter conventions are parsed but not enforced. Only
  `inout` changes the ABI;
- `rush` and `branch`; `race` is limited to unit-valued arms;
- the standard `seq`/`simd`/`gpu`/`arena`/`query` builders desugar in the AST but
  have no definitions in this tree. The `f64x4` vector type is what runs;
- `comptime` beyond `comptime { zeroed() }`;
- general projections. A projection can yield a field place, not a computed
  temporary;
- array literals. Fixed arrays are declared as `[T; N]` and filled by index or
  zeroed with `comptime { zeroed() }`;
- a file/module system. One file is one program, and external names come from
  `use std::io;`, `@include`d headers, or `extern` declarations you write;
- `deinit` clauses. They parse, but code generation does not lower them.

`PLAN.md` still contains designs for all of the above, and several milestone
labels there are older than the code. Read it as a design notebook.

## Build and run

You need a recent Rust toolchain (the language server uses edition 2024), a C
compiler for native builds, and `zig` if you want static Linux binaries. SDL3 or
raylib are only needed for the graphics demos.

```console
$ cargo build --release
$ export PATH="$PWD/target/release:$PATH"
$ rillc --help
```

To install both binaries into `~/.cargo/bin`:

```console
$ cargo install --path crates/rill-driver
$ cargo install --path crates/rill-lsp
```

The driver is `rillc`, and the language server is `rill-lsp`; there is no `rill`
binary. `rillc lsp` starts the same server.

A minimal program, `hello.rill`:

```rill
use std::io;

let main () -> i32 <io> = {
    io::println("hello, rill");
    0
};
```

```console
$ rillc exec hello.rill
hello, rill
```

`main`'s return value is the process exit status, so a program that returns `42`
exits with 42. A `main` that returns `()` exits 0. A failable `main` that fails
without a handler exits 1.

`rillc exec` compiles the script into `~/.rill/cache` and runs it. The cache key
covers the script text, the headers its `@include`s resolved to, the libraries
its `@link`s resolved to, and the running compiler. An unchanged script reuses
the cached binary; editing any of those rebuilds it. `RILL_CACHE_DIR` moves the
cache somewhere else.

## A longer example

Here are records, an enum, `match`, a loop, recursion, tuples and text in one
program:

```rill
use std::io;

type Stats = record {
    calls: i32;
    value: i32;
};

type Shape = | Circle i32 | Rect i32 i32 | Empty;

let fib (n: i32) -> i32 <computes> =
    if (n < 2) { n } else { fib(n - 1) + fib(n - 2) };

let area (s: Shape) -> i32 <computes> = match s {
    Circle r -> 3 * r * r,
    Rect w h -> 2 * (w + h),
    Empty -> 0
};

let main () -> i32 <io> = {
    var total = 0;
    for (i := 0..<10) { set total = total + i };
    var s = Stats 1 (fib 10);
    var pair = (total, s.value);
    io::println(
        "sum " ++ (pair.0 as String)
            ++ " fib " ++ (pair.1 as String)
            ++ " area " ++ (area(Rect 3 4) as String)
    );
    0
};
```

```console
$ rillc exec tour.rill
sum 45 fib 55 area 14
```

## The language

### Files and comments

A `.rill` file is a program. Top-level items are functions (`let`), records and
enums (`type`), traits and impls, projections (`project`), `extern`
declarations, C imports (`@include`), link requests (`@link`), `use` lines and
module-level `static var`s.

`#` and `//` start a line comment, `/* ... */` is a nested block comment, and a
run of `///` lines documents the declaration that follows. The language server
shows that documentation on hover and completion, including documentation
carried out of C headers.

A block is an expression. Its last expression is its value, which is why there
is no `return` statement. Statements are separated by `;`; the final expression
in a block does not need one.

### Bindings and storage

`let` declares a function. Values bind with `:=` (immutable) or `var`
(mutable); `set` assigns to an existing place.

```rill
use std::io;

let main () -> i32 <io> = {
    var count = 0;
    set count = count + 1;

    n := 7;                 # immutable local
    twice := n * 2;
    io::println((twice as String));
    0
};
```

A plain `var` is stack storage and belongs inside a block. At module level,
storage must be explicit: `static var` is one copy in the data segment, and
`static threadlocal var` is one copy per thread.

```rill
static var calls: i32 = 0;
static threadlocal var scratch: [i32; 4] = comptime { zeroed() };
```

Large fixed arrays normally start zeroed. There are no array literals yet.

### Functions

A function names its parameters and return type, then writes its effect row.

```rill
let add (x: i32, y: i32) -> i32 <computes> = x + y;
```

The row can be omitted for pure code; `<computes>` is the explicit spelling.
`()` is the unit type and `()` is the unit value.

Calls can use spaces or a parenthesised argument list:

```rill
add 20 22
add(20, 22)
```

An adjacent call, `f[x, y]`, is the failure-aware form. For a callee that cannot
fail it behaves like `f(x, y)`; for a `<decides>` callee, failure propagates into
the surrounding choice or handler. Spacing matters: `f [x]` is not the same
parse. Arrays use the same bracket syntax, `xs[i]`, so the checker decides
whether brackets mean a call or an index.

Partial application works with constant captures. `add 1` fixes the first
parameter and returns a function of the rest:

```rill
use std::io;

let add (x: i32, y: i32) -> i32 <computes> = x + y;

let main () -> i32 <io> = {
    var n = 41;
    inc := add 1;
    io::println((inc n) as String);
    0
};
```

Closures use braces. A non-capturing closure is a plain function pointer, and a
capturing one carries its environment:

```rill
use std::io;

let apply (f: (i32) -> i32, value: i32) -> i32 <computes> = f value;

let main () -> i32 <io> = {
    n := 7;
    add_n := { x -> x + n };
    io::println("captured " ++ (apply(add_n, 35) as String));
    0
};
```

Parameters can be marked `inout`, and the call site also says `inout`:

```rill
use std::io;

let bump (n: inout i32) -> () <writes> = { set n = n + 1 };

let main () -> i32 <io | writes> = {
    var n = 41;
    bump(inout n);
    io::println((n as String));
    0
};
```

`inout` is how a function mutates the caller's place. It is lowered as a C
pointer. `sink` and `set` are accepted by the parser but currently do not add
enforcement; the notes in “Status” apply.

### Effects

Effects are written after the return type and checked. A `|` separates entries
in a row.

```rill
let main () -> i32 <io | alloc | writes> = 0;
```

| Effect | What it covers |
| --- | --- |
| `computes` | no effects; local computation and local `var`s |
| `io` | console, process I/O and FFI |
| `fs` | filesystem access; the `io::read_file` family adds this alongside `io` |
| `alloc` | allocation: buffers and arenas |
| `writes` | writes through a place, `inout` parameter or static |
| `reads` | reads through a place or static |
| `decides` | the expression or function may fail |
| `suspends` | it may suspend or compose suspending work |
| `transacts` | shorthand for `reads \| writes \| alloc \| decides` |

Mutating a local `var` does not require `<writes>`; mutating through `inout` or
a static does. Effects propagate to callers. Omitting one is an error:

```text
error: `main` performs io but its signature declares <computes>
```

Custom effect names are accepted in rows, but the compiler does not give them
semantics beyond propagation.

### Control flow

`if` is an expression, `while` takes a condition, and `for` walks a range.
`break` exits the nearest loop. Ranges are inclusive with `..` and half-open
with `..<`.

```rill
var i = 0;
while (i < 10) {
    set i = i + 1;
    if (i = 5) { break } else { () }
};

var total = 0;
for (j := 0..<10) { set total = total + j };   # j is 0 through 9
for (j := 0..10) { set total = total + j };    # j is 0 through 10
```

Equality is `=`; `!=` is inequality. The lexer also accepts `==`, and the
examples use both. `and`, `or` and `not` are contextual: they are ordinary names
when an operator is not expected.

`match` takes patterns and binds payloads. An arm can also carry a guard:
`Rect w h if w = h -> w * h`.

```rill
let area (s: Shape) -> i32 <computes> = match s {
    Circle r -> 3 * r * r,
    Rect w h -> 2 * (w + h),
    Empty -> 0
};
```

### Types

| Kind | Spelling |
| --- | --- |
| integers | `i8` … `i128`, `u8` … `u128`, `isize`, `usize` |
| floats | `f32`, `f64` |
| boolean | `Bool` |
| text | `Text` (also written `String`) |
| fixed array | `[T; N]` |
| tuple | `(A, B)` |
| SIMD | `f64x4` |
| C pointer | `&raw T`, `&raw mut T` |

`isize` and `usize` are currently 64-bit. Integer literals can carry a suffix
such as `42i64`, `65u8` or `0u64`; unsuffixed integer literals take their type
from context.

Text is immutable. `++` concatenates, `size` returns its length in bytes,
`as String` renders a number, and indexing a `Text` yields a `u8`. `size` also
works on arrays.

Tuples are values; fields are `.0`, `.1`, and so on. Record fields use their
names. Fixed arrays are indexed with `xs[i]` and support nested places such as
`set xs[i].field = value`.

### Records and enums

```rill
type Stats = record {
    calls: i32;
    value: i32;
};

type Shape = | Circle i32 | Rect i32 i32 | Empty;
```

Records are constructed positionally, either `Stats 1 55` or `Stats(1, 55)`.
A field declared `var` can be written: `set account.balance = 100i64`. Enum
variants with payloads are constructed the same way: `Circle 3`, `Rect 4 5`,
`Empty`. `match` destructures them.

### Traits and `dyn`

A trait declares members over a type parameter. An `impl` supplies them, and a
plain call dispatches statically.

```rill
use std::io;

trait Show a = {
    let show (x: a) -> String <io>;
};

type Point = record { x: i32; y: i32; };

impl Show Point = {
    let show (p: Point) -> String <io> = "Point";
};

type Rect = record { w: i32; h: i32; };

impl Show Rect = {
    let show (r: Rect) -> String <io> = "Rect";
};

let main () -> i32 <io> = {
    io::println(show(Point 1 2));
    var d: dyn Show = Rect 3 4;
    io::println(d.show());
    0
};
```

A direct call such as `show(Point 1 2)` resolves to the `Point` impl at compile
time. `d.show()` goes through the `{ data, vtable }` pair that a `dyn` value
stores, so the same call site can reach either impl. Output is `Point`, then
`Rect`.

### Projections

A projection yields a place. Uses of the projection are inlined as that place,
so reads and writes cost nothing extra.

```rill
type Account = record {
    id: u64;
    var balance: i64;
};

project balance_mut (acc: inout Account) -> inout i64 <computes> = {
    yield inout acc.balance
};

let deposit (acc: inout Account, amount: i64) -> () <computes> = {
    set (acc.balance_mut) = acc.balance + amount
};
```

Reading `acc.balance_mut` reads the field; assigning through it writes the
field. This is the implemented form: the body must be a single
`yield inout <place>`. Computed projections, where `yield` produces a temporary
that is written back on release, are still future work.

### Failure and choice

`<decides>` marks a function or expression that can fail. `fail` is the failure
primitive. A bare boolean statement in a `<decides>` body is a guard: when it is
false, the function fails.

`f[x]` calls a failable function, and `e1 | e2` takes `e2` when `e1` fails.
A fallback that cannot fail discharges the failure, which is why `pick` below
stays `<computes>`.

```rill
let first_positive (x: i32, y: i32) -> i32 <decides> =
    if (x > 0) { x } else { if (y > 0) { y } else { fail } };

let pick (x: i32, y: i32) -> i32 <computes> = first_positive[x, y] | 0;
```

`?` forwards failure. On an `Option` it unwraps the payload and makes absence a
failure; on a deciding call it propagates that call's failure. `let? x = e`
binds a successful value or fails the enclosing function.

The handlers package this up:

- `option { e }` yields the value or absence; `?` unwraps it.
- `one { e1 | e2 | ... }` yields the first success and fails when every arm fails.
- `all { e1 | e2 | ... }` collects every success into a slice; `size` reads the
  count. It needs `<alloc>`.
- `when (cond);` inside a handler is a guard: when false, the rest of the block
  is skipped and the handler takes its failure path.

```rill
use std::io;

let first_positive (x: i32, y: i32) -> i32 <decides> =
    if (x > 0) { x } else { if (y > 0) { y } else { fail } };

let main () -> i32 <io | decides | alloc> = {
    var a = (option { first_positive[0, 7] })? | 0;
    var b = one { first_positive[0, 0] | first_positive[7, 0] };
    var found = all { first_positive[1, 0] | first_positive[0, 2] | first_positive[0, 0] };
    io::println((a as String) ++ " " ++ (b as String) ++ " " ++ (size found as String));
    0
};
```

### Rollback

A speculative region remembers writes and the ambient arena's watermark. If the
region fails, the writes are undone and the arena is reset before the fallback
runs. This is what keeps a transfer correct when the debit happens before the
guard that rejects it:

```rill
use std::io;

type Account = record { var balance: i64; };

let transfer (from: inout Account, to: inout Account, amount: i64) -> () <decides | writes> = {
    amount > 0;
    set from.balance = from.balance - amount;
    from.balance >= 0;
    set to.balance = to.balance + amount;
};

let main () -> i32 <io | decides | writes> = {
    var alice = Account 500i64;
    var bob = Account 100i64;
    option { transfer[inout alice, inout bob, 800] | transfer[inout alice, inout bob, 200] };
    io::println("alice " ++ (alice.balance as String) ++ " bob " ++ (bob.balance as String));
    0
};
```

The output is `alice 300 bob 300`: the 800 arm debits Alice and then fails, its
write is rolled back, and the 200 arm runs from the original balances. The undo
trail is a fixed buffer of 4096 entries and 1 MiB; overflowing it aborts the
process rather than silently losing a rollback. A nested region keeps its
entries until the outermost region commits.

`<transacts>` is an accepted effect alias for the full set of writes, reads,
allocation and decisions; the rollback itself is inserted at the speculative
region, not requested by a separate annotation.

### Memory

There is no garbage collector. Local values live on the stack. Two explicit
tools cover the rest: `Buf`, a growable byte buffer, and `Arena`, a bump
allocator with marks.

```rill
use std::io;

let main () -> i32 <io | alloc | writes> = {
    var a = arena_new 1024u64;
    arena_bind(a);
    var block = arena_alloc(a, 128u64);
    var out = buf_new();
    out.buf_push(65u8);
    out.buf_push(66u8);
    io::println(buf_text(out) ++ " arena " ++ (arena_used(a) as String));
    arena_unbind();
    io::println("buffer bytes " ++ (buf_len(out) as String));
    0
};
```

Output: `AB arena 128` then `buffer bytes 2`.

The builtins are:

| Name | Signature and note |
| --- | --- |
| `buf_new ()` | `Buf`, `<alloc>` |
| `buf_push (b, byte)` | append one `u8`, `<alloc>`; also `b.buf_push(byte)` |
| `buf_len (b)` | `usize` |
| `buf_get (b, i)` | `u8` |
| `buf_text (b)` | `String`, `<alloc>` |
| `arena_new (bytes)` | `Arena`, `<alloc>` |
| `arena_bind (a)` / `arena_unbind ()` | bind or pop the ambient layer, `<writes>` |
| `arena_mark (a)` | current watermark, `usize` |
| `arena_alloc (a, bytes)` | `&raw mut u8`, `<alloc>` |
| `arena_reset (a, mark)` | free back to a mark |
| `arena_used (a)` | bytes in use, `usize` |

An ambient arena is consulted by things like `all { ... }` and closure
environments when they need storage. Layers nest; `arena_unbind` returns to the
previous one. `all { ... }` restores the watermark when its region fails, which
is why `examples/layers.rill` can show the same usage before and after a failed
region.

### Concurrency

`<suspends>` covers suspension and composition. A function with `yield ()` is a
stackless coroutine; the I/O kernel steps it until it reports done.

```rill
use std::io;

let twice (n: i32) -> i32 <suspends> = n * 2;

let main () -> i32 <io | suspends> = {
    var both = sync { twice(20); twice(1) };
    io::println("sync " ++ (both.0 as String) ++ "," ++ (both.1 as String));
    0
};
```

`sync { ... }` joins its arms. Valued arms form a tuple; unit arms just run. With
an I/O kernel bound, unit arms with no captured environment are queued as tasks;
otherwise they run in order, which is observationally the same when nothing
suspends.

The kernel itself is bound from the language, the same way an arena is:

```rill
io_bind();
io_spawn(task);
var steps = io_run();
io_unbind();
```

`io_run` drains the queue and returns how many steps ran. A task that spawns
another is picked up in the same run. `race { ... }` is available for unit-valued
arms; the kernel stops at the first completion. Cancellation is cooperative, so
a started arm runs to its next `yield`. `rush` and `branch` are parsed but not
lowered.

Threads use a pthread pool:

```rill
threads_bind(2);
spawn_thread(task);        # returns Bool
var joined = join_threads();
```

`spawn_thread` accepts a named task or a closure whose captures the compiler can
prove are ownable. The pool queues work when every worker is busy; `join_threads`
waits for all queued work. There is no data-race checker here, so shared mutable
state is still the programmer's problem.

See `examples/coroutine.rill`, `examples/executor.rill` and
`examples/threads.rill` for complete programs.

### SIMD

`f64x4` is a four-lane vector with the ordinary arithmetic operators and
indexing:

```rill
use std::io;

let main () -> i32 <io> = {
    var a = f64x4 1.0 2.0 3.0 4.0;
    var b = f64x4 10.0 20.0 30.0 40.0;
    var sum = a + b;
    var doubled = a * f64x4 2.0 2.0 2.0 2.0;
    io::println(
        "sum " ++ (sum[0] as String) ++ " " ++ (sum[3] as String)
            ++ " doubled " ++ (doubled[2] as String)
    );
    0
};
```

Output: `sum 11 44 doubled 6`. Other lane types and the `simd { ... }` builder
are not implemented.

## Working with C

`@link` asks the linker for a library and `@include` reads a header. There is no
build file and no binding generator.

```rill
@link("SDL3");

type SDL3 = @include("SDL3/SDL.h");
use SDL3::*;

let main () -> i32 <io> = {
    SDL_Init(SDL_INIT_VIDEO);
    var win = SDL_CreateWindow("hello", 640, 360, 0);
    SDL_DestroyWindow(win);
    SDL_Quit();
    0
};
```

`@include` scans the named header and the headers it includes. Prototypes become
`extern "c"` functions with `<io>` (C can do anything, so the effects are
conservative), `typedef struct`/`union` become records that keep their C names,
enums become `i32` with their enumerators as constants, and `#define NAME literal`
becomes a constant. Documentation comments from the header travel with the
declarations: SDL3's Doxygen blocks and raylib's trailing comments are shown by
the language server.

Types the scanner cannot express are skipped: variadic functions,
function-pointer parameters and arrays. A name that never appears in the header
is reported as unknown at its use.

A pointer parameter to a struct, union or scalar is passed as a place with
`inout`:

```rill
SDL_RenderFillRect(ren, inout rect);
```

Opaque handles are passed as values. A `T *` in the header becomes an `inout`
parameter of `T` when `T` is a record or scalar, and a pointer value when `T` is
opaque.

For libc or any other C symbol, declare it yourself:

```rill
use std::io;

extern "c" let strlen (s: Text) -> usize <io>;

let main () -> i32 <io> = {
    io::println("strlen = " ++ (strlen("hello") as String));
    0
};
```

`&raw T` and `&raw mut T` are the pointer types for FFI. `malloc` and `free`
are declared with `&raw mut u8` in `examples/ffi.rill`.

Header search starts in the source file's directory, then checks
`RILL_INCLUDE_PATH`, `CPATH` and the standard prefixes. Library search checks
`LIBRARY_PATH`, `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`, then the usual system
directories. A found `@link` library also gets an `rpath`, so the produced binary
finds it at run time. `--link NAME` adds an extra `-lNAME`.

The SDL3 demos are `examples/sdl3/window.rill`, `plasma.rill` and
`sidescroller.rill`; the raylib ones are `examples/raylib/window.rill`,
`sidescroller.rill` and `match3.rill`. Set `RILL_SDL3_DEMO=1` (plus
`SDL_VIDEODRIVER=dummy` if there is no display) or `RILL_RAYLIB_DEMO=1` to run a
fixed number of frames and print a summary instead of opening a window.

```console
$ RILL_SDL3_DEMO=1 SDL_VIDEODRIVER=dummy rillc exec examples/sdl3/sidescroller.rill
score 1
score 2
score 3
demo frames 240 score 3
```

## Command line

```text
rillc build <FILE.rill> [-o OUT] [--target native|linux-musl] [--emit-c] [--link LIB]
rillc exec <FILE.rill> [ARGS...]
rillc fmt <FILE|DIR>... [--write]
rillc lsp
rillc <FILE.rill> [--dump=cst|ast|hir] [--stats]
```

- `build` runs the whole pipeline and writes an executable. The default output is
  the file stem in the current directory. The generated C is left next to it as
  `<OUT>.c`; `--emit-c` prints that path.
- `--target native` uses the host `cc` and produces a dynamically linked binary.
- `--target linux-musl` uses `zig cc -target <arch>-linux-musl -static`; the
  result is a statically linked ELF for the host architecture.
- `exec` compiles if needed into `~/.rill/cache` and runs the binary, forwarding
  the script's own arguments and its exit code.
- `fmt` prints the canonical form of each file. With `--write` it replaces the
  file. A directory argument formats every `.rill` file below it.
- `lsp` runs the language server over stdio. The `rill-lsp` binary is the same
  server under its own name.
- `--dump=cst` prints the lossless concrete syntax tree, `--dump=ast` the
  lowered AST, and `--dump=hir` the resolved HIR.
- `--stats` prints item, declaration, token, error and warning counts.

A bare `rillc FILE` only runs the front end (lex, parse and lower). It reports
syntax and lowering diagnostics but does not run name resolution, type checking
or code generation. Use `build` or `exec` for those.

Errors exit non-zero and render with the source line and a caret:

```text
error: `main` performs io but its signature declares <computes>
  --> hello.rill:3:24
   |
 3 | let main () -> i32 = { io::println("x"); 0 };
   |                        ^^^^^^^^^^^^^^^^
   = note: add the effect to the declaration, or call a function that does not need it
```

## Formatting

`rillc fmt FILE...` prints the canonical form of each file; `--write` edits it in
place. A directory is searched recursively for `.rill` files. Settings come from
the nearest `.rill-fmt` ancestor, so one file at the repository root can govern
the tree.

```toml
# .rill-fmt
max_width = 100
tab_width = 4
single_line_if_width = 80
single_line_block_width = 48
blank_lines_between_items = 1
preserve_blank_lines = true
```

Every key is optional; the values above are the defaults. The formatter decides
bracket adjacency from the tree, never from source spacing, so `xs[i]` stays a
bracket application while `xs [i]` stays an application to a separate argument.
Parenthesised argument lists are canonicalised to touch the callee, `f(x)`.
Long argument lists, arrays and match arms break one element per line.

`crates/rill-driver/tests/build.rs` formats every example and checks that the
program still behaves the same.

## Language server and editors

`rill-lsp` speaks JSON-RPC over stdio with full-document sync. It runs the same
parse, resolve and check pipeline as `rillc build`, so those diagnostics match a
build. It answers:

- diagnostics on open and change;
- hover, including the type and effect row;
- completion for names in scope, fields and methods after `.`, and module
  members after `::`;
- go to definition, including into an `@include`d C header;
- find references from the resolver's reference table;
- formatting with the nearest `.rill-fmt`;
- semantic tokens and inlay hints.

Both `rill-lsp` and `rillc lsp` start it. For Helix, the language server entry is:

```toml
[language-server.rill]
command = "rillc"
args = ["lsp"]

[[language]]
name = "rill"
scope = "source.rill"
file-types = ["rill"]
comment-tokens = ["#", "//"]
indent = { tab-width = 4, unit = "\t" }
language-servers = ["rill"]
```

The tree-sitter grammar and highlight queries live in
`editors/tree-sitter-rill`. Helix needs the grammar path as an absolute path and
a symlink for the queries; `editors/README.md` has the exact commands. The Zed
extension in `editors/zed-rill` is installed as a dev extension; it ships the
grammar and highlights but not a WASM server adapter, so the server is attached
manually in Zed's settings.

## How compilation works

The pipeline is:

```text
source
  -> lexer                    tokens, trivia preserved
  -> parser                   lossless CST
  -> AST lowering + desugar   builder protocol, and/or, handlers
  -> HIR + resolver           Res/LocalId/ExprId tables
  -> type and effect checker
  -> C code generation
  -> cc (native) or zig cc (static)
  -> executable
```

The backend is small on purpose. Typed HIR is rendered as C with a small
libc-backed runtime in the same translation unit: printing, buffers, arenas,
the undo trail, coroutines and the pthread pool. There is no separate Rill
runtime library to install, and a native binary links the platform's libc and,
when needed, pthreads.

The generated C is kept. For `build` it sits next to the output as `<OUT>.c`;
for `exec` it lives in the cache beside the binary. If code generation cannot
lower a construct, it reports a diagnostic naming the missing piece instead of
emitting a guess.

MLIR is the planned replacement for the C backend once the `rill` dialect and
the missing MIR/borrow passes land. The front end, HIR and type tables are meant
to survive that switch. Until then, the C path is the one that produces working
programs.

## Benchmarks

The three benchmark programs under `bench/` run the same algorithm in Rill, Rust
and Zig, with the same input and the same checksum asserted for all three. Times
are best-of-three wall clock for the whole process.

| Workload | Rill | Rust | Zig |
| --- | ---: | ---: | ---: |
| v1: byte scan, 200 passes | 0.12s | 0.13s | 0.13s |
| v2: JSON query, 55 KB | 0.03s | 0.03s | 0.02s |
| v3: JSON query, 577 KB | 0.50s | 0.51s | 0.50s |

The C compiler sees the same straight-line loops it would see from a hand-written
C program, which is why these stay close. `bench/RESULTS.md` has the input
details and the bugs the benchmark exposed; its “Planned rounds” section predates
v2 and v3 and no longer describes what is left.

## Repository map

```text
crates/
├── rill-span/          interned symbols, positions, spans, source map
├── rill-diagnostics/   labeled diagnostics and rendering
├── rill-syntax/        SyntaxKind, lossless CST, precedence table
├── rill-lexer/         hand-written cursor lexer
├── rill-parser/        resilient Pratt + recursive-descent parser
├── rill-ast/           AST, lowering, builder desugaring
├── rill-hir/           name resolution, HIR, C header expansion
├── rill-ty/            type inference and effect checking
├── rill-codegen/       typed HIR -> C
├── rill-cimport/       C header scanner
├── rill-fmt/           formatter and .rill-fmt config
├── rill-lsp/           language server
└── rill-driver/        rillc CLI, build/exec cache, linker flags
editors/
├── tree-sitter-rill/   grammar, highlights, corpus
└── zed-rill/           Zed dev extension
examples/               language examples and SDL3/raylib demos
bench/                  Rill, Rust and Zig versions of three workloads
PLAN.md                 design notebook and roadmap
```

## Examples

| Topic | Files |
| --- | --- |
| basics, loops | `fib.rill`, `counter.rill`, `closures.rill`, `captures.rill` |
| records, enums, match | `shapes.rill`, `traits.rill`, `dyn.rill` |
| failure and choice | `decides.rill`, `transfer.rill`, `transacts.rill`, `let_try.rill` |
| handlers | `option.rill`, `one.rill`, `all.rill` |
| memory | `alloc.rill`, `arena.rill`, `layers.rill` |
| projections | `projections.rill` |
| concurrency | `sync.rill`, `coroutine.rill`, `executor.rill`, `threads.rill` |
| SIMD | `simd.rill` |
| C interop | `ffi.rill`, `sdl3/`, `raylib/` |
| everything at once | `showcase.rill` |

`crates/rill-driver/tests/build.rs` builds every top-level example, runs it and
checks that it exits normally. Individual tests pin the output of the programs
whose behavior matters. The C import tests build and run the graphics demos
headless when the headers are installed; otherwise they skip.

## Development

```console
$ cargo test --workspace
$ cargo clippy --workspace --all-targets
$ cargo fmt --all
```

The workspace tests cover the lexer, parser, lowering, desugaring, resolver,
type checker, code generator, formatter, language server and end-to-end builds.
The static-ELF test skips when `zig` is not installed, and the SDL3/raylib tests
skip when their headers are missing.

The examples are also the regression corpus: `every_example_builds_and_exits`
will fail if a change makes a top-level example stop compiling or start dying
on a signal.
