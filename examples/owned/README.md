# Owned allocations

This example allocates one hosted system box, reads and updates its `i32`,
then moves ownership to a second binding. The result is `42`.

```sh
cargo run -- run examples/owned/box.rill
cargo run -- build examples/owned/box.rill -o /tmp/rill-owned-box
/tmp/rill-owned-box
cargo run -- run examples/owned/transfer.rill
cargo run -- run examples/owned/borrow.rill
cargo run -- run examples/owned/bytes.rill
```

Normal scope exit frees the live box once. Reading or moving the old binding
after the move is rejected. Allocation failure traps; traps do not guarantee
Rill cleanup. This is a narrow hosted example, not a general allocator or an
owned container API. See [owned allocation limitations](../../docs/blockers/owned-allocation.md).

`transfer.rill` passes the box to a function with `consume(move resource)`;
the callee owns and drops it. `borrow.rill` passes the same allocation through
shared and exclusive borrows without transferring ownership. All three
box examples print `42`. `bytes.rill` allocates a runtime-sized zero-filled
buffer, writes the first byte through an exclusive borrow and reads it through
a shared borrow; it prints `65`.

`json_output.rill` exports `write_string(inout output, text, offset)`. The caller
provides an owned byte buffer and a static UTF-8 input string. Success returns
the number of JSON bytes written, including the opening and closing quotes.
There is no trailing NUL. Prefix and suffix bytes outside the written range stay
unchanged. A negative offset, an offset beyond the destination, or insufficient
capacity returns `-1` and leaves every destination byte unchanged. This result
is the example's integer convention; ordinary `Result` values are also available.

The writer first checks capacity without writing or allocating. It emits quotes
and backslashes with a backslash prefix, control bytes as `\u0000` through `\u001f`
escapes, and other UTF-8 bytes unchanged. The caller can append by advancing the
offset by the successful byte count. Check for `-1` before advancing. Input is
still a static string; this does not provide runtime text input or owned strings.
The no-write guarantee covers a returned capacity error. Runtime traps do not
promise recovery or cleanup.

```sh
cargo run -- run examples/owned/json.rill
cargo run -- build examples/owned/json.rill -o /tmp/rill-owned-json
/tmp/rill-owned-json # prints 15, the number of bytes written
```

`tests/owned_json_output.rs` checks escaping, UTF-8, all control bytes, output
larger than the fixed-buffer limit, exact capacity, unchanged failure buffers,
invalid offsets and repeated writes through the interpreter and C at `-O0` and
`-O2`.

`return.rill` allocates and initializes an owned byte buffer in a helper, then
returns it with `move output`. `forward` passes the returned owner through a
tail call. The caller reads `42` and frees the buffer on normal scope exit.
The callee releases its other live locals before resuming the caller; the
returned allocation remains live. Returned owners can initialize locals or
enum fields. Borrowed owner returns and implicit owner copies remain unsupported.

```sh
cargo run -- run examples/owned/return.rill
cargo run -- build examples/owned/return.rill -o /tmp/rill-returned-owner
/tmp/rill-returned-owner # prints 42
```

`result.rill` returns `Ok(move owner)` or `Err(code)` from a factory and
consumes the result with `match move result`. The success arm reads the box;
its payload is freed once at arm exit. No allocation is added for the enum.
Both arms of an `if` or `match` must leave enclosing owners in the same state.

```sh
cargo run -- run examples/owned/result.rill
cargo run -- build examples/owned/result.rill -o /tmp/rill-owned-result
/tmp/rill-owned-result # prints 42
```

For recoverable allocation, `fallible.rill` imports `std.alloc` and `std.result`,
then consumes `alloc.box_i32(42)` with `match move`. Success prints `42`;
`InvalidLength`, `SizeOverflow` and `OutOfMemory` are ordinary error variants.
`alloc.bytes_zeroed(length)` follows the same Result/cleanup rules. The older
examples deliberately retain the trapping `_abort` policy.

```sh
cargo run -- run examples/owned/fallible.rill
cargo run -- build examples/owned/fallible.rill -o /tmp/rill-fallible
/tmp/rill-fallible
```

See [the library API](../../stdlib/README.md) for a braced function example and
exact result types. Fallible allocation does not add general unwinding,
custom allocators or growable storage.
