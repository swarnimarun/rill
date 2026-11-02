# Standard library and Rill JSON

The compiler supports initialized mutable locals, nominal ADTs, loops, immutable
static UTF-8 literals, and fixed-capacity byte values. It does not yet have
general borrowed strings or slices, growable allocator-backed buffers, a package system,
or file and stream I/O. File-based commands support a limited local module
subset; see [file modules](modules.md). `cstr` remains a separate type restricted to direct
foreign-call arguments. The experimental library can parse signed integer documents from runtime
owned bytes, but cannot yet build a general owned JSON tree.

## Available building blocks

Ordinary string literals have static program lifetime, are length-delimited,
and may contain NUL. `byte_len` returns their length as an `i32`; `byte_at`
returns an unsigned byte as `i32` and traps for negative or out-of-range indices.
Lengths larger than `i32::MAX` are rejected. Indices are byte offsets, not
Unicode scalar positions. Strings are copied as values referring to immutable
static storage, including when nested inside an ADT. The interpreter and C
backend have differential tests for bounds, embedded NUL, UTF-8 and nesting.
They do not implement general borrowed string lifetimes or runtime decoding.

`bytes_new(N)` creates a zero-filled value with a literal capacity from 0 to
256. `bytes_len` and `bytes_get` inspect it; `bytes_set` returns an updated copy
and checks the index and byte range. This is a fixed-size value, not a growable
buffer, aliasable place, allocator interface, or output stream. Explicit buffer
size and copying costs matter. Out-of-bounds access traps; it is the caller's
responsibility to check capacity before writing when a recoverable error is
needed.

`system_bytes_zeroed_abort(length)` provides a separate, fixed-length owned
buffer with runtime length, explicit system allocation and scoped cleanup.
It is non-Copy and usable only in the restricted owner subset. The
[owned JSON writer](../../examples/owned/README.md) borrows this buffer as an
explicit output destination. Its `write_string(inout output, text, offset)`
returns a byte count or `-1` for invalid offsets or insufficient capacity,
checking the full encoded length before writing. Returned errors leave every
byte unchanged. Static UTF-8 input, JSON escaping, arbitrary supported buffer
lengths and caller-selected offsets work in the interpreter and C. This is a
local example module with an integer result convention, not a stable standard
library API. `std.alloc.bytes_zeroed` provides recoverable allocation of the
same fixed-length owner through an ordinary `Result`. The buffer does not
provide growth, borrowed slices or I/O; see [owned allocation](owned-allocation.md).

[The JSON example](../../examples/json/README.md) validates JSON syntax in a
static input and demonstrates decimal integer serialization into a bounded byte
value. It does not build an owned JSON tree, read files, or implement a general
JSON serializer. The example documents its nesting and Unicode escape limits.

## Experimental reusable modules

[`stdlib`](../../stdlib/README.md) contains caller-allocated JSON writers for
static strings, full-range `i32`, booleans and null, plus a parser for complete
runtime owned-byte `i32` documents. Every writer checks capacity before changing
any byte. The parser rejects malformed integer syntax, overflow and trailing
data without trapping. It accepts JSON whitespace and `-0`; fractions and
exponents are outside this integer-specific API.

The input API returns `std.result.Result<i32, ParseError>` from `parse_i32`.
Errors distinguish a missing digit, leading zero, overflow and trailing data,
with the offending byte offset. `i32_valid` and `parse_i32_or` remain convenience
wrappers. Input may contain arbitrary bytes; parsing does not assume UTF-8 or
change caller storage. Differential tests check both backends at `-O0` and
`-O2`, including signed boundaries, all single-byte inputs, overflow and
serializer/parser round trips.

Import `std.json_input`, `std.json_output`, `std.result` or `std.alloc` explicitly. Their
sources are bundled in the compiler, so no file copying is required. The `std`
namespace is reserved for that catalogue. This does not provide a package
manager, canonical interface versions or a stable standard-library release.

## Remaining work

1. Add input APIs with checked lengths, explicit I/O authority and error
   handling. General borrowed `Str<r>` and byte slices require provenance and
   lifetime checks; UTF-8 validation must not trust arbitrary foreign bytes.
2. Generalize the owned JSON string writer's explicit destination and atomic
   capacity failure to more serializers and ordinary typed results. The current
   example has a call-scoped exclusive owner loan; general bounded destinations
   still need P2 initialized-place and loan rules. A stream needs an
   I/O capability and failure behavior. Check arithmetic before every write and
   never use `strlen` on length-delimited values.
3. Implement allocator-aware owned `String` and `Vec`, allocator capabilities
   beyond the recoverable system box/byte operations, and the remaining P2/P3
   ownership and cleanup rules. Ordinary finite `Result`
   values can now carry owners. A recursive JSON
   tree needs explicit indirection and allocator ownership rather than an
   infinitely sized ADT or hidden allocation.
4. Extend the file-module subset with packages, interfaces and compatibility
   guarantees for a reusable standard library. Define JSON numeric
   representation, duplicate-key policy, Unicode escape and surrogate policy,
   and error locations before promising a general parser API.
5. Compare parsing and serialization against independently generated fixtures,
   including malformed UTF-8, escape errors, unpaired surrogates, number
   boundaries, nesting, duplicate keys, output exhaustion and round trips.
   Run interpreter/C comparisons at multiple optimization levels.

The initial `u8x16` subset has a scalar fallback and an explicitly selected
NEON path; broader SIMD and runtime capability selection remain open. See
[SIMD limitations](simd.md). These steps do not by themselves complete P2/P3
or make the language stable.
