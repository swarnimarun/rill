# Default graphics distribution

The `v0.1.0-alpha` distribution combines the Rill executable with Slang, SDL3,
Raylib, SPIR-V validation and small scalar C ABI bridges. It is an experimental
checkpoint. [The graphics audit](graphics-feature-audit.md) records language
features still missing; bundling native tools does not complete the phase gates.

Extract an archive and add its `bin` directory to `PATH`. Keep `bin`, `libexec`
and `share` together, or use the [versioned installer](install.md).
A C11 compiler and the platform's development SDK remain
required for native `check`, `run` and `build`. Rust, CMake and Python are needed
to build the distribution, not to use the installed compiler. Automatic C
header discovery additionally needs a compatible libclang; the curated
`std.sdl` declarations and engine adapter use explicit declarations and do not
need automatic discovery.

```sh
rill new hello
rill project run hello
rill shader build path/to/rill.shaders.json
rill project build path/to/engine-3d
rill docs --language
```

The engine sources are under `share/rill/examples/engine-3d`. Copy that directory
to a writable location before building it. See its [guide](../examples/engine-3d/README.md)
for camera controls, deterministic rendering and capture commands.

## Package contents and lookup

```text
bin/rill
libexec/rill/toolchain.json
libexec/rill/slang/{bin,lib,include,...}
libexec/rill/spirv/bin/spirv-val
libexec/rill/sdl3/{lib,include,...}
libexec/rill/raylib/{lib,include,...}
libexec/rill/sdk/{lib,include/rill/{slang.h,raylib.h}}
share/rill/{stdlib,docs,examples,licenses,...}
```

The compiler searches `../libexec/rill` relative to its executable, or the
explicit `RILL_TOOLCHAIN_ROOT`. `toolchain.json` has version 1, an exact Rill
version, relative tool paths, native package include/library paths and runtime
library directories. Paths must resolve within the SDK. An invalid selected SDK
fails instead of silently switching to a system installation. Moving the whole
package before compiling is supported. Native executables built against shared
SDK libraries still need those libraries at runtime; ship them with an
application using the target platform's runtime packaging conventions.

Shader tool overrides take precedence, then bundled tools, then `PATH`. See
[shader tooling](shaders.md). Native `@pkg_config("sdl3")` and
`@pkg_config("raylib")`, `@pkg_config("rill-slang")` and
`@pkg_config("rill-raylib")` resolve through the bundle without invoking
pkg-config. Other packages still use the system's pkg-config and preserve its
ordered linker arguments. Rill child processes receive the SDK runtime library
paths. This does not change the parent shell or install anything globally.

The default frontend is Slang. Standalone `glslangValidator` is optional for
GLSL/HLSL recipes and is not included in this first archive. The SDK includes
Slang's own dependencies and license notices. Shader output and native GPU
execution retain their separate validation requirements.

## Standard modules

`std.sdl` exports a curated SDL3 scalar subset: initialization, windows,
renderers, drawing, event pumping/querying and timers. SDL resource pointers
are nullable raw pointers. Check creation, destroy the renderer before its
window, and call `SDL_Quit` after destruction. Rill does not infer a resource
lifetime or guarantee cleanup after traps. Window/video operations belong on
SDL's main thread. Use a native adapter for the GPU API's descriptor structs;
[the engine](../examples/engine-3d) demonstrates that boundary.

`std.raylib` exposes checked scalar bridge calls for windows, balanced drawing
scopes, camera setup, 2D shapes/text, 3D cubes/planes and frame capture.
`Color`, `Vector2`, `Vector3`, `Camera3D` and `Image` stay inside C; the Rill ABI
does not guess their layout. Calls belong on the main thread. Check every
boolean result, balance drawing/3D scopes, and explicitly close the window.
`WindowShouldClose` returns -1 on invalid state, 0 to continue and 1 to close.
See [the runnable example](../examples/raylib/README.md). This is a curated
subset of Raylib, not a binding for every function or automatic resource
ownership. Its OpenGL renderer does not consume the Slang SPIR-V/MSL assets
used by the SDL3 GPU example.

`std.shader` supplies `Stage`, `Target`, `Resources`, `stage_code`,
`valid_resources` and `target_for_backend`. Resource counts are host metadata,
not a proof of shader reflection compatibility. `Stage` codes 0, 1 and 2 mean
vertex, fragment and compute for the SDK bridge.

