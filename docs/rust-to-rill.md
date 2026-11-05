# Moving from Rust to Rill

Rill is experimental and implements a smaller language than Rust. Start with
small checked computations, bounded buffers or a C-hosted tool. Before porting
an application, check the [feature boundary](language-features.md) and
[standard library catalogue](standard-library.md). There is no Cargo package
ecosystem, general `Vec`/`String`, iterator protocol, async runtime or trait system.

This guide uses implemented syntax. Possible future additions are collected in
the [migration feature proposals](proposals/rust-migration.md) for discussion.
Continue with [practical recipes](rust-migration-recipes.md) for range traversal,
concrete callbacks and supplied-token parsing. The
[ranked increment queue](proposals/next-language-increments.md) separates useful
future additions from current working alternatives.

## Build and run

Build the compiler with `cargo build --locked --release --bin rill`. Put the
resulting `rill` executable on your PATH, or substitute `target/release/rill`
in these commands:

```sh
rill new my-tool
rill project check my-tool
rill project run my-tool
rill project build my-tool
rill project watch my-tool
rill docs --output build/docs
```

A project has a strict versioned `rill.json`, a source entry and a native output.
Use [project commands](projects.md) for options and paths. Direct source commands
are also available: `rill check main.rill`, `rill run main.rill`,
`rill run main.rill --interpret`, and `rill build main.rill -o build/tool`.
The interpreter cannot execute reached foreign calls. Native builds emit C11
and invoke the selected C compiler, with `-O3` by default.

A normally returning `main()` prints its result and a newline. Returning `42`
prints `42`; it does not set the process exit status to 42. Process argument
input, general file I/O and runtime strings are not standard-library services
yet. `std.cli` parses string tokens supplied by a caller, not process argv.

## Familiar syntax, different operations

| Rust habit | Current Rill spelling or boundary |
| --- | --- |
| `fn f(x: i32) -> i32 { x + 1 }` | The same braced function form works; types can often be inferred |
| `let mut x = 1; x = 2;` | `let mut x = 1; x <- 2;` |
| Tail expression or `return value` | Use a tail expression; early `return` is unavailable |
| `if`, `while`, blocks | Expressions; a non-tail expression needs a trailing semicolon |
| `break`, `continue`, `for`, `loop` | Use the explicit patterns below; these are not control-flow keywords |
| `struct Point { x: i32 }` | `type Point = { x: i32 }` |
| `Point { x: 1 }` | `record Point { x = 1 }` |
| `enum Choice<T> { None, Some(T) }` | `type Choice<a> = None \| Some(a)` |
| `match value { A => ..., B => ... }` | `match value { A => ... \| B => ... }` |
| `use crate::math;`, `pub fn` | `import math;`, `export fn` |
| `&T`, `&mut T` parameters | Call-scoped `read` and `inout` parameter modes |
| `?`, `.unwrap()`, methods | Explicit `match`, ordinary helper functions, or supported `_ops` modules |
| `impl`, traits, `dyn Trait` | No direct equivalent; generic functions and concrete closures cover some cases |
| `println!`, macros, `comptime` | No macro/compile-time evaluation language; `main` prints its result |

`!condition`, `&&` and `||` work on booleans. `&&` and `||` short-circuit.
There are no implicit truthy integers. Equality uses `==`; mutation uses `<-`.
Strings support byte-content `==` and `!=`, including embedded NUL bytes.
`std.text.equal` remains available. Record and enum operators still reject;
compare fields or use explicit matches. The
[search/equality guide](search-and-equality.md) provides checked examples,
including guarded loop suffixes after a match is found.
Local bindings currently infer their types, so `let x: i32 = 1;` rejects.
Closed function parameter/result annotations can constrain an inferred local.

## Records, mutation and borrowing

This complete [example](../examples/rust-migration/basics.rill) prints `42`:

```rill
type Counter = { value: i32 }

fn advance(inout counter: Counter) {
    counter.value <- counter.value + 1;
}

fn main() {
    let mut counter = record Counter { value = 40 };
    advance(inout counter);
    advance(inout counter);
    counter.value
}
```

