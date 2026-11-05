# Shader tooling

Rill integrates external shader compilers through `rill shader`. Shader source
uses Slang, GLSL, HLSL or Metal; this is not a Rill GPU language backend. The
[3D engine example](../examples/engine-3d/README.md) uses Slang for vertex and
fragment programs, and SDL3 GPU for device creation, pipelines and draw calls.

```sh
rill shader inspect examples/engine-3d/rill.shaders.json
rill shader check examples/engine-3d/rill.shaders.json
rill shader build examples/engine-3d/rill.shaders.json
```

The manifest argument defaults to `rill.shaders.json` in the current directory.
`inspect` validates recipe structure and reports it without running tools.
`check` compiles in a temporary directory and publishes nothing. `build` compiles
and publishes the complete artifact set. Commands print JSON with artifact names,
source dependencies, SHA-256 digests, validation status and compiler diagnostics.
Errors retain the compiler's source filename, line and message where supplied.

## Manifest

```json
{
  "version": 1,
  "output": "build/shaders",
  "shaders": [
    {
      "name": "mesh_vertex_spv",
      "language": "slang",
      "stage": "vertex",
      "entry": "vertex_main",
      "source": "shaders/mesh.slang",
      "target": "spirv",
      "include_dirs": ["shaders/include"]
    },
    {
      "name": "mesh_vertex_msl",
      "language": "slang",
      "stage": "vertex",
      "entry": "vertex_main",
      "source": "shaders/mesh.slang",
      "target": "msl"
    }
  ]
}
```

Create every declared include directory before running the command. Paths are
relative to the manifest directory. Sources and dependencies must stay inside
that directory, use regular files, and avoid symlinks. Paths containing spaces
are supported. Absolute paths, traversal in recipe paths, duplicate JSON keys,
unknown fields and overlapping source/output paths are rejected. Use a dedicated
output directory; do not put handwritten files in it.

`version` is required and must be the integer `1`. `output` defaults to
`build/shaders`. `shaders` contains 1–64 entries. Entry names must be ASCII identifiers, at most 64 bytes, unique without case
distinctions so artifacts remain distinct on case-insensitive filesystems. Entry functions are ASCII identifiers, at most
128 bytes. Supported stages are `vertex`, `fragment` and `compute`.

| Language | Target | Required tools | Result |
| --- | --- | --- | --- |
| `slang` | `spirv` | `slangc`, `spirv-val` | `<name>.spv`, actual Slang reflection JSON |
| `slang` | `msl` | `slangc` | `<name>.metal`, actual Slang reflection JSON |
| `glsl` | `spirv` | `glslangValidator`, `spirv-val` | `<name>.spv`; entry must be `main` |
| `hlsl` | `spirv` | `glslangValidator`, `spirv-val` | `<name>.spv` |
| `metal` | `msl-source` | None during bundling | `<name>.metal`; device validation still required |

Other combinations are errors. The artifact directory also contains
`manifest.json` and an ownership marker. Slang reflection is compiler output,
not a generated Rill host-binding contract. Applications must check their own
vertex attributes, uniform sizes, alignment, descriptors and pipeline layout.
DXIL, metallib, mesh stages and ray tracing are not exposed by this recipe yet.

## Validation and tool selection

