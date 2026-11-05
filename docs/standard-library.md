# Standard library utilities

Rill bundles 15 experimental source modules. They use the ordinary name, type,
ownership and backend checks; no implicit prelude is imported. Start with the
[catalogue](../stdlib/README.md) and import only what a program uses.

```sh
cargo run -- run examples/stdlib/utilities.rill
cargo run -- run examples/stdlib/options.rill
```

Both examples print `42`. The first combines checked math, a range cursor,
static text spans and caller-owned binary storage. The second uses generic
Result/Option callbacks with Copy values. Imports work outside the source
checkout because the compiler embeds the module sources.

## Option and Result

`std.option` defines `Option<a> = None | Some(a)`. `std.result` retains
`Result<a,e> = Ok(a) | Err(e)`. Both modules are type-only: importing them
adds no allocation, implicit propagation, panic unwrap or default type.
Use `match move` to consume a supported owner payload; ordinary Copy values
can use an ordinary match.

Opt into generic Copy helpers separately:

| Module | Operations |
| --- | --- |
| `std.option_ops` | `is_some`, `is_none`, `unwrap_or`, `unwrap_or_else`, `map`, `and_then`, `or_else` |
| `std.result_ops` | `is_ok`, `is_err`, `unwrap_or`, `unwrap_or_else`, `map`, `map_err`, `and_then`, `or_else`, `ok`, `err` |

Value comes first and callback/fallback second. Option fallbacks take no
arguments; Result fallbacks receive the error. `map` transforms the success
value, `map_err` transforms the error, and `and_then` expects another value of
the same container family. Callback branches run only for the selected variant.
`unwrap_or(value, fallback)` evaluates both arguments before the call, following
ordinary eager evaluation. Use `unwrap_or_else` for a deferred fallback.
`ok`/`err` convert the selected Result variant to Option and discard the other
Copy payload. Import the type module explicitly when naming constructors.

Generic functions in an owner program remain unsupported, including unused
imports. Keep the `_ops` modules out of owner programs. The type-only modules
can still carry supported owners; no helper silently duplicates or drops them.
Importing `std.bytes` or `std.binary` already introduces owner-typed declarations
and selects that subset, even if main never allocates or calls them. Such an
import cannot currently be combined with a generic `_ops` import.
Callbacks with mutable/owner/borrowed captures retain the language's existing
rejection rules. Inference must determine every physically stored payload;
use a closed parameter/result annotation when a constructor leaves the other
Result type unconstrained.

## Checked integer arithmetic

`std.int` operates on `i32`. `min_value()` and `max_value()` return the exact
signed limits. `min(a,b)`, `max(a,b)` and `signum(value)` do not allocate.

`checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`,
`checked_neg` and `checked_abs` return `result.Result<i32,int.Error>`.
`Error = Overflow | DivisionByZero | InvalidBounds`. Guards run before an
invalid operation, including minimum-value negation and `MIN / -1` or
`MIN % -1`. Division truncates toward zero, matching ordinary Rill arithmetic.
`saturating_add`, `saturating_sub` and `saturating_mul` clamp overflow to the
appropriate signed endpoint. Ordinary `+`, `-`, `*`, `/` and `%` keep their
existing trapping behavior.

`clamp(value, lower, upper)` returns a checked result, using inclusive bounds.
Inverted bounds return `Err(InvalidBounds)` rather than being swapped.
These functions do not add unsigned, floating, wrapping or target-sized math.

## ASCII and static text

`std.ascii` treats inputs as integer byte codes and is independent of locale.
Classifiers cover ASCII membership, controls, digits, lower/upper/any letters,
alphanumeric bytes, whitespace, horizontal blanks, graphic/printable bytes,
punctuation and hexadecimal digits. Non-ASCII and out-of-byte-range inputs
return false. `to_lower` and `to_upper` transform ASCII letters and preserve
every other integer. `digit_value` and `hex_value` return `Option<i32>`.

`std.text` operates on immutable program-lifetime `str` bytes:

