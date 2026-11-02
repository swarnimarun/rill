# Continue the v1 checkpoint

`v1` is an experimental implementation checkpoint, not a stable language
release. Implementation was paused at the user's request after this checkpoint
was prepared. Resume only when asked to continue.

## Open the checkpoint

```sh
git clone --branch v1 git@github.com:swarnimarun/rill.git
cd rill
cargo build --locked
cargo run -- run examples/blocks/main.rill
```

The local project uses Jujutsu. The `v1` bookmark is published as a Git branch,
so a cloud workspace needs only Git. This checkpoint has a separate lineage
from the repository's existing `main`. Continue from `v1`; do not assume it is
an incremental patch on `main` or rewrite the other branch.

Use a recent stable Rust toolchain and a C11 compiler available as `cc`. This
checkpoint was exercised with Rust/Cargo 1.99.1, Apple Clang 17, Zig 0.17.0 and
SDL3 3.4.18 on AArch64 macOS. Other hosts need their own verification. Cargo
has no third-party crate dependencies. SDL3 development files and pkg-config
are optional for ordinary examples, required for the real SDL3 integration.
Zig is optional; writable Zig caches may need configuring in a cloud sandbox.

## Read first

1. [PLAN.md](../PLAN.md), including its implementation amendments and section
   19 phase gates, remains the specification.
2. [Phase status](phase-status.md) records implemented subsets and open gates.
3. [README](../README.md) describes accepted syntax and CLI commands.
4. [C interoperability](c-interop.md), [library APIs](../stdlib/README.md), and
   [blockers](blockers/remaining-phases.md) describe current contracts.
5. [Continuation plan](CONTINUATION_PLAN.md) gives the next tasks and checks.

## What is available

The Rust compiler has braced functions/blocks, semicolon statements, inference,
mutable locals, records, concrete generic ADTs, exhaustive matching, a reference
interpreter, and C11 output. The restricted owner path supports hosted boxes,
fixed-length owned bytes, finite owner-bearing enums, moves, call-scoped
borrows, owner returns and scoped cleanup. `std.alloc` exposes recoverable
allocation through ordinary `Result` values. Bundled JSON modules parse i32
documents and serialize into caller-provided byte storage. SIMD has scalar
`u8x16` operations and opt-in AArch64 NEON.

Source `@include`, `@link` and `@pkg_config` directives drive native checking,
building and execution. Reusable binding modules export foreign functions and
constants. The target C compiler verifies explicit signatures against headers;
raw pointer lifetime, resource destruction and foreign effects remain the
programmer's responsibility. Automatic declaration import is not implemented.

The [SDL3 game](../examples/sdl3-direct/README.md) imports its bindings from
`sdl.rill` and calls the real system library without a C shim:

```sh
cargo run -- check examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill --zig-cc
```

## Verification at the checkpoint

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

A full regression run passed 287 tests. A subsequent declaration-validation
fix added one regression; the final affected-component run passed 106 tests:

```sh
cargo test --lib --test native_modules --test ffi --test module_compile --test sdl3_direct
```

Formatting and strict all-target Clippy passed after the fix. Independent
reviews checked native ABI handling, parser scopes, allocation-failure cleanup,
native modules and the final validation fix. The real three-frame SDL3 variant
ran successfully under the dummy video driver through both `cc` and `zig cc`.
Tests explicitly skip that integration when SDL3 development files are absent;
a skip is not evidence of a working SDL3 installation. Physical desktop input
was not simulated.

Two reviewed regressions must remain covered: source `@link` must not duplicate
the library already supplied by pkg-config, which broke Zig-built dylib loading
on macOS; compatible duplicate extern declarations must each be validated
before coalescing, so later duplicate parameter names cannot evade checking.

Do not claim the entire plan, full memory safety, effect system or stable ABI
is complete. Restricted owner programs still reject foreign declarations.
There are no pending source edits needed to repair a known failing check.
Clang can emit a cosmetic `-x none` warning for a library-only link command.

## Suggested skills and agent setup

Use `plan` and `delegate` for phase implementation, `verify` before reporting
success, and `handoff` when moving environments again, if those skills are
available. The user requested GPT-6.1 Sol for delegated work. Preserve the
project's Rust/C direction and record genuine blockers under `docs/blockers/`.