`inout counter` at the call grants an exclusive loan for that call. A `read`
parameter grants a shared loan; its caller supplies the value or place without
an `&` marker. Supported disjoint record fields can be borrowed separately.
Overlapping exclusive/shared loans reject. These are call-scoped operations,
not stored references. Returned views, explicit lifetime parameters and general
borrowed slices are unavailable. See [the ownership boundary](blockers/ownership.md).

Finite Copy records have nominal identity and compact C layouts. They do not
provide Rust layout attributes or a promise of C ABI compatibility. Owned
record fields and partial moves remain unsupported. Construction field values
execute in written order.

## Control flow

`break` and `continue` are not implemented. For a bounded search, put the stop
condition in `while`. For filtering, guard the work inside the loop. This
[complete program](../examples/rust-migration/control-flow.rill) prints `42`:

```rill
import std.option as option;

fn first_digit_index(text: str) -> option.Option<i32> {
    let mut offset = 0;
    let mut found = -1;
    while offset < byte_len(text) && found < 0 {
        let code = byte_at(text, offset);
        if code >= 48 && code <= 57 {
            found <- offset;
        };
        offset <- offset + 1;
    };
    if found < 0 { option.None } else { option.Some(found) }
}

fn sum_odd_below(limit: i32) -> i32 {
    let mut next = 0;
    let mut sum = 0;
    while next < limit {
        let value = next;
        next <- next + 1;
        if value % 2 != 0 {
            sum <- sum + value;
        };
    };
    sum
}

fn main() {
    match first_digit_index("abcdef7") {
        option.Some(index) => index + sum_odd_below(12)
        | option.None => 0
    }
}
```

The search still runs the increment after finding a digit. It skips further
search iterations when the next condition runs. A boolean flag alone does not
skip the remaining statements in the current body. Guard any remaining work
that must not happen after a stop condition. In the filtering loop, advancing
the counter before the guard keeps skipped items from repeating forever.

Use `while true { ... }` for an intentionally unbounded unit-valued loop. This
does not provide a value-producing `loop` or a way to exit it. `std.range`
offers `new`, `is_empty`, `next` and `next_back` over half-open i32 ranges;
these are ordinary functions, not an iterator or `for` protocol.

There is no early `return`. Arrange the result as a final `if` or `match`, or
extract a helper function with that tail expression. Recursion has a depth
guard and no promised tail-call elimination, so replacing a long loop with
recursion can introduce a depth trap. Nested loops need separate conditions;
there are no labels or nonlocal loop exits.

