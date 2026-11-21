# Voxel structures and first-person controller implementation plan

> Implement the independent GPU structure and player tasks in parallel, then verify their shared resource layout and frame ordering together.

**Goal:** Make partially damaged buildings lean on surviving spring supports, and add cursor-controlled Minecraft-style walking, jumping and crouching.

**Architecture:** Nine GPU building assemblies retain their rest voxels and transform them together. Surviving foundation columns act as breakable spring supports. Existing loose debris stays available for ordinary detached material. A GPU player AABB collides against current static and dynamic occupancy without CPU world readback. Rill controls orientation, mode, input decisions and dispatch order. The SDL host manages cursor capture and focus transitions.

**Tech stack:** Rill, C11 SDL3 GPU host, external MSL compute kernels, actual Metal readback tests.

**Spec:** User request in this conversation; `examples/sdl3-voxel/README.md` describes the previous accepted behavior.

## Global constraints

- Preserve the independent compiler work and CPU reference. No compiler changes.
- SDL permits eight compute write buffers. Extend existing allocations with explicit aligned tails rather than introducing a ninth binding.
- Params grows from160 to192bytes with float4 movement and uint4 controller. Existing offsets remain unchanged.
- Body remains80bytes. Nine128-byte Structures follow the body array, then one64-byte Player. CPU mirrors have static layout assertions.
- Cells allocation holds2×N³ uints: static materials followed by structural rest materials. Parent overlay and probe resources retain their contracts.
- Every dependent dispatch ends its compute pass. New passes17..20 are structure init/analyze/integrate/voxelize;21 is player_step.
- Do not silently discard material on errors or resource limits, or describe the simplified assemblies as a general engineering stress solver.

## Interfaces

`Structure` consists of float4 rest,position,velocity,angle,spin,bounds_min,bounds_max and uint4 info. rest.w is initial mass; bounds_min.w initial anchor count; bounds_max.w surviving anchor count; info.xy is live mass/active and info.zw broken support masks. `structures(bodies,p)` locates the tail.

`Player` consists of float4 feet_ground,velocity_height,eye_mode and uint4 flags. `player_state(bodies,p)` follows the nine structures. Params movement is strafe,forward,jump,crouch. Controller is mode,reset,sprint,inputActive. Modes0/1/2 are external benchmark camera/walk/GPU fly. Mode changes preserve player position.

## Task1: Building support and tilt

Files: GPU shared layouts, `simulation.metal`, new `structural.metal`, host buffer allocation and pass map.

- [x] Transfer generated upper-building materials into persistent rest storage, preserving the static foundation.
- [x] Analyze remaining material and foundation supports after edits; update center of mass and broken supports.
- [x] Integrate gravity and spring forces/torques with damping, support failure and ground collision; transform each building's voxels together.
- [x] Check partial-support leaning, full-sever collapse, symmetric stability, reset and material conservation through real GPU readback.

## Task2: Player and camera

Files: new `player.metal`, `main.rill`, `native.rill`, `gpu.h`, `gpu.c`, new pure `input.h`.

- [x] Add grounded AABB movement, normalized diagonal input, swept collision, wall sliding, jumping, gravity, crouch/stand clearance, steps and sneak-edge behavior.
- [x] Keep player movement GPU-resident and render/pick from its actual eye. Preserve position when toggling fly mode.
- [x] Capture the cursor on the first click without firing; Escape/Tab release it, focus loss clears input, focus regain requires a fresh click. Discard stale relative motion.
- [x] Make walking the default, keep deterministic external-camera smoke mode, and use an elapsed-time weapon cooldown.
- [x] Verify walls, floor, jump edges, crouch ceilings, mode transitions, cursor state and focus handling.

## Task3: Integration and evidence

Files: new `interaction_tests.c`, existing GPU tests/launcher, example README and a dated verification report.

- [x] Extend vg_test with actual shader fixtures and deterministic input-state tests.
- [x] Run C builds at-O0/-O2, strict GPU tests including256³ where available, CPU reference tests, format and Clippy checks appropriate to the changes.
- [x] Inspect a real camera-visible leaning/collapse capture and native-window presentation. Record measured performance and remaining approximation limits.
- [x] Review final diff and untracked files; retain no captures, binaries or temporary build output in the repository.

Evidence is recorded in the [structure/controller report](../../verification/2026-11-04-voxel-structures-controller.md)
and [performance report](../../verification/2026-11-04-voxel-performance.md).
The broad repository suite was interrupted and did not complete. The default
1280×720 destruction benchmark averages above 120 FPS but still exceeds 8.33 ms
at the 95th percentile; the performance report records that remaining target gap.
