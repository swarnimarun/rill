# Checked Copy foreign transport

## Bounded scope

F2 follows final F1 `2177a31675c0051888479ee480579e163a0ef49f`. Existing
`u8`, `u32`, `u64`, `f32` and exact nullable raw pointers now have explicit
checked typed representations. The existing conversions, null operations,
same-type equality/inequality, closed Copy records/enums, concrete generic
instances, root/field loans and local constant captures use that representation.
No source syntax, host FFI evaluator, foreign aggregate ABI, effect/lifetime
proof, pointer dereference/cast or owner/native mixing is introduced. P1 and P5
remain open, and unrelated unsupported Copy layouts retain the checked uniform
fallback.

Pointer identity includes the exact source pointee spelling and constness.
Target-compatible aliases do not merge. Header-owned underscore typedefs remain
valid; consuming such a C identifier does not declare it. Lexical validation is
shared with source declaration validation, and the Rill-generated `rl_` namespace
is reserved. The deterministic pointer registry is bounded at 4096 identities,
with exact-pair reuse allowed at that limit. Existing instance, type-depth,
shape/capture and 1 MiB estimated-frame policies remain in force.

C uses exact scalar types and private pointer typedefs before aggregate layouts.
A `read ptr<T>` slot is `const rl_cptr_N *`, meaning a pointer to a const pointer
slot, not `const T **`. An `inout` slot is `rl_cptr_N *`. Raw handles remain Copy
values without ownership/provenance or pointee-alias promises. Every primitive
and pointer layout has a target-compiler assertion consistent with the existing
conservative frame estimate. No `restrict` or fast-math flags are introduced.

Calls evaluate value operands into snapshots in written order. Unsigned
conversions check the evaluated i32 input before narrowing; f32 uses the target
C conversion. Header constants are evaluated at the original read, including
once-only generalized captures and unused shapes. Entry and nested printers
preserve full unsigned values, target float formatting and existing pointer
formatting. Tests compare pointer identity through fixture IDs/null/equality,
not nonnull addresses across processes.

The complete native manifest and all header assertions remain present, including
unused declarations and synthetic-main importer validation. IR validation
independently checks pointer IDs throughout signatures, expressions, ADT fields
and arguments, captures and projections. Native argument children use the live
enclosing Rill loan state. All visitors descend into new raw operation operands.

The evaluator never loads host native symbols. Reached calls evaluate their
non-cstr arguments before the native-call error; constants reject at the read.
Conversions, null creation and null tests reject before operand evaluation,
matching the AST reference. Unused declarations and untaken raw branches retain
successful pure paths. CLI native selection is unchanged.

## Oracles and regression coverage

`tests/typed_foreign_raw.rs` uses a controlled header and separate implementation
translation unit. It checks analysis, typed lowering/verification, production
route equality and absence of uniform values. Typed C, checked reference C and
handwritten target-C drivers compare exact exit status, stdout and flushed
C-side traces at `-O0` and `-O2`. GCC/Clang builds treat incompatible-pointer
warnings as errors. Generalized local shapes explicitly use the handwritten C
oracle because the old uniform layout analyzer cannot represent those shapes;
there is no silent oracle failure fallback.

The fixture covers complete unsigned values, f32 rounding/NaN/Infinity/signed
zero, opaque/void/scalar/const pointer identities, header-owned underscore
aliases, generics, nested patterns/printers, mutable roots/fields, forwarded
loans, value snapshots before later slot replacement, pointee aliasing without
slot aliasing, source-order generalized enum/record captures, shadowed constants,
byte/vector/Rill-call conversion children, nine conversion/trap modes, evaluator
error precedence and source/header rejection. Existing F1 static UTF-8/cstr,
scalar order, recursion-depth, native module and importer tests remain required.

