# Rill 3D engine example

This is a small raster renderer with a moving perspective camera, two rotating
cubes, a floor, directional lighting, separate materials and a depth buffer.
Rill builds the meshes, updates the camera, computes model/view/projection
matrices and submits each draw. SDL3 handles GPU resources through a C11 adapter.
The shaders are written in Slang and compiled by `rill shader`.

This example exercises the current language subset. It is not a complete game
engine or a claim that Rill has Rust/Zig feature parity.

## Build and run

The [default distribution](../../docs/distribution.md) supplies SDL3, Slang and
SPIR-V validation. Keep its SDK beside `bin/rill`, copy this project to a
writable directory, and use `rill project build` or `rill project run` there.
For a source checkout, install a C11 compiler, SDL3 development headers and
library, `pkg-config`, `slangc` and `spirv-val`.
SDL3 must have a usable Metal or Vulkan GPU driver.
The project needs a native build; the Rill interpreter cannot execute SDL calls.

From the repository root:

```sh
cargo build --locked
target/debug/rill project check examples/engine-3d
target/debug/rill project build examples/engine-3d
examples/engine-3d/build/engine-3d
```

`target/debug/rill project run examples/engine-3d --native` also builds and runs
the project. For rebuild/restart on source, native adapter or shader changes:

```sh
target/debug/rill project watch examples/engine-3d
```

The version 2 project manifest supplies native source/include paths, SDL3
`pkg-config` flags, the math library and the shader manifest. There is no shell
command embedded in the manifest. `build/shaders` contains the compiled Slang
Metal source, validated SPIR-V and an artifact manifest. Shader names and entry
points are explicit in `rill.shaders.json`.

Use W/S to move forward/back, A/D to strafe and Q/E to move down/up. Hold the left
mouse button and drag to look, or use the arrow keys. Escape closes the window.
Resizing recreates the color/depth targets after outstanding GPU work completes.
Zero dimensions and images exceeding the host allocation or 32-bit GPU transfer
size range are rejected before creating targets or reading pixels.

Build shader artifacts separately when iterating on a shader:

```sh
target/debug/rill shader build examples/engine-3d/rill.shaders.json
```

## Bounded rendering and captures

```sh
RILL_ENGINE_HEADLESS=1 RILL_ENGINE_FRAMES=20 \
  RILL_ENGINE_CAPTURE=/tmp/rill-scene.ppm \
  examples/engine-3d/build/engine-3d
```

Headless mode uses a real GPU and renders offscreen without opening a window.
SDL still needs its platform video subsystem and access to the GPU. A display
session may be required. An unavailable device prints `Engine GPU unavailable`
and a final Rill result of `77`. There is no software rendering fallback.
After device creation, shader/resource/protocol failures produce result `1`.
Success produces `0`. The current Rill executable prints its main result; these
values are printed results, not process exit codes.

The capture is an uncompressed P6 PPM image with RGB bytes. GPU readback waits on
a submission fence before mapping the transfer buffer. Image viewers that do
not accept PPM can convert it with their usual image conversion tool.

| Variable | Default | Meaning |
| --- | --- | --- |
| `RILL_ENGINE_HEADLESS` | unset | Set to `1` for actual offscreen GPU rendering |
| `RILL_ENGINE_FRAMES` | window: `0`, headless: `4` | Frame budget, `1..10000`; window `0` runs until quit |
| `RILL_ENGINE_WIDTH`, `RILL_ENGINE_HEIGHT` | `640`, `480` | Initial render dimensions, `32..2048` |
| `RILL_ENGINE_CAPTURE` | unset | Write the last submitted frame to this PPM path |
| `RILL_ENGINE_MODE` | `0` | `0` scene, `1` depth verification, `2` shifted camera |
| `RILL_ENGINE_SHADERS` | project shader directory, then executable directory + `shaders` | Override the shader artifact directory |

