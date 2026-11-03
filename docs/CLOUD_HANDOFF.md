# Continue the v1 checkpoint

`v1` is an experimental implementation checkpoint, not a stable language
release. The original checkpoint was paused, then resumed at the user's
request on 3 November 2026. The cloud continuation targets `v1-ernie`, leaving
`v1` and the unrelated `main` lineage unchanged. See the
[continuation plan](CONTINUATION_PLAN.md) and
[cloud verification](verification/2026-11-03-selected-c-import.md) for the new
selected-C-import increment and remaining work. The subsequent
[CLI worker increment](verification/2026-11-03-import-worker.md) adds Unix
timeout/cancellation, bounded IPC and process-group cleanup while keeping
the library API in-process.

The subsequent [typed-pattern increment](verification/2026-11-03-typed-copy-patterns.md)
brings concrete Copy enums and primitive/nested patterns onto the shared checked
typed interpreter/C path, including Copy matches inside restricted owner
programs. The [generic-instance increment](verification/2026-11-03-typed-specialization.md)
adds reachable concrete function specialization to that same path. The
[root-loan increment](verification/2026-11-03-typed-copy-loans.md) adds shared
and exclusive Copy root loans and computed read temporaries. The
[Copy-record increment](verification/2026-11-03-typed-copy-records.md) adds
compact finite record products, construction, field reads/assignment and
whole-record root loans. The [field-loan increment](verification/2026-11-03-typed-field-loans.md)
adds independently checked projected operands, prefix-overlap intervals and
original-slot forwarding. The [Copy-vector increment](verification/2026-11-03-typed-vectors.md)
migrates the eight existing `u8x16` operations, retaining scalar/opt-in NEON
behavior and adding no vector semantics. The initialization/drop-tree foundation
is present; accepted owner-record lowering still needs further work.
The [local-value increment](verification/2026-11-03-typed-local-values.md) adds
bounded capture-and-shape elaboration for eligible immutable schemes. Concrete
leaves are evaluated once into private typed capture records; aliases share
captures and each whole/field use has an exact concrete type. Unused phantom
shapes need no default type. This does not broaden generalization or ownership.
The [checked-native increment](verification/2026-11-03-typed-native-integration.md)
adds explicit typed scalar native calls/constants and static C-string arguments,
with complete declaration checks and source-order captures. Raw foreign
transport still uses the checked uniform fallback; P1 and P5 remain open.

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
SDL3 3.4.18 on AArch64 macOS. Other hosts need their own verification. The original checkpoint had no third-party crate dependencies; the optional
`c-import` feature now uses a pinned runtime-loaded libclang binding. SDL3 development files and pkg-config
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
programmer's responsibility. An optional `c-import` feature now imports explicitly selected declarations
through libclang; see the [current interop workflow](c-interop.md#import-selected-declarations)
and [cloud verification](verification/2026-11-03-selected-c-import.md).

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

## Reconstructed source-tree increment

After cloud filesystem loss, the grammar-tree work was reconstructed from the
restored baseline, not recovered from the missing old commit. Fresh checks are
recorded in [grammar-tree verification](verification/2026-11-03-grammar-source-tree.md).
The existing parser now records lossless grammar nodes and bounded top-level
recovery with structured diagnostics. Declaration maps require an error-free
parse. Resolved/IR mapping, finer recovery and P1 acceptance remain open.

The subsequent reconstructed [AST origin pass](ast-source-origins.md) covers all
parsed structural node families, including explicit block/unit/unary/record
provenance. It is optional and cannot alter strict parse success. Typed/generated-
code mappings still need separate work; module preservation follows below.

The next [module origin increment](module-source-origins.md) adds an opt-in
owned result for the exact flattened program, preserving structural origins
through the existing resolver/rewriter and all compatible native contributors.
Ordinary loading/checking and errors are unchanged. Typed/backend mapping and
source-aware checker diagnostics remain open.
