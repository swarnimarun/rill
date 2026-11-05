# Graphics engine and distribution implementation plan

> **For agentic workers:** Use the delegate skill for independent numeric,
> shader and renderer work, then review and verify the combined checkpoint.

**Goal:** Build a Rill-driven 3D renderer, integrated Slang shader tooling and
a reproducible default graphics SDK for release `v0.1.0-alpha`.

**Architecture:** Rill owns scene state, camera/update logic and transform math.
A narrow C11 SDL GPU adapter owns native descriptors, buffers, submission and
completion. External Slang/GLSL compilers produce checked shader assets; Rill
does not pretend to implement a shader-language type/effect system.

**Tech stack:** Rust/C11, SDL3 GPU, Slang, SPIR-V Tools, existing JSON tooling.

**Spec:** User requests for a feature audit, a 3D engine, first-class shader
tooling and standard modules, and bundled release assets. PLAN14.4 prioritizes
external shader integration. Publication is explicitly excluded by the user.

## Constraints

- Preserve all existing history and unrelated changes. Commit focused steps.
- Do not push, tag remotely, create a release or upload assets in this session.
- Reject unsupported language/ABI forms rather than invent layouts/signatures.
- Keep native resource destruction after GPU completion; no lifetime proof is
  inferred from a C header or an integer resource handle.
- Require actual shader compilation/validation. Label raw MSL source bundles
  as requiring runtime validation, never as validated GPU programs.
- Keep structured process arguments and original linker sequences.
- Run hardware checks outside the sandbox when necessary; the real SDL Metal
  device probe succeeds there. Report skips and untested platforms honestly.

## 1. First-class bounded f32 operations

Files: checker, shared float operation definitions, both evaluators, typed IR
verification/C and general C, `tests/f32_arithmetic.rs`.

```rill
fn main() {
    f32_add(f32(20), f32(22)) == f32(42)
}
```

- [x] Add monomorphic `f32_add/sub/mul/div/neg`, `f32_lt/le/gt/ge` and
  `f32_is_finite`. Preserve existing integer operator inference and syntax.
- [x] Make f32 values and i32 conversion interpretable, with IEEE numeric
  equality, NaNs/infinities/signed zero and matching main-result formatting.
- [x] Round each C operation to binary32 using volatile intermediates; reject
  common fast-math modes when the float feature is used.
- [x] Test rounding, noncontraction, all execution paths and malformed IR.
  Keep unsupported control/origin proof subsets explicitly rejected.
- [x] Write the actual renderer feature audit with evidence and remaining gates.

## 2. Shader projects and tooling

Files: `src/shader.rs`/submodules, `tests/shader_cli.rs`, CLI wiring, docs.

```json
{"version":1,"output":"build/shaders","shaders":[
  {"name":"mesh_vertex_msl","language":"slang","stage":"vertex",
   "entry":"vertex_main","source":"shaders/mesh.slang","target":"msl"}
]}
```

- [x] Implement `rill shader check|build|inspect [rill.shaders.json]`.
- [x] Compile Slang to SPIR-V/MSL, validate SPIR-V, retain reflection and exact
  dependency content identities. Support GLSL as another available frontend.
- [x] Snapshot dependencies and publish complete output atomically. Failed
  compilation must preserve the previous asset generation.
- [x] Resolve explicit tools, bundled tools and PATH without shell parsing.
- [x] Test real compilers and controlled failure/corruption/path cases.

## 3. Project native and shader recipes

Files: `src/project.rs`, `src/project/recipe.rs`, main/watch, project tests.

Version1 keeps its existing strict fields. Version2 adds optional recipes:

```json
{"version":2,"name":"engine-3d","entry":"src/main.rill",
 "native":{"sources":["native/renderer.c"],"include_dirs":["native"],
           "packages":["sdl3"],"libraries":["m"]},
 "shaders":"rill.shaders.json"}
```

- [x] Validate confined paths and schemas before building. Compile native
  sources in unique private directories using the selected current C compiler.
- [x] Pass object inputs before dependency libraries and keep recipe directories
  alive until link/run completes. Check native sources without publishing objects.
- [x] Route project build/run/check through recipes; each watch rebuild repeats
  native/shader preparation. Track manifest, source/header and shader inputs.
- [x] Preserve old running programs on failed candidate builds and keep ordinary
  source commands/project-v1 behavior intact.

## 4. Renderer example

Files: `examples/engine-3d/`, `tests/engine_3d.rs`.

- [x] Add Rill vector/matrix, camera, scene, mesh and draw orchestration.
- [x] Implement narrow scalar SDL GPU bridge with explicit mesh upload, depth
  texture, material/light uniforms and completion-aware cleanup.
- [x] Use Slang vertex/fragment assets, with actual GPU readback and image capture.
- [x] Verify deterministic occlusion/material pixels and math on CPU and GPU.
  Keep CPU checks explicit; never silently replace failed GPU rendering.

## 5. Standard modules and default distribution

Files: toolchain resolver, `stdlib/sdl.rill`, shader metadata/helpers, SDK bridge,
release scripts/lock/workflows, release documentation.

- [x] Bundle checked scalar SDL bindings and shader-stage/resource utilities.
  Provide a supported Slang compile boundary rather than guessed C++ layouts.
- [x] Discover relocatable SDL/tool paths from a versioned `toolchain.json`.
- [x] Pin upstream Slang, SDL and SPIR-V tooling by exact versions and SHA-256;
  include runtime libraries, headers, standard sources, examples and licenses.
- [x] Set package version `0.1.0-alpha`. Prepare tag/manual GitHub Actions build
  and release-asset jobs, with publication gated for the user's later action.
- [x] Build and inspect the local distribution, exercise relocation and bundled
  tool resolution, and document other platforms as CI-prepared until executed.

## Final checkpoint

- [x] Review all changes, run focused tests, strict formatting/Clippy and full
  locked tests after source freeze. Exercise real Metal rendering and capture.
- [x] Record feature gaps, hardware/toolchain evidence and release readiness.
- [x] Commit and advance local v1 only. Leave publication to the user.


## 6. Raylib and installer continuation

User steering prioritizes SDK libraries, installer scripts and README before
returning to remaining language work. Do not push or publish.

- [x] Pin official Raylib 6.2 native SDKs and preserve all upstream notices.
- [x] Add a scalar C11 bridge, `std.raylib` and a bounded 2D/3D example.
- [x] Verify actual OpenGL rendering, lifecycle failures and capture pixels.
- [x] Add checksum-verified versioned macOS/Linux installer and update handling.
- [x] Resolve installed executable symlinks before discovering the adjacent SDK.
- [x] Test local archive installation, failures, updates and installed SDK lookup.
- [x] Update README, searchable installation docs and future library proposals.
- [x] Refresh and verify the local SDK archive, then commit without publication.
