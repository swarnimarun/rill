# Systems ML checkpoint verification

The systems-ml branch uses the empty jj root as its parent. The reference
compiler and earlier regression corpus came from v1 source 951c9f38. To audit
the implementation delta, compare the two **trees** directly with
`jj diff --from v1 --to systems-ml`. Phase completion still follows PLAN §19.

## What ran

* The frontend suite checked conditional-pattern equivalence to explicit
  matches, legacy let-in syntax and CST roles, lexical binding scopes, malformed
  inputs, UTF-16 quick fixes and recovery before trailing comments. Deep
  malformed cases were run on a 2 MiB stack. The Tree-sitter corpus, generated
  parser and editor queries were checked with generator version 0.27.1.
* The [independent models](../type-models.md) cover 240 compiler inputs.
  Their predicted accepted/rejected split is 56/184. Accepted cases ran
  through the interpreter and batched generated C at O0/O3. Separate tests
  exercised typed and reference execution for fixed-capacity byte signatures,
  records, nominal compatibility and capacity errors.
* Lean 3.51.0 checked the [Option calculus](../option-core-proof.md) with five `no axioms` reports and no diagnostics. The proof covers the specified closed core. The Rust regressions exercised representative source programs and optimized C output.
* The scene graph ran under both interpreter and native C, with nested
  transforms, capacity failures and hit-testing. SDL's dummy driver and
  software renderer exercised actual queued input, framebuffer readback,
  startup/presentation failures and shutdown. The [modal editor test](../../tests/modal_editor.rs)
  drove real file edits, bounds, save permissions and external replacement.
* The database tests rebuilt and reopened replicas, compared complete file
  bytes, rejected conflicting revisions and scanned 100,000 rows. An independent
  optimized GCC run of the included benchmark replicated the last 100,000 rows
  of a 4,000,000-row file in a median 0.019 seconds over five runs, including
  process startup and close. Setup and byte verification were outside the
  timing. This measurement used a local pipe and buffered storage.
* The generated language manual passed its local audit with 26 HTML pages,
  1,926 links and 197 search addresses.

## Native environment and preliminary failures

The final native matrix used x86-64 Linux, Rust 1.94.1, GCC 14.3, libclang
22.1.8 and SDL 3.4.10. On Nix, select the GCC toolchain and pkg-config on PATH,
set PKG_CONFIG_PATH to the SDL development package's pkgconfig directory,
LIBCLANG_PATH to the libclang library directory, RILL_TEST_CLANG_RESOURCE_DIR
to the matching Clang resource directory, and C_INCLUDE_PATH to the target
libc headers. Normal platform installations may supply these defaults.

An initial suite used the system's wrapped Clang. It failed on linker-only
flags injected into syntax checks, a custom Clang resource directory overridden
by the wrapper and non-UTF-8 include paths. The explicit resource-directory
test was rerun successfully with Clang honoring the supplied resource path.
The first suite also observed the modal editor's new syntax with a compiler
binary built before fixed-capacity annotations were added. Fresh corpus checks
parse and format the file successfully. The watcher test now resolves its sleep
executable through PATH, since this host has no /bin/sleep. A configured SDL
build revealed the SDK's ELF --enable-new-dtags flag. Exact recognition of this
flag preserves the package linker sequence and rejects combined unknown options.

A later overlapping minimal-feature CLI test replaced the Cargo debug executable while the default suite was still running. The resulting native import failures explicitly reported a missing `c-import` feature. The final default suite ran by itself and passed in full. Run these two feature configurations sequentially or give them separate Cargo target directories.

See [the roadmap](../systems-ml-roadmap.md) and [remaining gates](../blockers/systems-ml.md)
for proof coverage, allocation and application limitations.

## Integrated results

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --show-output
cargo clippy --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

Formatting and both strict Clippy commands passed. The full Cargo run exited
successfully with **1,517 reported passes, zero failures and zero ignored**
across 159 test result groups. Five passing functions printed capability skips.
Three GPU tests lacked a native video device and the opt-in 256-cubed GPU test
was disabled. GCC skipped the Clang resource-directory check, which subsequently
passed with Clang. Real SDL3 direct, generated-binding, configured build and
software framebuffer tests ran. The Zig SDL3 build ran against the real SDK.
AArch64 NEON execution requires a different host.

A separate minimal-feature selection ran 15 tests in nine targets, including
the three applications, conditional patterns, fixed-byte annotations, operand
diagnostics, the finite models and the executable proof regressions. All 15
passed. A further minimal-feature run passed seven kernel library tests and the source-to-LLVM CLI test. These are overlapping workloads with the full default matrix.

## LLVM device targets

The normal `rill kernel lower` command read [clamp.rill](../../examples/gpu-ir/clamp.rill). The CPU IR evaluator returned `0, 0, 0, 7, 2147483647` for inputs `i32::MIN, -1, 0, 7, i32::MAX`. Source-derived and handwritten Rill IR emitted byte-identical LLVM modules on each target. Source tests compare three maps at 27 signed observations with the checked and reference source interpreters. They cover the 256/257-definition boundary and errors for effects and latent traps.

| Path | Artifact and observed execution |
| --- | --- |
| CUDA | LLVM 21.1.8, PTX 8.7, `sm_120`, required 64-thread block size. The [CUDA harness](../../examples/gpu-ir/check_cuda.py) ran the source-derived map on a GeForce RTX 5070 Ti with driver 616.92, CUDA driver API 13040. At O0 and at `opt` O2 plus `llc` O3, nine lengths per configuration passed. The handwritten LLVM reference separately passed the same 18 launches. Each run checked both buffers, signed extremes and canaries. A second source function returning −7, 3 or 11 at thresholds −5 and 42 passed another 18 hardware launches against an independent oracle. |
| Vulkan raw | Clang 22.1.8 produced SPIR-V 1.5. SPIRV-Tools v2026.1 validated it for Vulkan 1.2. The [Vulkan harness](../../examples/gpu-ir/vulkan-smoke.c) checked counts 0, 1, 5, 63, 64, 65, 129, all input/output words and the count. Mesa llvmpipe passed all seven. On the RTX through WSL D3D12, the raw artifact exited with capability skip 77 because the driver lacks `shaderInt8`. |
| Vulkan capability experiment | After optimization removed the 8-bit types, the remaining unused `Int8` capability was removed explicitly. The reassembled module passed Vulkan 1.2 validation and all seven lengths on llvmpipe and on the RTX through WSL D3D12. The D3D12 Vulkan driver identified itself as nonconformant. The [opt-in packager](../../examples/gpu-ir/package_vulkan_clamp.py) reproduces this separate fixture. |

SHA-256 digests for the source-derived LLVM modules were `6b3df115e0b6a5ea5ca6dda5f4064590c0659d40e23fbeb3d3f1952f7ec0784c` for NVPTX and `5fd5bddc3b9431e9857b42e5a48e7719b8ceeb48d035ab2b4e41da464cff0543` for Vulkan. The raw SPIR-V digest was `8076911e39c96d5abcfe27538d1b5e593a20cfab9216683d6eb8d848c28d1afa`, and the Shader-only experiment was `01a77deea42daf173b9c4a8cb5394c31f5cead7e13acfe49559c53cb0a5ce566`. A standalone checked-add CUDA probe also tested per-lane overflow signaling on hardware. Checked arithmetic is outside the accepted source/IR profile. [Reproduction commands and integration plan](../gpu-compilation.md)
