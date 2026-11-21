# Voxel performance and optimized launcher verification

This historical report is retained from archive snapshot `69277817`.
Fresh checks of the port are recorded in the [integration report](2026-11-04-ernie-integration.md).

The SDL3 voxel launcher now defaults to an optimized native game build. It
also avoids rebuilding unchanged GPU geometry and separates material edits
from pose changes. No compiler or editor source changes belong to this work.
This report declares no language phase complete.

## Build and launch

`examples/sdl3-voxel/run.sh` selects the GPU engine by default. The launcher
applies `-O3` to both the SDL adapter and the C generated from Rill, then probes
native CPU tuning and LTO through actual compile/link commands. On this Apple
Silicon host it prints `Native game build: -O3 -mcpu=native -flto=thin`.
Unsupported automatic options are omitted; explicitly requested unsupported
LTO fails. `--portable`, `--lto off` and `--opt 0` remain available.
No floating-point fast-math option was added. Metal compiles the compute shaders
separately at runtime; CPU flags do not stand in for GPU optimization.

The unused `test_geometry_cache` function is now called by `vg_test`.
The adapter compiles with C11, `-Wall -Wextra -Werror` at the optimized settings.
The exact default launcher was run with a twelve-frame exit limit in a native
SDL window. It returned zero, rendered nine intact structures and reported
`presentation: immediate`, one geometry rebuild and one distance rebuild.
`--vsync` opts into display synchronization.

## GPU work removed

- Persistent geometry and distance dirty flags drive GPU-generated indirect
  dispatch counts. Clean frames launch only guarded minimal workgroups.
- Geometry can refresh before player collision while pending distance work
  survives until lighting. The signed distance transform runs once after all
  frame edits, rather than again after a shot in the same frame.
- Separate completion passes clear flags after parallel producers finish.
  Clearing a flag inside a producer could make late workgroups skip work.
- Material edit bits distinguish static cells, building rest voxels and loose
  body atlases. Wall cuts update building mass without rescanning static
  connectivity; body-only cuts skip both static support and building analysis.
  Concurrent edit branches combine bits through atomic OR.
- A 64-lane integer reduction analyzes each building's mass, first moments and
  bounds. Intact assemblies sleep at their exact supported equilibrium.
- Pose comparisons now guard both occupancy and distance invalidation. A
  missing brace had made distance invalidation unconditional.
- Compact exact visibility queries avoid carrying a full primary-hit record
  through probe occlusion and sunlight rays.
- Balanced GI caches diffuse gathering at half image resolution, validates the
  current frame, exact voxel, face and depth, and falls back to full gathering
  at rejected boundaries. Primary rays and sunlight remain full resolution.
  `--gi full` retains per-pixel gathering. Eight indirect rays per updated probe
  remain enabled.

An earlier profile measured about 0.08 ms CPU submission against about 10.4 ms
waiting for GPU work. Simulation, field construction and rendering already ran
on the GPU; the costly part was unnecessary GPU work and serial shader scans.

## Completed-frame measurements

Hardware was an Apple M4 Pro with 20 GPU cores, SDL3 3.4.18 using Metal.
Each run used 128³ voxels, balanced GI, the optimized default build, a fixed
simulation step and an explicit completion fence per frame. Other benchmark
jobs and the broad repository suite were stopped. Device/shader initialization
is excluded; CPU command encoding and GPU fence completion are included.

```sh
examples/sdl3-voxel/run.sh --benchmark 360 --stress
examples/sdl3-voxel/run.sh --benchmark 360 --stress --width 1152 --height 648
```

| Compute image | Frames | Warmup excluded | Steady mean | p95 | Maximum including warmup |
| --- | ---: | ---: | ---: | ---: | ---: |
|1280×720|360|20|6.7123 ms|8.6149 ms|31.6058 ms|
|1152×648|360|20|6.0429 ms|8.0757 ms|22.0680 ms|

Both runs removed 392 actual voxels, retained nine active assemblies with
175,193 structural voxels, and reported a tilted assembly with remaining
support. Both reported zero support, capacity, traversal and stuck-player
errors. Geometry rebuilt 412 times and distance fields 360 times. Moving
assembly poses require real updates even between edits.

The default-resolution mean is about 149 FPS; its p95 exceeds the 8.33 ms
budget for 120 FPS. At 1152×648, 95% of these measured frames fit that budget.
Neither short run proves a locked minimum rate for every collapse or camera
position. The stress mode walks while directing brushes at known material so
the workload cannot devolve into missed shots. It is not physical input replay
and does not represent arbitrary large-scale fracture.

Exit metrics identify synchronous benchmark timing versus asynchronous window
submission timing. An unbounded interactive run reports no percentile when it
has no retained frame samples. Counter readback occurs at close; ordinary
simulation does not copy bulk world state back to the CPU.

## Checks

The following checks completed successfully:

```sh
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 cargo test --locked --test sdl3_voxel_gpu -- --nocapture --test-threads=1
RILL_VOXEL_GPU_DEBUG=1 examples/sdl3-voxel/run.sh --self-test --world 128
cargo test --locked --test sdl3_voxel
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

The required GPU suite passed four tests with no skips, including 256³
execution, default `-O3` self-tests, identical `-O0`/`-O2` rendered captures,
partial-support leaning and native SDL cursor/focus behavior. The later
Metal-validation run exercised the final cache-counter assertions and compared
all 2,097,152 distance entries with independent references. The cache tests
check missed shots, rest-only and body-only edits, retained buffers, skipped
support/analysis through sentinels, indirect buffer extents and independent
geometry/distance completion. The CPU reference passed all ten tests.

The broad repository regression suite was interrupted intentionally with exit
130 during the performance investigation and was not completed. No full-suite
pass is claimed. Strict all-target Clippy and the final formatter check passed.

## Experiments retained outside the repository

A conditional sunlight query produced identical pixels but did not reliably
improve timings. Direct non-atomic overlay reads and then a separate read-only
kernel view were also measured. The safe view passed GPU tests and produced
byte-identical images, but improved the mean only 1.3–1.8% and p95 0–1.2% in
matched repeat runs. These changes were not shipped.

Balanced versus full GI on a matched final stress image had mean absolute RGB
error 0.049 on a 0–255 scale; 687 of 921,600 pixels exceeded an 8-level maximum
channel difference. Captures were inspected without visible wall leaks.
Temporary captures, private experiments and native binaries remain outside
the repository.
