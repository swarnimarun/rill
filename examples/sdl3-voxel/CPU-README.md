# CPU voxel reference

A small voxel-engine core in Rill, with an SDL3 software-rendered sandbox.
Destroy pillars, watch unsupported voxels drop, and inspect the lighting as the
scene changes. This example uses Rill's current subset without compiler changes.

From the repository root, with Rust, a C11 compiler, Python 3, pkg-config and
SDL3 development files installed:

```sh
examples/sdl3-voxel/run.sh --cpu
```

The launcher builds Rill and a small SDL adapter, then compiles the example to
C11 at `-O2`. It uses a temporary build directory and passes compiler arguments
directly. `CC` selects the C compiler executable; `RILL` selects an existing Rill
binary. Use `--opt 0` for an unoptimized build. SDL3 3.2 or newer is required for
its debug-text renderer. No textures, font downloads or asset files are needed.

- Left mouse button blasts a radius-two sphere around the highlighted voxel.
- Right mouse button places a moss voxel immediately above it when there is room.
- `X` blasts the demonstration pillars. `R` restores the scene.
- `Tab` rotates the view by a quarter turn.
- `G` cycles distance-field lighting, unlit materials and the signed-distance
  slice at height three. Warm slice cells are solid; blue cells are air.
- `Space` pauses gravity. Edits still work while paused. `Escape` quits.

The blue base is indestructible bedrock. Brown stone and green moss transmit
support through their six neighbors. Amber voxels emit light. Placement uses
the frontmost voxel under the cursor, so the bedrock remains a build target
after the other voxels are removed.

## Core modules

`world.rill` stores an 8×8×8 volume in two fixed 256-byte Copy buffers. Each cell
contains a material ID. Samples outside the world are air; out-of-bounds writes
are ignored. Storage is bounded and has value semantics. This is a small test
world, not a streaming chunk system.

`physics.rill` applies clipped spherical destruction while preserving bedrock.
Each fixed 100 ms tick floods support from bedrock and occupied bottom-plane
cells, then moves unsupported voxels down one cell in bottom-up order. Occupied
destinations block movement. Material is conserved during gravity. The loop
allows at most three catch-up ticks per frame and clamps elapsed wall time
through the SDL adapter. Pause discards accumulated time.

This is cellular voxel gravity. Connected unsupported pieces do not have rigid
body rotation, inertia, friction or fracture constraints, and there is no
player capsule controller yet.

`lighting.rill` rebuilds a signed Manhattan distance field and scalar indirect
illumination after an edit or gravity movement. Distances measure voxel-center
distance to the nearest cell of opposite occupancy, with exterior air. Solids
are negative, air is positive, and an entirely empty volume uses +24 as a bounded
infinity. This convention is not a continuous Euclidean surface SDF.

Six axis-aligned distance-guided rays gather sky, emissive light and one diffuse
bounce from a snapshot of direct lighting. Rays use conservative empty-space
steps and test encountered solids. Opening a roof changes skylight; removing
an emitter removes its contribution. The renderer adds fixed face shading.
This is an SDFGI-inspired CPU approximation, not full SDFGI, physically based
GI, RGB light transport, temporal accumulation or a GPU renderer. It rebuilds
the whole small volume synchronously. No frame-rate or allocation-free claim
is made for this reference example. Native foreign calls now use the checked
typed backend where their layouts are supported.

`render.rill` handles isometric projection, face visibility, scanline drawing,
rotation, picking and debug views. `main.rill` owns simulation state and tick
scheduling. `platform.c` handles SDL window/renderer lifetime, event aggregates,
input, bounded elapsed time, text and scalar line calls. It contains no voxel,
physics or lighting implementation. `native.rill` declares these scalar calls;
the target C compiler checks them against `platform.h` on each build. All SDL
calls stay on the main thread, and explicit cleanup covers initialization and
render failures. Raw foreign resources do not acquire Rill ownership proofs.

## Verification

Run the simulation without SDL:

```sh
cargo run -- run examples/sdl3-voxel/headless.rill
cargo run -- build examples/sdl3-voxel/headless.rill -o /tmp/rill-voxel-core
/tmp/rill-voxel-core
```

Run deterministic core comparisons and a real SDL dummy-driver smoke:

```sh
cargo test --locked --test sdl3_voxel -- --nocapture
examples/sdl3-voxel/run.sh --cpu --smoke
```

The core tests compare the interpreter with C at `-O0` and `-O2`. They cover
storage boundaries, spherical edits, bedrock preservation, connected support,
falling and material conservation, signed distances, occlusion and lighting
invalidation. The native smoke runs eight frames, blasts the pillars at frame
two and steps gravity every frame. It reports an explicit skip in the test
suite if SDL3 development files are unavailable. Physical input needs a desktop
session and is not simulated by this smoke.

Set `RILL_VOXEL_CAPTURE` to a BMP output path with `--smoke` to save the eighth
real SDL framebuffer. The adapter writes that path, replacing an existing file
if present. Captures and binaries belong outside the repository.

SDL contracts: [event polling](https://wiki.libsdl.org/SDL3/SDL_PollEvent),
[elapsed milliseconds](https://wiki.libsdl.org/SDL3/SDL_GetTicks),
[debug text](https://wiki.libsdl.org/SDL3/SDL_RenderDebugText).

Larger-engine prerequisites and the native C stack limitation are recorded in
[voxel scale limits](../../docs/blockers/voxel-native-scale.md).
