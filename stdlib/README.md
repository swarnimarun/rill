# Experimental library modules

The compiler bundles these experimental Rill source modules. Import them
explicitly from any entry directory, without copying library files:

```rill
import std.json_input as input;
import std.json_output as output;
import std.result;
import std.alloc as alloc;
import std.cli as cli;
```

The catalogue now contains 15 modules. New source utilities use braces and
semicolons, explicit errors and caller-selected storage. See the
[standard library guide](../docs/standard-library.md) for complete signatures,
edge cases and runnable examples.

| Modules | Purpose |
| --- | --- |
| `std.option`, `std.result` | Type-only Option and Result, including supported owner payloads |
| `std.option_ops`, `std.result_ops` | Opt-in generic helpers for Copy values and callbacks |
| `std.int` | Checked/saturating i32 arithmetic and bounds utilities |
| `std.ascii`, `std.text` | Locale-free byte classification and static-string search/spans |
| `std.bytes`, `std.binary` | Checked owned-buffer operations and signed endian encoding |
| `std.range` | Half-open integer cursors with checked length |
| `std.alloc` | Explicit recoverable system allocation |
| `std.json_input`, `std.json_output` | Bounded byte parsing and serialization |
| `std.cli` | Supplied token classification and integer parsing |
| `std.reload` | Explicit host generation context |

The `_ops` helpers are separate because the current owner path rejects generic
functions, even unused imports. The other new utility modules are monomorphic
and can coexist with owner programs. No implicit prelude is introduced.
Importing `std.bytes` or `std.binary` selects the owner subset through their
declarations; keep those imports separate from `_ops` modules even if unused.

The default alias is the final segment, such as `json_input` or `result`.
`std` is reserved for the shipped catalogue. Local files cannot shadow bundled
modules, and unknown `std` imports report `E-MODULE-STDLIB` without searching
the filesystem. No library is linked implicitly. There is no package manager,
search path, or stable interface version.

`std.cli` parses supplied string tokens without allocation or mutation:

- `Span = { start: i32; end: i32 }` uses zero-based byte offsets and an
  exclusive end into the supplied string.
- `classify(text, options)` returns `Token`: `Positional(Span)`,
  `LongOption(Span)`, `LongValue(name_span, value_span)`, `ShortOption(ascii)`,
  `EndOptions`, or `InvalidOption(Span)`. Pass `false` after `EndOptions` to
  make subsequent tokens positional.
- `span_equal(text, span, expected)` compares bytes, returning `false` for an
  invalid span.
- `parse_i32(text, span)` returns `result.Result<i32, cli.ParseError>`, accepting
  optional `+`/`-`, decimal digits and leading zeros over the full signed range.
  Errors are `InvalidSpan`, `ExpectedDigit(offset)`, `InvalidDigit(offset)` and
  `IntegerOverflow(offset)`. Offsets refer to the original token, including
  when parsing a value inside `--name=value`. The first error wins.

Long names start with an ASCII letter and continue with ASCII letters, digits,
`_` or `-`. Values can contain arbitrary bytes present in the string, including
`=`; empty values remain empty spans. Short options have one ASCII letter;
clusters return `InvalidOption`. Negative decimal tokens remain positional.
The caller defines options, duplicates, subcommands and separate-token values.
The module does not provide process-argument input or convert foreign pointers
to strings. See the [runnable CLI example](../examples/cli/README.md).

`std.result` exports the ordinary algebraic type
`Result<a, e> = Ok(a) | Err(e)`. Use qualified constructors such as
`result.Ok(42)` and exhaustive matching. Importing it adds no special Result
semantics, propagation operator, or allocation behavior.

`std.alloc` exports `AllocError = InvalidLength | SizeOverflow | OutOfMemory`
and two hosted system-allocation functions:

- `box_i32(value)` returns `std.result.Result<SystemBoxI32, std.alloc.AllocError>`.
- `bytes_zeroed(length)` returns `std.result.Result<SystemBytes, std.alloc.AllocError>`.

