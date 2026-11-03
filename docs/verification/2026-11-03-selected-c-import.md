# Cloud verification for selected C import

This work continues the experimental `v1` checkpoint, commit
`4a1014e0c4b9b28434c1506cf32ce90d47f87ec2`, on `v1-ernie`. It does not establish
completion of P1–P11 or a stable language, ABI, or foreign safety contract.

## Environment

- Host: x86_64 Linux, Debian 13.
- Rust/Cargo: 1.99.1, stable; rustc commit `58cf8f9c35b355199727627d07ae34a4e2846d67`.
- Default C compiler: GCC 14.2.0.
- Import tooling: Clang/libclang 19.1.7, official Debian packages.
- Rust and Clang were installed in an isolated cloud workspace. The user's
  laptop is not needed for this work.
- The initial baseline had no SDL3 development files or Zig. SDL3 was installed
  later for the real-library verification below. AArch64 execution and Zig
  remain unavailable on this host.

## Pristine checkpoint baseline

The following ran against a separate, unmodified copy of the checkpoint:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

Formatting and strict Clippy passed. The full test runner reported **287
passed, zero failed, zero ignored**. One nominal pass is the SDL3 test's
explicit dependency skip, not a real-library execution result. Running that
test with `--nocapture` confirmed the missing SDL3 development dependency.

Both `examples/blocks/main.rill` and `examples/owned/fallible.rill` printed
`42` through the interpreter and native C execution. The x86_64 test rejecting
an explicitly requested unsupported NEON target passed. It is not evidence of
working AArch64 NEON output. A Clang smoke fixture using `stddef.h` and
`stdint.h` compiled and ran successfully.

## Implementation verification

The final source and test changes passed:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
```

- Default build: **289 passed, zero failed, zero ignored** reported by Rust's
  test runner. One nominal pass is the explicit direct-SDL3 dependency skip.
- Importer-enabled build: **307 passed, zero failed, zero ignored** reported by
  the test runner. Two nominal passes are the direct/generated SDL3 dependency
  skips; neither establishes real SDL3 execution on this host.
- All **16 selected-import integration tests** ran and passed. Generated
  modules made actual C fixture calls at both `-O0` and `-O2`.
- Formatting and strict all-target Clippy passed for both build configurations.
- Both smoke examples again printed `42` in interpreter and native execution.
- Independent review and additional CLI probes checked exact scalar kinds,
  opaque pointer identity and typedef-hidden constness, unsupported signatures,
  preprocessing configuration, frontend/target disagreement, diagnostics,
  paths with spaces, and preservation of existing output on failures.
- Diff/whitespace checks passed. No target directory, downloaded toolchain,
  cache, temporary executable or machine-specific installation path is included
  in the change.

The tested importer backend is libclang 19 on x86_64 Linux with GCC 14.2 as the
final C validator. The implementation requires libclang 12 or newer, but this
run does not establish a multi-version or multi-target conformance matrix.
Generated modules require the same target, sysroot, defines and include paths
on subsequent builds. The importer does not prove foreign lifetimes/effects,
provide aggregate/callback ABI adapters. At this first-increment checkpoint it
also did not isolate libclang; the subsequent [worker verification](2026-11-03-import-worker.md)
records the Unix CLI process boundary and deadline added afterward. Target/cache
persistence, broader ABI coverage and foreign safety remain continuation work.

## Subsequent real SDL3 regression

After installing SDL3 3.2.10 in the cloud workspace, the full importer-enabled
suite ran again with `--nocapture`: **307 passed, zero failed, zero ignored, and
zero dependency skips**. The direct and generated-binding SDL3 tests executed
against the actual library. [Native SDL3 verification](2026-11-03-sdl3-native.md)
records the separate GCC/Clang checks and genuine headless framebuffer captures.