Mode 1 draws a near red quad before a farther blue quad, then verifies the center
pixel is red. Without working depth rejection, the blue draw would cover it.
Mode 2 moves the Rill camera two units right. Headless frame timing is a fixed
16 milliseconds, so input-free captures are reproducible for a given GPU backend.
The shader uses floating point lighting, so other backends can differ by a few
channel values at triangle edges.

## Where the work lives

| File | Responsibility |
| --- | --- |
| `src/math.rill` | f32 vector, matrix, perspective and camera transforms |
| `src/scene.rill` | Camera state, clamped look and movement |
| `src/mesh.rill` | Cube/floor/quad vertex generation |
| `src/render.rill` | Scene objects, materials, matrix composition and draw order |
| `src/main.rill` | Input, frame loop, failure propagation and explicit cleanup |
| `src/native.rill` | Checked scalar declarations for the C header |
| `native/renderer.c` | SDL input, GPU allocations/uploads, render commands and readback |
| `shaders/mesh.slang` | Vertex transform and directional diffuse fragment stage |

The math uses column vectors and row records. Rill sends rows through scalar
functions, and the C adapter packs column-major shader buffers. The coordinate
system is left handed, with +Y up, +Z forward and normalized depth `0..1`.
CPU tests independently check composition order, translation, a 90 degree
rotation, camera basis, and the near/far projection planes.

Rill currently spells float operations explicitly, such as
`f32_add(a, b)` and `f32_div(f32(1), f32(10))`. It does not yet accept float
literals or overloaded vector operators. The adapter provides `sinf`/`cosf` for
integer millidegree angles; matrix arithmetic is Rill code.

## Native and shader contracts

`ptr<Engine>` is a raw foreign handle. Rill does not prove its lifetime or clean
up GPU resources automatically. Every successful `e3_open` has one explicit
`e3_close`, which waits for GPU work before releasing resources. Do not retain
or call a handle after close. See [the C interop contract](../../docs/c-interop.md).

The adapter accepts at most eight triangle meshes, each with at most 4096
vertices. Vertices are immutable after upload. A vertex contains two C float3
values with a 24 byte stride. Static assertions check the host layout.
Matrices use two 64 byte float4x4 values and materials use two float4 values.
Scalar FFI setters reject nonfinite data, incomplete matrices, zero normals,
invalid mesh indices and draws outside an active frame. This explicit boundary
avoids assuming that Rill records have C or shader struct layout.

Slang generates both stages from the same source. Vertex inputs use
`TEXCOORD0/1`, which map to SDL vertex attributes `0/1`. Vertex uniforms occupy
SPIR-V set 1 and fragment uniforms set 3, each at binding 0. Metal receives one
stage-local uniform buffer at slot 0. The build fixes column-major matrix layout.
SDL's [shader resource binding rules](https://wiki.libsdl.org/SDL3/SDL_CreateGPUShader)
define these conventions. A Slang source compiling successfully does not prove
arbitrary shader/host layouts match; changing these structs requires changing
both sides and the verification.

The lighting normal transform is correct for the positive uniform scaling used
by this scene. Nonuniform or negative scaling needs a proper normal matrix and
an explicit winding policy before it is supported. This example has no textures,
asset loader, skeletal animation, GPU memory allocator or runtime shader reload.
Rebuild/restart remains the project hot reload workflow. Function replacement
and live GPU resource migration require separate ABI and synchronization work.

## Verification

```sh
cargo test --locked --test engine_3d -- --nocapture
RILL_REQUIRE_GPU=1 cargo test --locked --test engine_3d -- --nocapture
```

The math test runs the same Rill modules through the interpreter and C backends
at `-O0` and `-O3`. The GPU test copies the project to a temporary directory,
builds the native adapter and all Slang artifacts, and checks real readback for
depth rejection, material colors, camera movement and animation. It also checks
that a broken shader produces an error after a GPU has been obtained.

The first command reports a GPU skip if tools or device access are unavailable;
the CPU test still runs. `RILL_REQUIRE_GPU=1` turns those skips into failures so
a hardware validation job cannot silently pass without drawing.
