# Graphics SDK library choices

Status: Raylib is accepted for the default SDK in this checkpoint. The other
libraries below are proposals, not bundled packages or implemented Rill modules.
Reviewed against official upstream sources on 2026-11-05. See
[distribution contents](../distribution.md) for the shipped package and
[the graphics audit](../graphics-feature-audit.md) for language limitations.

Rill already has SDL3, Slang and SPIR-V validation. Add Raylib for a short path
from a Rill program to a window, input and drawing. Keep further additions tied
to a tested example and a small supported API. The cost ratings below are design
judgments about dependencies and maintenance; they are not measured archive-size
or performance comparisons.

| Candidate | Decision and useful next example | Rill ABI fit | Distribution and maintenance cost |
| --- | --- | --- | --- |
| **Raylib** | **Accept now:** beginner window/drawing example, then basic 3D | Use scalar C functions and a checked adapter for aggregate arguments | Another graphics runtime and backend to test; existing SDK need not change its SDL GPU renderer |
| **SDL_image** | **Defer, first asset candidate:** load a PNG/JPEG texture in the SDL engine | Pointer-returning loaders fit; surfaces/textures require explicit destruction | Moderate with a fixed small codec set; substantially larger dependency/notice set with optional formats |
| **SDL_mixer** | **Defer:** sound effects and streamed music in an SDL application | Opaque pointers and scalar controls fit; audio callbacks and thread ownership need a separate API | Moderate/high codec and device testing; avoid enabling every decoder by default |
| **Tracy** | **Opt-in developer integration later:** CPU frame/compile timing | C macros and zone structs need an adapter or compiler emission; no direct C++ ABI | Separate C++ client and profiler application, matched versions, profiling overhead and platform tooling |
| **RenderDoc** | **Document external use:** capture an SDL Vulkan frame on Linux | Its application capture API could be wrapped later; normal use needs no language binding | A complete debugger/UI, not a small runtime dependency; it does not cover the Metal example on macOS |
| **Selected stb headers** | **Vendor privately when needed:** image loading/writing for one example | Compile implementation once behind an explicit C adapter | Small source integration but ongoing decoder review; a second copy may duplicate Raylib's dependencies |

## Accepted: Raylib

