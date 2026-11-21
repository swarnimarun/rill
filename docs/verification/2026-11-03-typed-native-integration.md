# Integrated checked scalar native checkpoint

The integrated source is `84b9a6bc950364827896672cbdf67aa33c8135af`, which
applies the reviewed F1 implementation `2177a31675c0051888479ee480579e163a0ef49f`
to the previously verified native-oracle checkpoint. The subsequent commit
adds only this verification record and the handoff link.

## Final integrated checks

The complete seven-stage matrix ran against the unchanged, clean source on
3 November 2026, 21:41:54–21:45:10 UTC. Cloud x86_64 Linux used Rust/Cargo
1.99.1, GCC 14.2.0, Clang/libclang 19.1.7 and real SDL3 3.2.10.

- Formatting: passed.
- Full default tests: 567 passed, zero failed or ignored.
- Full `c-import` tests: 601 passed, zero failed or ignored.
- Strict all-target Clippy: passed in both feature configurations.
- Windows GNU all-target compile checks: passed in both configurations.
  These are compile-only, not Windows execution or ABI verification.
- Neither full suite reported a dependency skip.

Commands match the [F1 verification report](2026-11-03-typed-native-calls.md).
The shared temporary build target used two jobs, disabled incremental builds
and omitted dev/test debug symbols. Each stage required at least 2 GiB free
target storage and 3 GiB available memory. No persistent cache deletion or
new toolchain installation was needed.

## Independent review

The exact author patch was independently accepted after source and tested
library identities were reconciled. Eight controlled cases produced 126
GCC/Clang executions at `-O0`, `-O2` and `-O3`, with exact results and event
traces compared against handwritten C and, where representable, uniform C.
Eight forged-IR unit cases and 34 focused integration tests also passed.

The review closed the compound bool-macro initializer regression and made
`_Bool` storage explicit in the conservative ABI temporary bound. The
independent review artifact has SHA256
`c4e534cb2612f26dc27dd95081fd9abf377e2550ff99ec9ddacaa5c812ef0931`.
The author additionally recorded 50 Clang native/CLI/module/game replay tests;
those are separate from the independent run and integrated matrix above.

## Scope and remaining work

Supported scalar native calls, constants and static C-string arguments now
use explicit checked typed operations. Full unused declaration/header checks,
source-order snapshots, once-evaluated captures and owner/native exclusion
remain in place. Evaluation still rejects reached foreign operations rather
than calling host symbols. The [F1 report](2026-11-03-typed-native-calls.md)
details the exact supported forms and oracle limitations.

Real direct and generated SDL integration passed, but still uses the retained
uniform fallback for unsigned/float/raw-pointer transport. Typed raw transport
is the next increment, followed by the full real-SDL transition gate. No new
framebuffer, physical input, hardware rendering, Zig or AArch64 execution is
claimed here. P1, P5 and the wider plan remain open.