`std.slang` exports `RillSlang_compile_spirv` and `RillSlang_compile_msl`:

```rill
import std.slang as slang;
import std.shader as shader;
fn main() {
    slang.RillSlang_compile_spirv(
        c"shaders/compute.slang", c"main",
        shader.stage_code(shader.Compute), c"build/compute.spv"
    )
}
```

Each call creates and releases its own Slang session, compiles one stage and
writes diagnostics to stderr. Zero means success. Nonzero results report invalid
arguments/stage, compilation failure or output I/O failure. C++ objects never
cross the ABI. Paths are static C-string arguments under the existing foreign
call rules. Runtime compilation allocates inside Slang and performs file I/O;
no Rill purity or allocation proof is claimed. The bridge emits compiler output;
it does not independently run `spirv-val`. Prefer `rill shader build` for
validated, dependency-snapshotted project assets. MSL still needs device
compilation and pipeline validation.

## Build the archives locally

Use Python 3.12 or newer, Rust/Cargo, CMake, a C/C++ compiler and platform SDK.
The Python script downloads official archives into `target/distribution`, checks
every SHA-256 and compiles SDL3, the SPIR-V validator and both bridges.
Raylib uses its pinned official native SDK, with a universal macOS library and
a Linux x86_64 library; it is not rebuilt from source by this script.
It then builds Rill with the locked Rust dependencies and runs a bundle smoke
check before producing an archive and checksum. Nothing is published.

```sh
python3 -m unittest discover -s tools/dist -p 'test_*.py'
python3 tools/dist/build.py --jobs 4
```

Output defaults to `target/dist/rill-0.1.0-alpha-<target>.tar.gz`, its `.sha256`
and an unpacked prefix. Existing output is never overwritten; choose a fresh
`--output` for another build. Use `--rill-bin` to package an already built binary
of the same version. Downloads persist after failure, while private build stages
are removed. Packaging is version/checksum pinned, not claimed bit reproducible:
platform SDKs, C/C++ compilers, timestamps and Rust toolchains can differ.

[Dependency pins](https://github.com/swarnimarun/rill/blob/v0.1.0-alpha/tools/dist/dependencies.json) select Slang 2026.21,
SDL 3.4.20, Raylib 6.2, SPIRV-Tools 2026.4 and the SPIRV-Headers revision from that release's
DEPS. Upstream sources are the [Slang release](https://github.com/shader-slang/slang/releases/tag/v2026.21),
[SDL release](https://github.com/libsdl-org/SDL/releases/tag/release-3.4.20) and
[Raylib release](https://github.com/raysan5/raylib/releases/tag/6.2), plus
[SPIRV-Tools dependencies](https://github.com/KhronosGroup/SPIRV-Tools/blob/v2026.4/DEPS).
Update hashes, licenses and smoke evidence together when changing these pins.

## GitHub release setup

[The workflow](https://github.com/swarnimarun/rill/blob/v0.1.0-alpha/.github/workflows/release.yml) prepares native archives for
macOS arm64, macOS x86_64 and Linux x86_64. Other targets, including Windows,
need their own dependency locks, native compiler/ABI checks and package tests.
Only locally executed targets have local verification evidence. Linux and Intel
macOS jobs remain prepared until the workflow runs on those hosts.

A push of `v0.1.0-alpha` builds Actions artifacts. It does not publish a GitHub
Release. Manual dispatch defaults to `publish=false`. To publish later, run the
workflow on the exact `v0.1.0-alpha` tag with `publish=true`; its release job
checks archive digests and uploads assets to a prerelease. The
`github-release` environment can hold repository-required reviewers.
Existing asset names are not overwritten. No tag, push, release or upload is
performed by the local packaging commands.

The workflow also prepares `install.sh` and its checksum as release assets.
The installer downloads a matching archive, verifies it, preserves installed
versions and activates only an installer-owned `bin/rill` symlink. It does not
edit shell profiles or execute the downloaded compiler during installation.
See [installation](install.md) for local archive and update commands and
[library candidates](proposals/graphics-sdk-libraries.md) for later SDK additions.

GitHub only registers manual dispatch when the workflow exists on the
repository's default branch. Place the workflow there before relying on the
manual publication step. This setup does not change the default branch or merge
the separate `v1` and `main` histories. GitHub documents this requirement under
[workflow_dispatch](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch).
