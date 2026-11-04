# GPU voxel sandbox verification

This historical report is retained from archive snapshot `69277817`.
Fresh checks of the port are recorded in the [integration report](2026-11-04-ernie-integration.md).

This increment makes the [SDL3 voxel example](../../examples/sdl3-voxel/README.md)
use a real compute renderer and GPU simulation by default. The CPU reference
remains available with `--cpu`. No language phase is declared complete.

## Environment and execution

Apple M4 Pro with 20 GPU cores, native SDL3 Metal backend, SDL3 3.4.18, AArch64
macOS. SDL compiles the external MSL source at runtime. Neither Xcode's `metal`
compiler nor a software-rendering fallback was used. Normal sandbox execution
cannot access the host display/GPU services. The same actual GPU tests ran
successfully with approved host access; no device-unavailable skip occurred.

Rill controls camera state, input decisions and dispatch order. Shader passes
perform procedural generation, edits, exact connected support, fragment
integration, occupancy reconstruction, signed distance reconstruction, indirect
lighting and rendering. SDL compute passes end between dependent dispatches.
Persistent voxel data remains on the GPU during ordinary interactive execution.

## Final measured runs

| Configuration | Frames | Mean completed frame | Maximum | Bodies | Detached voxels |
| --- | ---: | ---: | ---: | ---: | ---: |
|128³,1280×720|120|8.0385 ms|24.0587 ms|728|17,180|
|256³,960×540|60|35.8314 ms|56.3835 ms|4502|134,376|

The first world has 2,097,152 voxels and 35.6 MiB of persistent storage buffers;
the larger world has 16,777,216 voxels and 266.8 MiB. Image textures, temporary
readback buffers, driver and shader memory are additional. Both runs reported
zero support errors, capacity exhaustion and traversal errors. The scripted
blast removed 281/2280 voxels respectively before transferring unsupported
materials into GPU fragment atlases.

Timings cover CPU command encoding, GPU work and fence completion for each
frame. Setup, shader creation and final diagnostic readback are excluded.
Capture overhead for the final frame is included. These are short offscreen
runs at fixed simulation steps, with a scripted demolition; they are not
isolated GPU timestamps or universal frame-rate guarantees.

A separate visible SDL window/swapchain/blit smoke passed at 128³ and 1280×720.
Its 12-frame vsynced average was16.56 ms before the final scene presentation
adjustments. Final offscreen captures were inspected visually, including the
camera-visible collapsed building. No physical mouse/keyboard input was replayed.

## Checks performed

The real-GPU Rust integration harness passed both tests with
`RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1`. It required hardware and enabled the
optional 256³ configuration. It compiled the native host/Rill output at `-O0`
and `-O2`, checked both results, required actual destruction and nonzero debris,
rejected support/capacity/traversal errors, and compared rendered 64³ images
byte-for-byte across optimization levels.

The native readback self-test compares every packed distance entry against an
independent reference for sparse sources, a completely solid world with
exterior air and an empty world. It compares complete voxel buffers after a
clipped spherical blast, checks bedrock preservation and removal counts,
checks exact support roots, compares retained static material and detached
atlas mass, asserts real fragment motion with finite coordinates, verifies
unsupported sleeping debris wakes, and verifies zero-capacity detachment
retains every voxel. The 64³ self-test also passed with SDL GPU validation enabled.

Independent actual GPU probes checked thin-wall hits from opposite directions,
empty-brick skipping and exact wall distances. Numerical DDA backtracking was
found through nonzero traversal counters and corrected. Indirect rays use a
conservative signed-grid bound, exact DDA near surfaces and exact fallback at
the distance-march budget. Budget exhaustion is not silently treated as a miss.

The existing CPU reference harness passed all 10 tests after adding explicit
`--cpu` selection. Targeted Rust formatting, standalone strict Clippy for the
new GPU test, Python parsing, Bash syntax and C11 `-Wall -Wextra -Werror` builds
passed. No compiled shaders, binaries, captures or dependency caches were added
to the source tree.

## Concurrent compiler changes

Cargo-wide verification was attempted, but the independent compiler work was
mid-edit. `cargo check --locked` failed on incomplete `Lambda`, `Apply` and
`FunctionOp` match coverage and new `CheckedProgram` fields. A whole-project
format check also encountered a then-missing `src/check/callables.rs` and an
unrelated formatting difference. These files were not changed to make the
example pass.

The GPU launcher used the existing working `target/debug/rill`. The same Rust
test source was compiled directly against the cached `serde_json` dependency:

```sh
CARGO_MANIFEST_DIR="$PWD" CARGO_BIN_EXE_rill="$PWD/target/debug/rill" \
  rustc --edition=2021 --test tests/sdl3_voxel_gpu.rs \
  --extern serde_json=target/debug/deps/libserde_json-93c669fd3870f69a.rlib \
  -L dependency=target/debug/deps -o /tmp/rill-voxel-tests-gpu-suite
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 /tmp/rill-voxel-tests-gpu-suite --nocapture
```

The dependency hash identifies the observed local build, and may differ after
a rebuild. Once the independent compiler work is ready, normal verification is:

```sh
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 cargo test --locked --test sdl3_voxel_gpu -- --nocapture
cargo test --locked --test sdl3_voxel
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Remaining boundaries

The compute backend currently requires Metal. External GPU shaders are not
Rill shader compilation. Fragments collide conservatively with static geometry;
fragment-to-fragment contacts, large rigid connected islands, streaming chunks,
transparent light transport and production directional/cascaded SDFGI remain
open. Quantized fragment voxelization may overlap visually, while atlas mass
remains intact. This is an engine prototype with measured GPU execution, not
feature parity with a commercial destruction game.

Primary API contracts were checked against the installed headers and official
[compute binding](https://wiki.libsdl.org/SDL3/SDL_CreateGPUComputePipeline),
[pass synchronization](https://wiki.libsdl.org/SDL3/SDL_BeginGPUComputePass) and
[texture readback](https://wiki.libsdl.org/SDL3/SDL_DownloadFromGPUTexture)
documentation.
