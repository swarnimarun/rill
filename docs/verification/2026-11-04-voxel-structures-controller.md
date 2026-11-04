# Building structures and first-person controller verification

This historical report is retained from archive snapshot `69277817`.
Fresh checks of the port are recorded in the [integration report](2026-11-04-ernie-integration.md).

The [GPU voxel example](../../examples/sdl3-voxel/README.md) now has connected
building assemblies supported by destructible spring joints, plus a walking
player and cursor capture. The Rust compiler and unrelated concurrent changes
were preserved. This increment declares no language phase complete.

## Actual GPU behavior

The nine generated buildings store their upper materials in a rest atlas and
transform each assembly together. Intact foundation reactions balance force
and torque. Partial support removal creates a tipping moment while surviving
columns still carry spring loads. Overstrain can break the remaining joints;
complete severing lets the assembly fall. GPU fixtures check every building's
intact stability, supported lean, full-sever motion and material conservation.
A test found an incorrect clamped load weighting at 128³; separable positive
weights corrected its force and torque balance before the tests passed.

The player uses a swept AABB against current static and voxelized dynamic
geometry. GPU tests cover wall sliding, grounded jump edges, rejected repeated
air jumps, landing, crouch/stand clearance, dynamic debris collision, long-frame
bounds, fast falls, fly/walk continuity and invalid-placement recovery.
Player position and contact state remain on the GPU; rendering and picking use
its actual eye. Frame order updates geometry before controller movement.

Pure event-policy tests and a separate native SDL window regression cover
first-click capture without firing, capture-frame motion discard, subsequent
relative look, Escape/Tab release, focus loss, stale queued destruction actions,
focus gain without recapture and final quit. The native test checks SDL's actual
relative-mode flag. Capture API failure stops the next frame and closes the
window instead of letting logical state diverge from a hidden native cursor.
These are scripted SDL events, not a replay of physical user input.

## Resources and synchronization

SDL allows eight compute write buffers. The existing cells allocation now holds
both static and structural rest voxels. The body buffer retains its 80-byte
fragment records, followed by nine 128-byte Structure records and a 64-byte
Player. Params is 192 bytes; CPU mirrors have size/offset assertions. GPU
readback tests exercise the same offsets. No ninth write binding was added.

Each dependent dispatch still ends its compute pass. New passes initialize,
analyze, integrate and voxelize structures, then integrate the player after
fresh geometry reconstruction. Edits update assembly mass/support before
rebuilding occupancy and distances. The controller uses bounded substeps;
weapon cooldown uses elapsed time rather than render-frame count.

## Observed development runs

Apple M4 Pro 20-core GPU, SDL3 Metal 3.4.18, native C at -O2:

| Scenario | Frames | Image | Mean completed frame | Maximum |
| --- | ---: | --- | ---: | ---: |
|128³ overview/full sever|120|1280×720|9.4147 ms|39.4243 ms|
|128³ player/partial cut|90|1280×720|12.6992 ms|110.4135 ms|
|256³ overview/full sever|60|960×540|42.8058 ms|156.7316 ms|

The broad repository suite was running during these captures. Timings include
CPU encoding, GPU work and fence completion, not shader/device creation. They
are development observations, not isolated GPU timestamps or realtime promises.
The runs reported zero support, capacity, traversal and stuck-player errors.
The partial cut reported a tilted building with foundation support remaining;
full severing reported an unsupported assembly. Captures were visually inspected
at player eye height and from the overview. The 128³ buffers total 43.6 MiB and
the 256³ buffers 330.8 MiB, excluding images, transfers and driver allocations.

## Checks

Commands used for the actual hardware suite and host/source checks:

```sh
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 cargo test --locked --test sdl3_voxel_gpu -- --nocapture
RILL_VOXEL_GPU_DEBUG=1 examples/sdl3-voxel/run.sh --self-test --world 128
examples/sdl3-voxel/run.sh --input-test --world 64
cargo build --locked
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

The required GPU suite passed all four tests with no hardware skips, including
optimized/debug capture agreement, 256³ execution, partial-lean/player capture
and a native cursor/focus test. The Metal-validation self-test passed. Native
C builds use C11 with -Wall -Wextra -Werror at both tested optimization levels.
The build, formatter and strict all-target Clippy checks passed at that checkpoint.
A full default repository regression run was started, then deliberately
interrupted with exit 130 after the user reported poor performance. It did not
complete. The concurrent suite also made those development timings unsuitable
as an isolated performance baseline. The later performance report records the
optimized builds and isolated runs.

## Boundaries

The structure model is a rigid building assembly with foundation spring/bending
joints. It does not provide arbitrary upper-building fracture, local beam
flexure, assembly-to-assembly contacts or a general stress solver. Ground
contact uses conservative rotated bounds. The player is kinematic, with an
AABB rather than a capsule, and does not push rigid bodies. Recovery is bounded
and reports failure rather than claiming every possible engulfment can be
resolved. No source-generated captures or binaries belong in the repository.

SDL capture semantics were checked against the official
[relative mouse mode contract](https://wiki.libsdl.org/SDL3/SDL_SetWindowRelativeMouseMode).
