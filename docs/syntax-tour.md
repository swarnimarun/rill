# Current Rill syntax

These examples use the accepted experimental subset. [PLAN.md](../PLAN.md)
also contains proposed syntax that the compiler does not accept yet.

## Functions and blocks

```rill
fn double(x) { x * 2 }

fn main() {
    let answer = double(21);
    answer
}
```

Save this as `main.rill`, then check, interpret or compile it:

```sh
cargo run -- check main.rill
cargo run -- run main.rill
cargo run -- run main.rill --interpret
cargo run -- build main.rill -o /tmp/rill-example
/tmp/rill-example
```

Native and interpreted executions print `42`. Parameters and results are inferred. Parameter
annotations such as `x: i32` are supported. Braced functions also accept a
closed result constraint, such as `fn double(x: i32) -> i32 { x * 2 }`.
Local type annotations and lambda result annotations remain unsupported.

Statements end in `;`. A block's final expression is its value; a trailing
semicolon makes the block return unit `()`. Newlines do not separate statements.

## Mutation and exclusive access

```rill
type Point = { x: i32; y: i32 }

fn bump(inout p: Point) { p.x <- p.x + 1 }

fn main() {
    let mut p = record Point { x = 0; y = 10 };
    while p.x < 42 {
        bump(inout p);
    };
    if p.x == 42 { p.x } else { 0 }
}
```

`let` is immutable, `let mut` permits assignment, and `<-` assigns. `inout`
borrows a mutable place for the call. `read` parameters borrow shared access.
Overlapping exclusive arguments are rejected. Disjoint record fields may be
borrowed separately. See the [record example](../examples/records/README.md).

Braced `if` may omit `else` when its body returns unit, such as
`if ready { draw(); };`. It returns `()` and does not return from the function.
Value-producing branches and legacy `if ... then ...` still need `else`.
Non-tail `if` and `while` expressions need a final semicolon.
Logical `&&` and `||` short-circuit. Use `not (predicate value)` when negating
a whitespace application. Use `not ready` or `!ready` for
boolean negation. Negation binds before binary operators, so a comparison needs
parentheses, as in `not (x == y)`.

## Algebraic types and matching

```rill
type Option<a> = None | Some(a)

fn value_or_zero(value) {
    match value {
        Some(x) => x
        | None => 0
    }
}

fn main() { value_or_zero(Some(42)) }
```

Constructors begin with an uppercase letter. Matches must cover every possible
case. Generic functions specialize to concrete types for C output.

A braced conditional pattern selects a single case. Both branches are
expressions, so a fallback can be written at its use site:

```rill
fn value_or(value, fallback) {
    if let Some(n) = value { n } else { fallback }
}
```

The compiler translates this to `match value { Some(n) => n | _ => fallback }`.
It evaluates the scrutinee once and binds `n` only in the matching block.
The same syntax works with nested patterns, qualified constructors and ordinary
`Result` values. An omitted `else` produces unit and requires a unit matching
block. Both branches are checked even for an irrefutable pattern. The existing
owner match rules apply, including explicit `move` for an owning local,
per-arm cleanup and restrictions on redundant owner arms. A failed pattern
selects the fallback after any effects of evaluating the scrutinee. See the
[runnable Option example](../examples/conditional-pattern.rill).

## Fixed byte values

Spell the exact capacity of an inline Copy byte buffer as `bytes<N>`, where
`N` is a decimal literal from 0 through 256. A function can borrow a buffer
for mutation and return a copy whose capacity stays visible to the caller.

```rill
fn put_first(inout page: bytes<8>, n: i32) {
    page <- bytes_set(page, 0, n)
}

fn main() {
    let mut page = bytes_new(8);
    put_first(inout page, 41);
    let snapshot = page;
    put_first(inout page, 1);
    bytes_get(snapshot, 0) + bytes_get(page, 0)
}
```

The result is 42. `bytes<7>` and `bytes<8>` have different types. Capacity
can also appear inside records, enums and type arguments. The existing
constructor and bounds checks apply, including bounds traps for any indexed
access on `bytes<0>`. Capacity expressions, capacity variables and C ABI
buffer parameters require additional language and layout rules. An existing
nominal type named `bytes` continues to accept ordinary type arguments such
as `bytes<i32>`. The [modal editor](../examples/modal-editor/README.md)
uses a caller-owned `bytes<256>` with an independently tracked text length.

The checker first infers the scrutinee type and checks the pattern against it.
Only the first arm extends the surrounding type environment with the pattern
bindings. It checks the fallback in the surrounding environment and unifies
the arms’ result types. Existing match rules check moves and join ownership
state at the exit. The [conditional-pattern tests](../tests/conditional_patterns.rs)
compare the elaborated AST directly with a written match before exercising
the interpreter and C11 output.

## Owned storage

```rill
fn main() {
    let mut bytes = system_bytes_zeroed_abort(8);
    owned_bytes_set(inout bytes, 0, 42);
    owned_bytes_get(bytes, 0)
}
```

This allocates eight zeroed bytes, writes one byte and prints `42`. The owner
is freed at normal scope exit. Transfer ownership with `move bytes`. Use
`std.alloc` and `match move` for recoverable allocation errors; see the
[owned examples](../examples/owned/README.md). The owner subset currently
rejects foreign declarations.

## Modules and SDL3

The direct game uses an ordinary binding module:

```rill
import sdl;

fn main() {
    let ready = sdl.SDL_Init(sdl.SDL_INIT_VIDEO);
    sdl.SDL_Quit();
    if ready { 0 } else { -3 }
}
```

Save this next to a copy of
[`examples/sdl3-direct/sdl.rill`](../examples/sdl3-direct/sdl.rill). Its
`@include`, `@pkg_config` and `@link` directives select the actual installed
SDL3 headers and library. `run` automatically compiles native dependencies.
The example prints `0` when initialization succeeds. Foreign pointers require
manual resource management; header checks establish ABI compatibility only.

For a complete window, drawing loop, input and cleanup:

```sh
cargo run -- check examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill --zig-cc
```

See [SDL3 setup and binding generation](../examples/sdl3-direct/README.md),
[C interoperability](c-interop.md) and [remaining phases](blockers/remaining-phases.md).