- `equal`, `compare`, `starts_with`, `ends_with` and `contains` compare exact
  bytes, including embedded NUL. `compare` returns exactly -1, 0 or 1.
- `find(text, needle)` returns the first byte offset as Option; `rfind` returns
  the last. Empty needles match at zero or the text's byte length, respectively.
- `trim_ascii(text)` returns `Span = { start:i32; end:i32 }`, trimming space,
  tab, LF, vertical tab, form feed and CR. All-whitespace input has an empty
  span at the original byte length.
- `valid_span(text, span)` checks ordered, contained byte offsets.
  `span_equal(text, span, expected)` returns false for invalid spans.

Span ends are exclusive. Text spans and the existing `cli.Span` are distinct
nominal types. Spans may split UTF-8 code points; they are byte coordinates,
not new decoded strings. Search is a straightforward byte search with worst
case work proportional to text length times needle length. No Unicode
normalization, case folding, substring allocation or mutable string is implied.

## Owned bytes and binary data

`std.bytes` borrows an existing `SystemBytes`; allocate explicitly with
`std.alloc.bytes_zeroed` or the aborting builtin. The module retains no reference
and does not allocate, resize or transfer ownership.

| Operation | Arguments after the buffer | Result |
| --- | --- | --- |
| `valid_range(read buffer, offset, count)` | Offset/count | Boolean |
| `fill(inout buffer, offset, count, value)` | Byte from 0 through 255 | Checked count |
| `copy(inout destination, dst_offset, read source, src_offset, count)` | Two complete ranges | Checked count |
| `copy_within(inout buffer, src_offset, dst_offset, count)` | Two ranges in one buffer | Checked count |
| `write_str(inout buffer, offset, text)` | Static text, no trailing NUL | Checked byte length |
| `equal(read left, read right)` | Two complete buffers | Boolean |
| `find(read buffer, value)` | Byte from 0 through 255 | Checked Option offset |

Checked operations return canonical `result.Result` with
`bytes.Error = InvalidRange | InvalidByte`. `fill` checks ranges before byte
values; errors preserve every destination byte. Empty ranges at the end are
valid. Negative or excessive ranges reject without overflowing offset arithmetic.
`copy_within` handles overlap in either direction like memmove. Separate-buffer
`copy` follows ordinary read/inout loan checks, which reject using the same root
for both arguments. Use `copy_within` for that case.

`std.binary` reads and writes signed `i32` in exactly four two's-complement
bytes. `read_i32_le(read buffer, offset)` and `read_i32_be` return checked values.
`write_i32_le(inout buffer, offset, value)` and `write_i32_be` return a checked
count of four. All errors are `bytes.InvalidRange`. Offsets need no alignment;
readers leave input unchanged and writers validate the full range before any
write. Byte order is explicit and independent of the host's endian order.

These are buffer operations, not raw-pointer or C aggregate ABI adapters.
Owner allocation failure and trap cleanup retain the existing
[allocation contract](blockers/owned-allocation.md).

## Half-open ranges

`std.range.new(start,end)` constructs `Range = { start:i32; end:i32 }` without
normalizing endpoints. `is_empty(read range)` treats equal or reversed bounds
as empty; `contains(read range,value)` tests the remaining interval.
`next(inout range)` removes the front value and `next_back` removes the back,
returning `Option<i32>`. Empty iteration leaves both endpoints unchanged.
`len_checked(read range)` returns `Result<i32,int.Error>` because the distance
between valid i32 endpoints can exceed the maximum positive i32 value.

The cursor supports the full signed endpoint domain without wrapping. Bounds
are ordinary values, not storage references or a proof that an index remains
valid after collection mutation. There is no implicit iterator trait or loop
desugaring.

## Remaining library work

The existing allocation, JSON, CLI and reload modules retain their documented
contracts. This expansion supplies foundational utilities, not a complete
systems standard library. Growable containers, runtime strings, OS streams,
files, process arguments, networking, time and randomness still need their
compiler, resource and platform contracts. See the
[standard-library blockers](blockers/standard-library.md). No language phase
gate or stable library ABI is established by these additions.