The official latest release is [Raylib 6.2](https://github.com/raysan5/raylib/releases/tag/6.2),
published 2026-10-15. Its tag resolves to commit
`02566453880e06bec97f5b8160fa6fe1e98397ca`, verified through
[GitHub's tag reference](https://api.github.com/repos/raysan5/raylib/git/ref/tags/6.2).
Pin the selected source or platform SDK archive by SHA-256 in the distribution
lock; do not build a moving branch. Raylib uses the
[zlib/libpng license](https://github.com/raysan5/raylib/blob/6.2/LICENSE).

The tagged [README](https://github.com/raysan5/raylib/blob/6.2/README.md) describes
2D/3D drawing, models, textures, fonts, audio, OpenGL and a software backend. These
features explain its value for examples, not the extent of the first Rill
wrapper. `Color`, vectors, cameras, images and models must stay inside a C adapter
until Rill supports their actual foreign aggregate ABI. An SDK should expose the
curated module's exact support and cleanup rules, rather than promise every
upstream function.

The existing Slang-to-SPIR-V/MSL pipeline targets SDL GPU. Raylib's default
OpenGL shader path has a different contract: do not pass those artifacts to
Raylib or imply backend compatibility. Follow-up shader support needs explicit
GLSL targets, binding rules and a real Raylib pipeline test.

The official macOS SDK download was inspected locally: 3,458,403 compressed bytes;
SHA-256 `100437734a44d56ed9d2a1c4b249dbfc6d6cc3477a45e306f9f56fe5f1e2b4ef`.
Its shared library is 3,559,800 bytes and contains both x86_64 and arm64. `otool`
shows an `@rpath` install name, same-directory library aliases and system-framework
dependencies. It also includes a static library and headers. These are upstream
SDK measurements, not the incremental size of a completed Rill distribution or
verification on every platform. The distribution lock selects official platform
SDK archives; recording the source revision does not claim a local source build
of those upstream binaries.

## Deferred asset and audio packages

**SDL_image** loads images into SDL surfaces; its
[`IMG_Load` API](https://wiki.libsdl.org/SDL3_image/IMG_Load) returns a nullable
pointer that must later be destroyed. Several common formats are built in, while
additional formats depend on optional libraries. Its
[build configuration](https://github.com/libsdl-org/SDL_image/blob/main/CMakeLists.txt)
selects individual codecs, native ImageIO/WIC backends and vendored dependencies.
The library's own [license is zlib](https://github.com/libsdl-org/SDL_image/blob/main/LICENSE.txt);
codec notices still need an inventory. Prefer an explicit PNG/JPEG/QOI policy
and fail if that policy cannot be built. Choose it after an engine texture example
requires it, with missing/corrupt image tests and texture/surface cleanup tests.

**SDL_mixer** provides mixing, streamed decoding and effects. SDL3_mixer is a new
API and requires SDL 3.4 or newer, according to its
[README](https://github.com/libsdl-org/SDL_mixer/blob/main/README.md).
Its [CMake options](https://github.com/libsdl-org/SDL_mixer/blob/main/CMakeLists.txt)
include multiple MP3/Vorbis backends, Opus, FLAC, MIDI and other decoders, each
with its own dependency choices and notices. The top-level license is zlib; it
is not a blanket statement about optional codecs. Start with a bounded audio
example and selected formats, test mixing to memory without audio hardware, and
keep callbacks out of the first wrapper. Raylib already supplies an audio route,
so another default mixer should solve a demonstrated SDL-specific need.

## Profiling and capture tools

**Tracy** supports CPU and GPU profiling, including Metal, as described in its
[README](https://github.com/wolfpld/tracy/blob/master/README.md). Its
[C interface](https://github.com/wolfpld/tracy/blob/master/public/tracy/TracyC.h)
uses static source-location records, macros and zone context structs. Decide how
Rill emits locations and closes zones on every exit before claiming scoped
profiling. Start with optional native-adapter instrumentation; leave the GUI
outside the default runtime package. Its own
[license is BSD-3-Clause](https://github.com/wolfpld/tracy/blob/master/LICENSE),
and the [profiler build](https://github.com/wolfpld/tracy/blob/master/profiler/CMakeLists.txt)
has additional UI/platform dependencies.

**RenderDoc** is an MIT-licensed frame debugger for Vulkan, OpenGL and Direct3D
on its supported platforms. Its [upstream support table](https://github.com/baldurk/renderdoc/blob/v1.x/README.md)
explicitly does not support Metal, and its
[build guide](https://github.com/baldurk/renderdoc/blob/v1.x/docs/CONTRIBUTING/Compiling.md)
does not describe a supported macOS debugger. Link to installation/capture
instructions for Linux Vulkan projects. Use Apple's
[Metal debugger](https://developer.apple.com/documentation/xcode/metal-debugger)
for a separate Metal capture workflow. Bundling RenderDoc would add a platform-specific tool
without serving the current macOS GPU path.

## Small vendored helpers

The [stb collection](https://github.com/nothings/stb) offers individual C headers
under public-domain or MIT terms. A selected implementation belongs in one
native translation unit with allocator, failure and buffer-size rules exposed
by the adapter. Do not expose implementation macros as a Rill API or ship the
entire collection solely for future possibilities. Check Raylib's existing
[external sources](https://github.com/raysan5/raylib/tree/6.2/src/external) before
adding another decoder copy.

Upstream documents delayed security fixes in its README, and
[`stb_truetype.h`](https://github.com/nothings/stb/blob/master/stb_truetype.h)
explicitly excludes untrusted font input. A tiny header does not remove the need
for asset-input boundaries, failure tests and a maintained revision pin.

## Evidence required before another default addition

A proposed package needs a concrete Rill example, a reviewed scalar/pointer ABI
or explicit adapter, source/platform archive hashes, all dependency notices,
relocation tests and clear resource/thread rules. Measure compressed archive
size, installed bytes and clean/incremental build time before and after adding
it. Run both malformed-input/failure tests and a real operation on each claimed
platform; report unavailable hardware separately. Optional tools should not
increase a basic program's startup work or silently enable network listeners.
