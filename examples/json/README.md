# JSON syntax validator

For a reusable parser of signed integer documents from runtime bytes, see
[the bundled JSON library](../../stdlib/README.md) and
[`result.rill`](result.rill). That example returns an ordinary `Result` with
typed error offsets and runs with `cargo run -- run examples/json/result.rill`.

`validator.rill` is a Rill implementation of a bounded JSON syntax validator.
Its sample `main` validates a static string and prints `true`:

```sh
cargo run -- run examples/json/validator.rill
cargo run -- build examples/json/validator.rill -o /tmp/rill-json
/tmp/rill-json
```

It recognizes JSON literals, number syntax, strings and escapes, arrays,
objects, and JSON whitespace. It requires the entire input to match and limits
container nesting to eight levels. Raw control bytes in JSON strings are
rejected. `\u` escapes must have four hexadecimal digits; the validator checks
escape syntax only and does not check whether UTF-16 surrogates are paired.
Duplicate object keys are accepted. The source string is valid UTF-8 by
construction, so the example does not accept arbitrary bytes or diagnose invalid
UTF-8.

This example returns a Boolean; it is not a JSON value parser, serializer, file
reader, or reusable library. Input is currently a static source literal because
Rill does not yet have ordinary runtime input or owned strings. The nesting
limit bounds recursive parser calls and is intentionally small for the current
interpreter. No C function performs JSON parsing.

`cargo test --test json_validator` checks accepted and rejected inputs and
compares representative interpreter and C results at `-O0` and `-O2`.

The same validator is also provided as an exported local module, with a separate
consumer. Run it from the repository root with:

```sh
cargo run -- run examples/json/modules/main.rill
cargo run -- build examples/json/modules/main.rill -o /tmp/rill-json-module
/tmp/rill-json-module
cargo test --test json_module
```

The module example is a copy of the standalone validator; the module test checks
that their implementations stay in sync. Both retain the limitations above.

## Unsigned decimal serialization

`unsigned_decimal.rill` demonstrates writing a nonnegative `i32` as ASCII
base-10 bytes in Rill. It is a bounded integer serializer, not a JSON object or
string serializer. Its fixed 11-byte result stores the first digit's offset in
byte 0 and right-aligns the digits in bytes 1 through 10. Earlier bytes are
zero. For negative inputs, byte 0 is 255 and the remaining bytes are zero. A
consumer must use the offset; the result is not NUL-terminated text.

```sh
cargo run -- run examples/json/unsigned_decimal.rill
cargo test --test unsigned_decimal
```

`bytes_set` returns an updated copy of the fixed-capacity value. The tests
check zero, digit-count boundaries, `i32::MAX`, negative inputs, decoding, and
interpreter/C agreement at `-O0` and `-O2`. A general serializer still needs an
output API that reports capacity exhaustion and can return the written length.

## JSON string serialization

`string_serializer.rill` serializes one static UTF-8 string as a JSON string.
It escapes quotation marks and backslashes and writes all control bytes as
`\u00xx`; other UTF-8 bytes are preserved. It does not serialize arrays,
objects, numbers or arbitrary runtime bytes.

The `JsonWritten` result contains a fixed 256-byte value. Slot 0 stores the JSON
text length, and slots 1 through that length contain the JSON text, including
the opening and closing quotes. The remaining bytes are zero and are not part
of the result. The largest output is 255 bytes. If the escaped string would
exceed that capacity, the result is `JsonTooLong` and no partial output is
returned. `bytes_set` copies the buffer each time; this example favors a clear
bounded contract over performance.

```sh
cargo run -- run examples/json/string_serializer.rill
cargo test --test json_string_serializer
```

The tests check independent expected strings and round trips, all control bytes,
UTF-8, and 255-byte and 256-byte output boundaries. Representative cases also
compare the interpreter with generated C at `-O0` and `-O2`.
