# Checked scalar native calls and constants

## Scope

F1 is the first native-operation increment on base
`2b48052857bf846d795fa7c0b08a7d8329ce99f2`, following the checked uniform C
oracle and shared ABI-declaration extraction. Existing native calls with
`i32`/`bool` value parameters, direct static `cstr` literals and `i32`/`bool`/`unit`
results now lower to explicit typed operations. Existing `i32`/`bool` header
constant reads lower too. The surrounding Copy program must still have a
supported concrete layout. These programs emit typed C without `rl_value` or
`RL_CAP`; this does not complete P1, P5 or the complete verified MIR boundary.

The owned manifest retains complete source configuration and all resolved
foreign declarations, including unused unsigned/float/pointer forms. Independent
verification rechecks configuration, declaration syntax, unique names, native
identities, arity, exact operand/result types, literal NUL exclusion and current
enclosing loans. Shared ABI emission preserves every unused header assertion.
Header compatibility remains the selected target compiler's job, not a fact
established by IR verification or library emission.

Scalar arguments are evaluated in source order into snapshots. ABI `_Bool`
parameters/results convert explicitly to/from internal typed bools. Unit calls
execute once. C strings use static octal-escaped UTF-8 literals, preserving
retained-pointer lifetime and exact NUL termination. Header constant expressions
are read at their original evaluation position. Constant leaves in generalized
constructor/record initializers are captured once, including aliases and unused
shapes; no purity claim or common-subexpression elimination is introduced.

The evaluator does not load or call host native symbols. Reached foreign calls
evaluate non-cstr arguments left-to-right before `E-INTERP-FFI`; earlier traps
and reached nested-native errors retain precedence. Reached constants report
`E-FFI`. Unused declarations, headers and untaken foreign branches can still
have a successful pure path. CLI native selection and target/link configuration
are unchanged.

Owner/foreign exclusion is independently checked at source and IR boundaries.
Headers and constants cannot silently enable owner/native mixing. Link/package
flags alone keep the existing owner behavior. Runtime raw unsigned/float/pointer
forms remain whole-program checked uniform fallback until F2. Invalid typed IR
never selects that fallback. There are no new source forms, effects/lifetime
proofs, C aggregate ABIs, callbacks, C-callable Rill exports or host FFI engine.

## Test contracts and oracle limits

`tests/typed_foreign.rs` uses a controlled header and separate C implementation
translation unit. Ordinary scalar-native cases compare typed C against the F0
checked uniform emitter at `-O0` and `-O2`, checking status, exact stdout and
flushed C-side stderr traces. Handwritten C drivers independently exercise ABI
round trips, `_Bool`, source ordering, static strings, snapshots and constants.
An unparenthesized bool comma macro reproduces the independent review finding:
its entire initializer must be parenthesized to remain one expression and one
read, including in a generalized capture. The regression failed before that
fix and passed after it. ABI `_Bool` temporaries are also explicitly covered
by the existing 64-byte target-layout bound used in frame estimates.
The fixture builds with `-std=c11 -pedantic-errors -Werror`.

Generalized/unused local-shape cases explicitly use handwritten C rather
than the uniform oracle: that older layout analyzer rejects their unresolved
polymorphic initializer shapes with `E-C-ADT`. Typed parse/check/lower/verify,
default-route equality, no-uniform representation, exact expected output/trace
and C-driver parity remain required. No marker syntax or silent oracle-error
fallback is used. The ordinary monomorphic constant snapshot still compares
both compiler backends.

Eight internal native-boundary tests mutate IR metadata/nodes and require
verification, emission and evaluation to reject identically. They cover invalid
IDs, signatures, operand kinds, NUL, incorrect bool/int types, malformed and
duplicate symbols/parameter names, configuration injection, missing constant
headers, unsupported raw runtime nodes, inactive owner/native mixtures and
native argument expressions accessing live enclosing Rill loans. An unused
full-ABI manifest and dependency-only owner case are also checked.

## Verification environment and results

Cloud x86_64 Linux, Rust/Cargo 1.99.1, GCC 14.2.0, Clang 19.1.7 and SDL3 3.2.10.
The existing cloud toolchain and shared RAM Cargo target were reused with two
build jobs, no dev/test debug symbols and incremental compilation disabled.
Commands require at least 2 GiB free on the RAM filesystem and 3 GiB available
memory before starting. No installs, persistent build caches or remote writes
were made for F1.

Final source checks after the comma-macro fix and `_Bool` layout assertion:

- Full default suite: **567 passed**, zero failures or ignored tests.
- Full `c-import` suite: **601 passed**, zero failures or ignored tests.
- Strict all-target Clippy: passed with default features and with `c-import`.
- `x86_64-pc-windows-gnu` all-target compile checks: passed with both feature
  configurations. These are compile-only, not Windows execution or ABI evidence.
- Formatting and `git diff --check`: passed.
- Clang replay of final native/reference/CLI/module/game executables: **50
  passed** across `backend_reference`, `c_import_direct`, `ffi`, `native_cli`,
  `native_modules`, `typed_foreign`, `sdl3_direct`, `sdl3_game` and `raylib_game`.
  A `cc` wrapper selected actual Clang 19.1.7 for their C compiler invocations.
  The new native fixture passed its `-O0`/`-O2` comparisons with both GCC and
  Clang; existing raw-value tests also retain their `-O3` checks.

Neither aggregate nor the Clang native replay reported a dependency skip.
Actual direct and generated three-frame SDL3 builds/runs succeeded against
SDL3 3.2.10. They still use F1's uniform fallback for raw foreign transport;
this is preserved native integration, not evidence of typed raw-pointer support.
Collector/raylib gameplay suites use their existing C stubs. No new framebuffer
capture, physical input or hardware-renderer validation was performed here.

Commands for the final Rust/test matrix:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --nocapture
cargo test --locked --features c-import --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --features c-import --all-targets -- -D warnings
cargo check --locked --target x86_64-pc-windows-gnu --all-targets
cargo check --locked --features c-import --target x86_64-pc-windows-gnu --all-targets
git diff --check
```

Clang replay used the already-built final importer-enabled test executables,
with only `cc` selection changed; it did not rebuild Rust under a different
configuration. The source/test fingerprint was checked unchanged after the
matrix. No Zig executable was available and no Zig or AArch64 NEON execution
was performed. The x86_64 unsupported-NEON rejection remains covered.

The macro issue was found during independent source review and its red/green
regression is retained. Independent final replay and integration acceptance
remain separate from this implementation's verification. The bounded F1
implementation does not close the raw-transport F2 or whole-transition F3 gates.
