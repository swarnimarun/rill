# GPU voxel destruction sandbox

The default example now uses real SDL3 GPU compute, a perspective camera and a
128³ voxel world. The old 8³ software example is available with `--cpu` as a
[reference implementation](CPU-README.md).

```sh
examples/sdl3-voxel/run.sh
```

For a larger world:

```sh
examples/sdl3-voxel/run.sh --world 256
```

This backend currently targets Metal on macOS. It needs SDL3 development files,
pkg-config, a C11 compiler, Python 3 and a Metal GPU. Shader source compiles at
runtime, so Xcode's `metal` command is unnecessary. The launcher uses `RILL` if
set, otherwise an existing `target/debug/rill`, and builds the compiler when no
binary exists. Rebuild that binary explicitly after compiler changes. No GPU
operation silently falls back to the software renderer.

The default native build uses `-O3`, supported native CPU tuning and link-time
optimization. On Apple Silicon with Apple Clang this selects
`-O3 -mcpu=native -flto=thin` for both the SDL adapter and generated Rill C.
The launcher probes compiler/linker support and prints the selected flags.
`--portable` disables native CPU tuning, `--lto off` disables LTO, and
`--opt 0` selects a debug build. These options affect native code; Metal compiles
the compute shaders separately at runtime. Native builds do not add `-ffast-math`.

## Controls

- Click the window to capture the cursor. That first click does not fire.
- Move the mouse to look. `WASD` walks, `Space` jumps, `Shift` sprints and `Ctrl` crouches/sneaks.
- Hold the left mouse button for small, precise blasts under the crosshair.
- `T` cuts part of a visible building's foundation so it leans on the surviving supports.
- `X` cuts the full foundation and lets that building fall.
- `F` switches between walking and fly mode without teleporting. In fly mode, `Space` rises and `Ctrl` descends.
- `G` switches lit materials, material colors and distance-field visualization.
- `P` pauses physics. `R` resets the scene.
- `Escape` or `Tab` releases the cursor. Escape with a free cursor quits.

Focus loss releases the cursor and clears pending actions. Returning to the
window requires another click; stale relative motion is discarded. Arrow keys
also turn the camera while captured.

The default controller is a voxel-colliding player AABB, with a 0.6 m width,
1.8 m standing height and 1.5 m crouch height at quarter-meter voxel scale.
It uses gravity, grounded jump edges, wall sliding, bounded swept movement,
step-up and crouch ledge checks. A low ceiling prevents standing up. Rendering
and blast picking use the GPU player's actual eye position. Movement is
kinematic: the player collides with debris and tilted buildings but does not
apply reciprocal rigid-body impulses. If moving geometry overlaps the player,
a bounded recovery searches for clear space or the last safe walking position.

## Work performed on the GPU

The world starts as nine hollow multi-storey buildings, with slabs, windows,
metal framing, destructible lights and an indestructible ground plane.

Compute kernels generate materials and apply spherical destruction. Each of
the nine buildings retains its rest voxels in a GPU atlas and moves as a rigid
assembly. Its foundation columns are breakable vertical spring supports with
bending resistance and damping. Uneven cuts shift the supported load and make
the building lean. Excessive joint strain breaks the remaining supports;
complete severing lets the building fall. Removing upper material updates its
mass and center of mass without moving the surviving voxels discontinuously.
Intact support loads balance both force and torque, so intact buildings remain
stable. Rotation and raster overlap preserve the atlas's material mass.

Exact six-neighbor connectivity through atomic union-find still handles loose
static material. Unsupported pieces use bounded 4³ voxel fragments with linear
and angular velocity, gravity and conservative static-world collision. Sleeping
fragments recheck support. The 8192-body limit retains excess static material.

A retained dynamic occupancy grid and 8³ brick occupancy accelerate exact voxel
DDA visibility. GPU dirty flags and indirect dispatches skip reconstruction when
neither materials nor poses change. Static support is recomputed only after
static edits; rest-atlas wall cuts update assembly mass without rescanning static
connectivity. Separate kernels reconstruct both sides of a signed Manhattan
distance field using three separable transforms. Indirect-light rays use its
conservative Euclidean lower bound for empty-space steps, then exact DDA near
surfaces. Spatial probes accumulate sky, sunlight, emissive light and previous
radiance for successive diffuse bounces. Fresh geometry rejects occluded probes.
The compute renderer adds shadows, contact shading, materials, fog and tone
mapping, then SDL blits the image to its GPU swapchain.

Balanced GI caches diffuse probe gathering at half image resolution. It accepts
only current-frame samples on the same voxel face and falls back to full probe
gathering at rejected boundaries. Primary visibility and sunlight shadows remain
full resolution. `--gi full` gathers GI separately for every visible pixel.
Distance fields rebuild once after all frame edits, so shooting cannot trigger
a duplicate transform in the same frame.

