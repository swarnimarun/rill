# Practical recipes for Rust developers

These programs use the implemented Rill subset. Start with the
[migration guide](rust-to-rill.md) for syntax, ownership and missing features.
Run a source file with `rill run path/to/file.rill`, or use `--interpret` for
the checked interpreter. The examples below need no native dependencies.

## Traverse and filter an integer range

To port a small `for value in start..end` loop, keep a `std.range` cursor and
request its next value explicitly. Guard the work that should happen only for
selected values. This complete program prints `25`, the sum of odd integers
in the half-open interval from zero through nine:

```rill
import std.range as range;
import std.option as option;

fn sum_odd_between(start: i32, end: i32) -> i32 {
    let mut cursor = range.new(start, end);
    let mut sum = 0;
    while !range.is_empty(cursor) {
        match range.next(inout cursor) {
            option.Some(value) => {
                if value % 2 != 0 {
                    sum <- sum + value;
                };
            }
            | option.None => ()
        };
    };
    sum
}

fn main() {
    sum_odd_between(0, 10)
}
```

The cursor advances before the filter guard, so skipped values cannot prevent
progress. `range.next` borrows the mutable cursor only during its call and
returns an ordinary `option.Option<i32>`. Reversed and empty bounds yield no
values. The `None` arm remains explicit even though this loop checks exhaustion
before calling `next`.

This is an ordinary source-library cursor, without an iterator trait, adapter
chain or `for` syntax. The accumulator uses checked i32 addition and can trap
on a large sum. Use the [checked integer helpers](standard-library.md) when
overflow must be represented as a recoverable error. For early search exits,
use the [condition/guard patterns](rust-to-rill.md#control-flow).

Source: `examples/rust-migration/recipes/range-filter.rill`.

## Pass an operation as a concrete callback

For a small abstraction that needs one operation, pass an ordinary function
instead of first designing a trait or method hierarchy. This program prints
`42` by adding three twice:

```rill
fn apply_twice(transform, value) {
    transform(transform(value))
}

fn make_offset(offset: i32) {
    fn(value) { value + offset }
}

fn main() {
    let add_three = make_offset(3);
    apply_twice(add_three, 36)
}
```

`apply_twice` is inferred generically and specializes for its reachable concrete
calls. It can also receive a boolean-negating callback or a different concrete
closure in another call. `make_offset` captures an immutable Copy integer by
value; constructing this supported closure does not introduce a heap box.

The concrete closure environment remains part of its type. This recipe does
not let a conditional join unrelated closure types into a dynamic function
object. It also does not establish trait bounds, associated types or automatic
method lookup. Capturing a mutable local, borrowed place or owner is outside
the supported callable subset. See [functions](functions.md) for currying,
composition and the current owner/capture boundaries.

Source: `examples/rust-migration/recipes/copy-callback.rill`.

## Parse a supplied option with application rules

`std.cli` classifies supplied static-string tokens. Keep the accepted spelling,
numeric parsing and application policy as separate decisions. This complete
program accepts only the attached form `--count=N`, requires one through 64,
and prints `8`:

```rill
import std.cli as cli;
import std.result as result;

type CountError = ExpectedCountOption | InvalidCount(cli.ParseError) | CountOutOfRange

fn parse_count(token: str) -> result.Result<i32, CountError> {
    match cli.classify(token, true) {
        cli.LongValue(name, value) => {
            if cli.span_equal(token, name, "count") {
                match cli.parse_i32(token, value) {
                    result.Ok(count) => {
                        if count >= 1 && count <= 64 {
                            result.Ok(count)
                        } else {
                            result.Err(CountOutOfRange)
                        }
                    }
                    | result.Err(error) => result.Err(InvalidCount(error))
                }
            } else {
                result.Err(ExpectedCountOption)
            }
        }
        | _ => result.Err(ExpectedCountOption)
    }
}

fn main() {
    match parse_count("--count=8") {
        result.Ok(count) => count
        | result.Err(_) => 0
    }
}
```

| Token | Result |
| --- | --- |
| `--count=1`, `--count=64` | Accepted endpoints |
| `--count=+12` | Accepted signed decimal |
| `--count=0`, `--count=65`, `--count=-1` | `CountOutOfRange` |
| `--counts=8`, `--count`, `8` | `ExpectedCountOption` |
| `--count=` | `InvalidCount(cli.ExpectedDigit(8))` |
| `--count=12x` | `InvalidCount(cli.InvalidDigit(10))` |
| `--count=2147483648` | `InvalidCount(cli.IntegerOverflow(...))` |

Parse-error positions are zero-based byte offsets into the original token,
including the option prefix. `span_equal` compares the name without allocating
a substring. `parse_i32` handles the complete signed i32 range; the narrower
one-through-64 rule belongs to this application. Explicit matching preserves
the library's typed parse error without a `?` operator or implicit conversion.

This example reads a literal supplied token. Rill does not yet expose process
argv or general runtime strings through its standard library. It does not
handle separate-token values, duplicate options or subcommands. A host must
supply inputs, and the application must define those policies. See the
[CLI library contract](../stdlib/README.md) before building a larger parser.

Source: `examples/rust-migration/recipes/count-option.rill`.

## Checks and next steps

`tests/rust_migration_recipes.rs` keeps the complete source snippets synchronized
with these files. It exercises their demo outputs and empty/reversed/negative
ranges, distinct Copy callback instances, option bounds, missing values, bad
digits and integer overflow through both the interpreter and C at O0/O3.

These are useful ways to structure small current programs, not replacements
for every Rust abstraction. If the explicit form becomes repetitive in a real
port, bring that program to the [feature proposals](proposals/rust-migration.md)
discussion. Iterator protocols, abrupt loop exits, trait interfaces and runtime
I/O each need their own checked semantics and acceptance evidence.
