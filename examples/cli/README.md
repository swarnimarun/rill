# CLI token parsing

```sh
cargo run -- check examples/cli/main.rill
cargo run -- run examples/cli/main.rill
cargo run -- run examples/cli/main.rill --interpret
```

The example prints `42`. `std.cli` classifies one supplied string token and
returns byte spans for its name and value, then parses a checked signed decimal.
It demonstrates ordinary records, enums, exhaustive matching and `Result`.

The module does not read process arguments. The current string type describes
program-lifetime UTF-8 strings; the example supplies a literal. A future runtime
argument adapter must establish that storage contract rather than reinterpret
an arbitrary foreign pointer as a checked string.

Call `classify(token, true)` while parsing options. After `EndOptions` (`--`),
pass `false` for subsequent tokens so even `--name` becomes positional. Long
options use an ASCII letter followed by ASCII letters, digits, `_` or `-`.
`--count=42` yields separate name/value spans; `--count=` yields an empty value.
`-h` yields its ASCII byte. Short clusters and malformed options return
`InvalidOption`; leading negative decimals such as `-42` stay positional.
The caller decides which options exist, whether a following token supplies a
value, and how to handle duplicate options or subcommands.