Internal forged tests check raw IDs/types in all storage positions, exact
pointer equality/operands, null/conversion shapes, illegal raw operators,
metadata injection, hidden owner children, live enclosing loans and wrong field
projection/mode forwarding. Emission and evaluation must reject with the same
verifier error. Existing direct-header tests now require typed lowering rather
than allowing raw fallback.

Independent source review found an existing uniform-oracle bug: a raw constant
macro containing an unparenthesized comma expanded into extra arguments to
`rl_ffi_f`/`rl_ffi_p`. The regression failed before the narrow `wrap_c` operand
parenthesization fix and passed afterward, preserving one entire header
expression and one read. The typed emitter was already correct. Both float and
pointer comma macros are retained in the regression.

The first full run exposed the old F0 regression that required raw runtime
programs to select byte-identical uniform fallback. That assertion is intentionally
migrated for F2: production emission must equal independently verified typed C,
exclude `rl_value`/`RL_CAP`, and differ from explicit checked reference emission.
The native fixture still executes both implementations against its target-C
oracle. The original failing full-run log is retained; this was a planned route
change, not an unrelated flaky test.

The existing real-SDL direct/generated integration now explicitly checks for
no `rl_value`/`RL_CAP` and builds/runs its three-frame dummy-video variant at
`-O0` and `-O2`, then checks native CLI dispatch. This is not physical-input or
hardware-renderer validation; screenshot recapture and final integrated-source
acceptance remain separate F3 work.

## Verification evidence

Cloud x86_64 Linux, Rust/Cargo 1.99.1, GCC 14.2.0, Clang 19.1.7, SDL3 3.2.10.
Existing cloud toolchain/shared RAM target, two Cargo jobs, dev/test debug
symbols disabled and no incremental compilation. Each command checks at least
2 GiB target-filesystem free and 3 GiB MemAvailable. No installs, new persistent
build caches, cache cleanup or remote writes were performed by this increment.

Author verification used Rust/test checkpoint
`8b421f2649874db97ef3aa7aed5e97a58ee2766b` (final production changes were already
in `59763c1611c98b1a11a0a8e3a4b9a1a1c0e68aac`). The final report/documentation
commit changes no Rust or fixture source. Crate-root timestamps were refreshed
before Cargo so the shared target could not silently reuse another worktree's
package artifacts; build logs identify this checkout explicitly.

- Focused native IR tests: 17 passed.
- Focused raw/F1/direct-header integrations: 43 passed.
- Broader affected unit and integration suites: 287 passed.
- `cargo fmt --all -- --check`: passed.
- `cargo test --locked --no-fail-fast -- --nocapture`: 596 passed.
- `cargo test --locked --features c-import --no-fail-fast -- --nocapture`:
  630 passed.
- Strict all-target Clippy with `-D warnings`: passed both without and with
  `c-import`.
- `cargo check --locked --target x86_64-pc-windows-gnu --all-targets`: passed
  both without and with `c-import`. This is compile evidence, not Windows runtime
  execution.

The feature-built native executables were replayed with an actual `cc` wrapper
that invokes Clang 19.1.7, rather than assuming every fixture honors `CC`. All
93 tests across 11 native suites passed, including selected imports, raw/F1 ABI
oracles and real direct/generated SDL. The replay records executable hashes and
C compiler invocations. It had no ignored tests or explicit skip notices.

Neither full suite had ignored tests or explicit environment-skip notices.
The available real SDL3 library exercised direct and generated bindings,
O0/O2 builds and native CLI runs. The native GCC cases use separate implementation
translation units and the documented exact behavioral oracles.

The earlier bounded pass retained 16 native-boundary unit results and actual
red/green evidence for the comma-macro reference fix. The first full default
failure, retained separately, is the F0 fallback-gate migration described above;
its corrected full rerun passed. No failing assertion was removed to hide a
runtime mismatch.

Independent final-source review, integration with later source-origin work and
final SDL screenshot recapture remain separate acceptance gates. These author
results do not claim P1/P5 completion or hardware-renderer/input validation.
