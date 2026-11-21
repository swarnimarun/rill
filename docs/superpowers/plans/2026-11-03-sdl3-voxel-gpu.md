# GPU voxel sandbox implementation plan

**Goal:** Replace the default tiny software demo with a large perspective destruction sandbox using actual SDL3 GPU compute on the available Metal GPU.

**Architecture:** Rill owns controls, camera state and explicit frame-graph scheduling. SDL3 C host code owns GPU resources, packs sixteen-byte uniforms and submits passes. External MSL kernels own voxel generation, edits, connected support, rigid debris, distance reconstruction, radiance caching and ray-traced images. Keep the old CPU core as a separate reference mode.

**Constraints:** Preserve the other agent's compiler work. No fabricated Rill shader compiler or production/Teardown parity claims. All dependent dispatches have separate compute passes. Keep persistent data GPU-resident, with readback only for verification/capture/benchmark. Prove mass preservation and reject failed GPU work. Default128³ cells, optionally64³/256³; real compute output, no software fallback posing as GPU execution.

## Interfaces

`gpu.h` defines scalar `vg_*` calls for Rill. `shared.metal` defines160-byte Params and80-byte Body. All kernels receive one uniform atbuffer0 and eight readwrite storage buffers at1..8: staticmaterials, atomicparents plus dynamic overlay, packed distances,8³ brick occupancy, bodies,64material-per-body atlas, pingpong float4 probes, atomic counters. Render additionally writes RGBA8 texture0. End each dispatch's pass before its dependent dispatch.

Counters:0 allocated bodies,1 removedvoxels,2 detachedvoxels,3 union failure,4 capacity saturation,5 livevoxels initial,6 frame steps. Bodies retain materialatlas and mass; exhaustion leaves voxels static. Only detach after support union completes successfully.

Pass IDs:0resetworld,1edit sphere,2supportseed,3supportunion,4detach,5integrate,6clearoverlay,7voxelizedebris,8brickoccupancy,9distanceinit,10distanceX,11distanceY,12distanceZ,13probes,14render,15clearprobes. Workgroup64x1x1 except render8x8x1. Uniform frame.x frameindex; frame.y viewmode; frame.z reset/historyepoch; frame.w reserved. limits.x bodycapacity, limits.y probegridN, limits.z distanceaxis unused, limits.w raysperprobe. config.x brickgridN, config.y outputwidth, config.z outputheight, config.w probe update stride.

## Tasks

- [x] Implement actual compute simulation kernels and a large architectural scene; atomic connected components, conservative detach on error, bounded rigid fragments with gravity, rotation and static collision.
- [x] Implement exact distance transform, occupancy acceleration, perspective rays, shadows and spatial/temporal indirect lighting.
- [x] Implement resource/command lifecycle, camera/input ABI and Rill gameplay/dispatch loop; GPU default launcher and reference CPU mode.
- [x] Verify real GPU state/readback, destruction/support/mass invariants, images, debug/optimized agreement and measured timing. Preserve and run CPU reference tests.
- [x] Document tested scale, controls, GPU backend requirements, measured results and remaining physics/lighting limits.

## Accepted implementation and verification

- GPU runtime MSL compilation works without the Xcode Metal command-line tools.
- Exact atomic union-find performs support classification, with error flags retaining unresolved material. Sleeping fragments recheck support after static edits.
- Camera-visible demo destruction and warm front-facing sunlight make the collapse inspectable in captures.
- Default compute image is1280×720. Actual120-frame run:8.04ms mean with728 fragments. A256³960×540 sixty-frame run:35.83ms mean with4502 fragments. Both reported zero support/capacity/traversal failures.
- Real GPU regression harness passes both C optimization levels and the optional256³ case. CPU reference ten-test harness also passes.
- Cargo-wide checks are currently unavailable because the independent compiler agent is expanding callable AST/checker support. Verification uses the existing working compiler and unchanged test sources compiled against cached dependencies; no compiler workaround or unrelated edit was introduced.