Both inputs are `i32`. Negative byte lengths return `InvalidLength`; sizes that
cannot include storage metadata on the target return `SizeOverflow`. Neither
case calls the allocator. A null allocation returns `OutOfMemory`. Success
owns initialized storage, including one allocation for zero bytes. Errors
fabricate no owner. Match consumes the affine result and normal scope cleanup
releases any live success payload:

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

These are the canonical bundled nominal types. Same-shaped local types named
`Result` or `AllocError` cannot substitute for them. The lower-level
`system_box_i32` and `system_bytes_zeroed` builtins also require the bundled
types through explicit imports. Their `_abort` counterparts keep trapping on
failure. This API adds no custom allocator, allocation capability, growable
container, propagation operator or general unwinding guarantee.
See [allocation limits](../docs/blockers/owned-allocation.md).

Both JSON modules operate on `system_bytes_zeroed_abort` owned buffers. Allocation
belongs to the caller. They borrow storage for each call and do not allocate,
resize, transfer ownership, or retain a reference.

`json_output` exports these writers:

- `write_string(inout output, static_text, offset)` escapes a static UTF-8 string.
- `write_i32(inout output, value, offset)` writes any signed 32-bit integer.
- `write_bool(inout output, value, offset)` writes `true` or `false`.
- `write_null(inout output, offset)` writes `null`.

Success returns the number of bytes written, without a trailing NUL. Invalid
offsets or insufficient capacity return `-1` without changing any destination
byte. Check the result before advancing an append offset. The string writer
uses the same implementation as the earlier `examples/owned/json_output.rill`.
The numeric writer covers the full signed range without negating `i32::MIN`.

`json_input.parse_i32(input)` parses a complete runtime byte buffer as a JSON
integer document and returns `result.Result<i32, json_input.ParseError>`.
JSON whitespace may surround the number; `-0` and the full signed i32 range
are accepted. Errors carry a zero-based byte offset:

- `ExpectedDigit(offset)` marks the missing or invalid first digit, after a minus if present.
- `LeadingZero(offset)` marks the second digit after an initial zero.
- `IntegerOverflow(offset)` marks the first digit that exceeds the signed range.
- `TrailingData(offset)` marks the first non-whitespace byte after the integer.

The first error wins. Fractions and exponents report trailing data because
this API accepts only integers. Input may contain arbitrary bytes; parsing
requires no UTF-8 assumption and leaves the buffer unchanged.
`json_input.i32_valid(input)` matches this result to a boolean.
`json_input.parse_i32_or(input, fallback)` returns the parsed value or the
caller-selected fallback. All three operations allocate no storage.

These APIs do not provide a general JSON tree, runtime string decoding,
floating point, growable output, I/O, or a stable standard-library contract.

Run [the typed parsing example](../examples/json/result.rill) with
`cargo run -- run examples/json/result.rill`; it parses runtime bytes for `-7`
and matches both success and every error variant. The
[bundled-library example](../examples/stdlib/main.rill) writes and parses `42`
using only explicit `std` imports.

`tests/stdlib_json.rs` compares the interpreter with generated C at `-O0` and
`-O2`, using full-range integer round trips, independently expected decimal
bytes, malformed runtime input, capacity failure, and concatenated writes.
`tests/json_result.rs` checks typed errors, offsets and unchanged input across
790 cases through the interpreter and C at both optimization levels.


`std.reload` exports `Context = { generation: i32; previous: i32 }`,
`context(generation, previous)`, `changed(read context)` and
`first_load(read context)`. A host supplies the accepted generation and the
last one it handled; a negative previous generation denotes first load.
These are pure source decisions with explicit inputs, without global state or
implicit callbacks. See [reload workflows](../docs/hot-reload.md) and the
[runnable source example](../examples/reload/main.rill).