SPIR-V uses the Vulkan 1.0 environment, passes through the real `spirv-val`, and
must contain the declared entry name and execution stage. A missing validator is
an error. Slang uses column-major matrix layout and preserves SPIR-V entry names
with `-fvk-use-entrypoint-name`; see the official
[Slang command reference](https://docs.shader-slang.org/en/stable/external/slang/docs/command-line-slangc-reference.html).
The current Slang compiler may warn that the requested SPIR-V profile is old;
that diagnostic is included in the report.

Slang `msl` means Slang compiled the source into Metal code. It does not mean the
Apple compiler or a GPU accepted the resulting pipeline. Both `msl` and
`msl-source` reports explicitly require Metal runtime validation. A renderer must
handle shader and pipeline creation errors and retain a working pipeline until a
replacement succeeds. Tooling does not invent success when a tool or device is
missing.

The `metal`/`msl-source` mode bundles authored text. It expands literal quoted
local includes with `#line` information and preserves the SDK headers
`<metal_stdlib>`, `<metal_math>` and `<metal_texture>`. It rejects include cycles,
macro includes and other system includes. Include discovery is conservative:
local includes in inactive preprocessor branches must still exist. It does not
parse, type-check or verify the entry function. Prefer Slang when preprocessing
or compile-time checks are needed.

Executable lookup uses this order:

1. Explicit environment override: `RILL_SLANGC` (then `SLANGC`),
   `RILL_GLSLANG_VALIDATOR`, or `RILL_SPIRV_VAL`.
2. The matching tool in the relocatable Rill SDK selected by
   `RILL_TOOLCHAIN_ROOT`, or adjacent to the installed Rill executable at
   `../libexec/rill`. The SDK must have a matching `toolchain.json`.
3. The corresponding executable on `PATH`.

An override names one executable, not a shell command with flags. Tools and SDK
configuration are trusted local build inputs. Recipe files cannot specify a
command, arbitrary flags or a shell script. Bundled runtime library paths are
passed to child tools. An explicitly selected broken SDK fails with a diagnostic
instead of silently changing providers.

## SDL3 graphics resource layout

The engine compiles one entry per artifact. It gives vertex and fragment stages
separate uniform buffers and uses explicit Vulkan bindings together with HLSL
registers:

```slang
struct FrameData { float4x4 mvp; float4x4 model; };
struct MaterialData { float4 color; float4 light; };

[[vk::binding(0, 1)]]
ConstantBuffer<FrameData> frame : register(b0, space1);
[[vk::binding(0, 3)]]
ConstantBuffer<MaterialData> material : register(b0, space3);
```

| Resource used by its stage | SPIR-V location | Metal location | SDL uniform slot |
| --- | --- | --- | --- |
| Vertex frame data | Set 1, binding 0 | `[[buffer(0)]]` | Vertex slot 0 |
| Fragment material data | Set 3, binding 0 | `[[buffer(0)]]` | Fragment slot 0 |

These are the engine's verified single-buffer layouts. Slang preserves buffer
register indices in Metal and ignores register spaces there. Avoid assigning two
live resources the same Metal buffer index within one entry. Slang's
[Metal target documentation](https://docs.shader-slang.org/en/stable/external/slang/docs/user-guide/a2-02-metal-target-specific.html)
describes this translation and matrix layout.

Use `TEXCOORD0`, `TEXCOORD1`, and subsequent indices for vertex attributes, and
`SV_Position` for clip-space position. The generated Metal vertex input uses
`[[stage_in]]`. For more textures, storage buffers or uniforms, follow SDL's
format-specific ordering and explicit resource counts; shader compilation alone
does not validate those host declarations. SDL documents the binding tables and
vertex-buffer convention in
[SDL_CreateGPUShader](https://wiki.libsdl.org/SDL3/SDL_CreateGPUShader).

## Rebuilds, dependencies and publication

The compiler's Make-style dependency file discovers includes and Slang imports.
Rill captures their bytes, copies the confined graph into a private source
snapshot, recompiles that snapshot, and requires the final dependency graph and
bytes to agree. It checks that original inputs and the recipe stayed unchanged
before publication. Metadata hashes describe the bytes actually compiled.
Compiler diagnostics from the snapshot refer back to project source paths.

A failed compile, validation, dependency check or ownership check preserves the
previous artifact set. On macOS and Linux, replacement uses an atomic directory
exchange; unsupported filesystems report failure and keep the previous output.
Publishers use a per-output advisory lock on Unix. Other platforms use a staged
rename with rollback, which has a brief directory-visibility gap. Publication is
not an fsync-based crash-durability guarantee. Treat the output directory as
owned generated content and avoid editing it during a build.

Tool execution has a 60-second deadline, a 2 MiB limit per output stream, and
process-group cancellation on Unix. Sources are limited to 1 MiB each; a single
entry's dependencies are limited to 4,096 files and 16 MiB, with 64 MiB across the
recipe. Artifacts are limited to 16 MiB each and output metadata to 4 MiB.

Every shader action currently recompiles; there is no persistent shader cache.
The first compilation discovers dependencies, so a normal checked artifact uses
two compiler invocations. This favors verifiable inputs over a guessed cache
key. A future cache needs tool/version identities, include search precedence,
compiler flags and dependency additions before it can safely skip either pass.

A version-2 `rill.json` can set `"shaders": "rill.shaders.json"` beside its native
recipe. Project `check` checks shaders without publishing; project `run`/`build`
prepare artifacts before the native program starts. Project watch includes the
shader manifest, sources and include/import search directories, so dependency
edits participate in rebuild/restart. Potential-file discovery skips unrelated
assets and symlinks, and recognizes `.slang`, `.slangh`, `.glsl`, `.vert`, `.frag`,
`.comp`, `.hlsl`, `.hlsli`, `.metal`, `.inc`, `.h` and `.glsli`. Dependencies with
other extensions are exact watch inputs once an owned published inventory records
them. Before the first successful build, add `--watch path/to/custom.dependency`
for a custom extension that may change independently of its source. This is project restart, not live GPU
pipeline replacement or state-preserving native function patching.

## Acceptance evidence

`tests/shader_cli.rs` exercises real Slang SPIR-V/Metal translation and reflection,
real GLSL/HLSL SPIR-V translation, mandatory SPIR-V validation, include hashing,
source diagnostics, explicit runtime-required bundling, preserved previous builds,
unknown output rejection and symlink boundaries. Real compiler tests print a
skip reason if their external toolchain is unavailable; a skipped compiler test
is not shader compilation evidence. Device pipeline and rendered-image checks
belong to the engine integration and can require an accessible GPU/display.