These alternatives work best for short loops. Abrupt exits would help larger
parsers and scans, but require independently checked owner cleanup and loan
rules on every exit. Their [proposal](proposals/rust-migration.md#control-flow)
records the value and acceptance criteria without adding syntax prematurely.

## Enums and recoverable errors

Import Option and Result explicitly. Constructors use the module alias, such
as `option.Some(value)` or `result.Err(error)`. Match arms use `|`, not commas.
The type parameters in `type Choice<a> = ...` are inferred type variables, not
Rust trait bounds. This [complete example](../examples/rust-migration/results.rill)
prints `42`:

```rill
import std.result as result;

type ArithmeticError = DivisionByZero

fn divide(value: i32, divisor: i32) -> result.Result<i32, ArithmeticError> {
    if divisor == 0 {
        result.Err(DivisionByZero)
    } else {
        result.Ok(value / divisor)
    }
}

fn main() {
    match divide(84, 2) {
        result.Ok(answer) => answer
        | result.Err(DivisionByZero) => 0
    }
}
```

This example handles a zero divisor only. The i32 minimum divided by minus one
still traps. Ordinary i32 arithmetic is checked in optimized builds too.
Use `std.int` for checked Result-returning and explicit saturating operations.
Unsigned/floating types mainly serve native transport and conversions; they
do not yet offer general arithmetic. There is no implicit integer widening,
general numeric casting syntax or wrapping arithmetic convention.

`std.option_ops` and `std.result_ops` provide generic functions such as `map`
and `and_then` for Copy payloads. A fallback value argument is evaluated eagerly;
use a callback-taking alternative when evaluation must be conditional. The
owner path currently rejects generic functions even in unused imports. Import
the type-only `std.option`/`std.result` modules for owner payloads instead.
There is no `?` operator, exception unwinding or implicit error conversion.

## Explicit owned storage

This [example](../examples/rust-migration/ownership.rill) allocates an i32 owner
through the recoverable API and prints `42` on success:

```rill
import std.alloc as alloc;
import std.result as result;

fn main() {
    let outcome = alloc.box_i32(42);
    match move outcome {
        result.Ok(owner) => box_read(owner)
        | result.Err(error) => match error {
            alloc.InvalidLength => 1
            | alloc.SizeOverflow => 2
            | alloc.OutOfMemory => 3
        }
    }
}
```

`move outcome` consumes the affine Result. Its successful arm owns the box,
which is released on normal scope exit. Ordinary owner movement is explicit;
copying an owner without `move` rejects. `_abort` allocation operations trap
instead of returning an error. Traps do not promise cleanup or unwinding.
There is no user-defined Drop trait, growable Vec or general allocator trait.

`str` currently denotes static UTF-8 strings. Byte access uses `byte_len` and
`byte_at`, with byte offsets rather than Unicode character indices. Runtime
owned bytes use explicit capacities and checked `std.bytes`/`std.binary`
helpers. They are not growable strings or borrowed slices.

## Modules and reusable abstractions

Put `export fn twice(value) { value + value }` in `math.rill`, then use
`import math; fn main() { math.twice(21) }` in `main.rill` beside it. The entry
directory is the local module root. Imports are explicit; bundled `std` sources
need no files in the project. Declaration order is not a module search path.
Unimported files do no compilation work, while every declaration in imported
files is checked. Imported modules cannot define `main`.

Generic functions specialize statically for reachable concrete uses. Immutable
local lets generalize only eligible nonexpansive values. Mutable locals and
many computed values remain monomorphic. Do not expect Rust turbofish syntax,
trait resolution, associated types, dynamic trait objects or const generics.
Functions and closures support immutable Copy captures; captures retain concrete
environment identities. Different closure types do not implicitly erase to a
shared dynamic callable type. See [functions](functions.md).

Use plain functions, records and enum matches for small abstractions. Reach for
C bindings when an application needs existing OS or graphics services, following
the [foreign ownership boundary](c-interop.md). A raw C pointer has no checked
Rill lifetime, dereference API or automatic resource destruction. Ordinary Rill
calls and the scalar C export API are different interface contracts.

## Diagnose a failed port

Run `rill check` on the smallest example first. Parser errors include a line
and column; semantic errors carry category codes such as `E-TYPE`, `E-NAME`,
`E-LOAN` and `E-MODULE-PRIVATE`. Full source-span coverage is still incomplete.
Contextual help is advisory. A hint does not relax the checker or convert
source automatically. For example, a local `answer` misspelled as `asnwer`
through the file loader reports:

```text
E-MODULE-NAME: @entry: unknown symbol asnwer
help: did you mean 'answer'?
```

Suggestions use the actual visible namespace. Private imported names and
compiler-generated identities are excluded. Tied near matches, short names and
scopes beyond the bounded suggestion budget produce no arbitrary choice.
Missing qualified members remain ordinary name errors, even when their names
are `break`, `continue` or `return`.

Unsupported Rust declaration forms suggest `type`, `record`, `export` or
`import` where appropriate. Local `: Type` bindings point to function annotations
and inference; Rust-style `=` mutation points to `<-` and `==`. Unsupported
`?` and `&` forms explain explicit matching and call-scoped parameter modes.
Rejected `loop`/`for` syntax and undefined control-flow words link to current
loop alternatives. These hints preserve strict rejection and source-tree
recovery rather than guessing the intended program.

1. Check the spelling and import alias. There is no implicit prelude.
2. Compare syntax with the table and the complete examples above.
3. For owner/layout errors, reduce the example to the admitted subset before
   introducing generic callbacks or new data structures.
4. If the feature is missing, consult the [proposal queue](proposals/rust-migration.md)
   and [phase status](phase-status.md). Do not use foreign pointers to bypass
   a missing ownership rule.

Generate searchable local HTML with `rill docs --output build/docs`. The guide
has the address `language/rust-to-rill.html`; headings have stable generated
anchors within that page. For a project manual, use `rill project docs`.