Rill owns input decisions, mouse orientation, walk/fly mode and dispatch order in
[gpu/main.rill](gpu/main.rill). [gpu/gpu.c](gpu/gpu.c) manages SDL resources,
scalar foreign calls, uniform packing, submission and verification readback.
The GPU owns player position, velocity and contact state. External MSL implements the GPU algorithms. This does not add a Rill shader
compiler. Dependent dispatches use separate compute passes, with persistent
storage and explicit fence waits for verification. Normal play does not read
bulk voxel state back to run simulation on the CPU.

This is an engine prototype. Generated buildings are rigid assemblies joined
to foundation springs, not a general finite-element or beam stress solver.
New cuts retain holes but do not split an upper building into arbitrary separate
components. Assembly ground contact uses conservative rotated bounds; assemblies
do not yet collide with one another. Loose fragment rotation is voxelized.
There is no fragment-to-fragment contact solver, streaming world,
transparent glass transport, directional SH probe cache or production SDFGI.
The distance transform is a grid metric, not a continuous Euclidean surface SDF.

## Measured configurations

Apple M4 Pro 20-core GPU, SDL3 Metal 3.4.18, 128³ world,
default optimized build and balanced GI:

| Scenario | Image | Frames | Steady mean | 95th percentile | Removed voxels |
| --- | --- | ---: | ---: | ---: | ---: |
|Scripted walking and repeated wall cuts|1280×720|360|6.71 ms|8.61 ms|392|
|Same workload|1152×648|360|6.04 ms|8.08 ms|392|

The first 20 frames are excluded from the steady measurements. Timings include
CPU encoding, GPU simulation, geometry, lighting, rendering and fence completion;
device/shader creation is excluded. This isolated run averaged about 149 FPS,
but its 95th percentile exceeds the 8.33 ms budget for 120 FPS. It does not
establish a locked 120 FPS rate or measure every possible collapse. All nine
assemblies remained active, with zero support, capacity, traversal and
stuck-player errors. The stress mode uses scripted brush positions to guarantee
actual cuts while walking; it is not a physical input replay.

For more margin around the 120 FPS target, use
`examples/sdl3-voxel/run.sh --width 1152 --height 648`. That measured run kept
95% of completed frames below 8.33 ms, with the same physics and GI settings.

Normal window presentation runs uncapped when SDL supports immediate mode.
`--vsync` opts into display synchronization. Exit metrics distinguish asynchronous
window timing from synchronous benchmark timing and report the actual mode.

Reproduce measurements or capture a real compute-rendered frame:

```sh
examples/sdl3-voxel/run.sh --benchmark 120 --capture /tmp/rill-gpu.bmp
examples/sdl3-voxel/run.sh --benchmark 360 --stress
examples/sdl3-voxel/run.sh --benchmark 240 --walk --no-demo
examples/sdl3-voxel/run.sh --benchmark 60 --world 256 --width 960 --height 540
examples/sdl3-voxel/run.sh --smoke --world 64 --width 320 --height 180
examples/sdl3-voxel/run.sh --benchmark 90 --lean --walk --capture /tmp/rill-walk-lean.bmp
examples/sdl3-voxel/run.sh --self-test --world 64
examples/sdl3-voxel/run.sh --input-test --world 64
```

`--stress` benchmarks walking with repeated actual wall cuts. `--no-demo` disables
the automatic foundation cut. `--lean` changes that demo from full severing to a partial cut.
`--walk` uses the ground-level player for automated captures; ordinary interactive
play already defaults to walking. `--smoke` runs twelve deterministic offscreen frames. `--benchmark N` also uses
fixed simulation steps and waits for completed GPU work. `--width` and `--height`
select compute resolution; `--opt 0` selects unoptimized native host/Rill C.
`--capture` replaces the requested BMP path. `CC` selects a C compiler executable.
`RILL_VOXEL_GPU_DEBUG=1` enables SDL GPU validation.

## Verification and remaining work

The GPU tests run actual shaders and compare complete distance buffers, edited
materials, support roots, detached mass, motion, sleeping-body wake-up and capacity behavior. Additional fixtures cover intact
assembly stability, supported leaning, full severing, material conservation,
wall sliding, jump edges, crouch clearance, mode transitions and recovery. Native captures agree at `-O0` and `-O2` on the tested GPU;
the self-test also exercises the default `-O3` build.
An optional 256³ run checks the larger allocation and dispatch sizes.

```sh
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 cargo test --locked --test sdl3_voxel_gpu -- --nocapture
cargo test --locked --test sdl3_voxel
```

A GPU-unavailable preflight may skip when `RILL_REQUIRE_GPU` is absent. After a
device exists, shader, execution and readback failures fail the tests. Metal
needs the native video backend and host GPU access; SDL's dummy driver cannot
run these compute tests. The CPU reference still uses the dummy software driver.

See the [GPU verification report](../../docs/verification/2026-11-03-sdl3-voxel-gpu.md)
for the earlier GPU checkpoint. The [structure/controller verification report](../../docs/verification/2026-11-04-voxel-structures-controller.md)
records the latest checks and approximation limits. The [native scale notes](../../docs/blockers/voxel-native-scale.md)
distinguish the old CPU storage limit from the GPU path.
The [performance report](../../docs/verification/2026-11-04-voxel-performance.md)
records the optimized launcher and destruction measurements.
