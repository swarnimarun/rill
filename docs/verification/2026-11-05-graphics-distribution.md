# Graphics tooling and alpha packaging verification

This checkpoint adds explicit binary32 operations, external shader projects,
version-2 native/shader recipes, an SDL3 GPU engine example, standard graphics
modules and local release packaging. It does not complete a PLAN phase or claim
Rust/Zig feature parity. GPU resource ownership and live pipeline migration
remain outside the checked language subset.

## Host and toolchains

The local host is arm64 macOS with Rust/Cargo 1.99.1 and Apple Clang 17.
Source-tool integration used installed Slang 2026.19, SDL3 3.4.18,
glslangValidator 16.6.0 and spirv-val 2026.4. The default SDK was separately built
and exercised with pinned Slang 2026.21, SDL3 3.4.20, SPIRV-Tools 2026.4 and its
pinned SPIRV-Headers revision. The bridge also compiled with
`-Wall -Wextra -Werror` against the pinned SDK.

## Executed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| Full `cargo test --locked --no-fail-fast` | 1,595 passed across 162 test batches, zero failures |
| Final binary unit tests | 57 passed, including immutable repeated Metal include capture and dynamic project inputs |
| Final shader CLI integration | 19 passed, no tool skips |
| Project CLI and watch integration | 7 project CLI and 3 watch tests passed |
| Native/shader project recipes | 7 passed |
| Bundled-tool CLI regressions | 3 passed; installed aliases and relative SDK selection survives project cwd changes and dangling SDKs reject |
| Bundled-tool unit tests | 3 passed |
| Binary32 integration | 4 passed across both evaluators and C backends at O0/O3 |
| Graphics standard modules | 2 passed; pure metadata across backends and real SDL ABI/native calls |
| Required engine hardware integration | 3 passed, including native image bounds, CPU math and actual Metal rendering |
| Slang SDK CTest | 1 passed, containing 5 Python ABI cases, against pinned Slang and bundled spirv-val |
| Offline packaging tests | 13 passed, including 8 notice-integrity cases |
| Release workflow | Ruby YAML syntax check passed; GitHub jobs were not executed |
| Generated documentation | 37 HTML pages, 3,570 links and 264 search addresses checked |

The full locked sweep began on the integrated implementation while independent
reviews continued. Subsequent repeated-include capture, shader discovery and
renderer readback fixes received the final focused checks above. No numeric
backend change followed the start of that sweep. The final standard modules
also passed Rill's own `fmt --check` and their integration tests after canonical
formatting. These are recorded as separate checks rather than a claim that the
long sweep reran every later focused revision.

Real shader tests cover Slang SPIR-V/MSL and GLSL/HLSL SPIR-V compilation,
reflection, mandatory validation, exact dependency bytes, malformed output,
source diagnostics, cancellation, output limits, concurrent publishers and
preservation of prior generations. Raw Metal source bundling is explicitly
runtime-required, not semantically validated shader output.

## Hardware and native boundaries

The default sandbox prevents SDL's Metal device access. An external execution
probe established that the real host has a Metal device. Required engine tests
then ran outside that sandbox with `RILL_REQUIRE_GPU=1`, and ran again with
`RILL_TOOLCHAIN_ROOT` selecting the final extracted SDK. Both runs passed all
three engine tests. The GPU test cannot turn an unavailable device into a pass
when that environment variable is set.

The renderer checks near-red/far-blue depth rejection, orange/cyan materials,
Rill camera movement and animated frame differences through real GPU readback.
A deliberately broken shader must produce failure after device creation.
Independent CPU checks cover matrix composition, translation, rotation, camera
basis and near/far depth through both evaluators and C at O0/O3. Native bounds
checks exercise zero, 4K, maximum fitting and overflowing image/transfer sizes
before any GPU access. A separate 640x480 capture rendered 20 frames with 60
draws and 1,560 submitted vertices, returned Rill result 0 and was visually
inspected. Metal API validation was enabled.

The full suite's optional hardware/tool integrations retain their own skip
conditions. A successful Rust test result alone is not proof that each optional
platform ran. The explicit required Metal checks above establish this new
engine's local hardware evidence. Linux Vulkan, Intel macOS and Windows GPU
execution were not tested here.

## Distribution acceptance

The local arm64 macOS archive includes the compiler, full Slang runtime/headers,
SDL shared library/headers, spirv-val, the Slang C ABI bridge, bundled source
modules, guides, examples and upstream licenses. Every fetched archive passed
its pinned SHA-256 check. SDL and the validator were built from those sources.

The archive was extracted to a second location whose name contains spaces.
Smoke tests compiled and validated SPIR-V, called real SDL through `std.sdl`,
compiled a shader through `std.slang`, and generated language docs. Inherited
tool overrides were removed and failing PATH tools prevented accidental fallback
to installed Slang, spirv-val or pkg-config. The relocated package passed again
after the final compiler/module refresh.

SDK libraries use relative install names/runtime search paths. SDL's generated
pkg-config prefix was corrected to `${pcfiledir}/../..`; a native C compile/run
against the relocated metadata passed, including a package path with spaces.
This is SDK relocation before building applications. It does not make an
executable independent of the shared libraries it linked.

The workflow prepares macOS arm64, macOS x86_64 and Linux x86_64 builds using
Rust 1.99.1. Only the local arm64 package ran here. Windows archives are not
configured. Tag pushes build Actions artifacts; a separate manual dispatch on
the exact alpha tag with `publish=true` enables release upload. The workflow
must first exist on GitHub's default branch to register manual dispatch.
No push, remote tag, release creation or asset upload occurred in this session.

## Graphics build timing observation

One warmup and three measured samples used the release compiler from the bundled
SDK, Slang 2026.21, SDL3 3.4.20 and default O3. Timings include process startup,
Rill checking/emission, shader dependency discovery and snapshot compilation,
SPIR-V validation, and native syntax checks or compilation/linking. The full
Rust sweep was running concurrently, so these are local observations with
contention rather than quiet-machine benchmark claims.

| Command | Samples in milliseconds | Median |
| --- | --- | ---: |
| `rill project check <engine>` | 1048.2, 1099.5, 1137.3 | 1099.5 ms |
| `rill project build <engine>` | 1161.7, 1146.1, 1164.2 | 1161.7 ms |

Every shader build still recompiles. There is no persistent shader cache or
native object cache in project recipes. Existing exact-graph checked-C reuse
remains separate and native inputs are always prepared afresh. These timings do
not measure GPU frame throughput, large libraries or general Rust/C/Zig parity.


## Raylib, installer and hosted-docs continuation

The SDK adds the pinned official Raylib 6.2 native package, a scalar C11 bridge
and `std.raylib`. The bridge and Rill example passed real OpenGL 2D/3D rendering,
image pixel checks, invalid-state checks and hidden-to-visible reopening. The
bounded Rill example completed 180 frames and printed result 0. Both SDK CTest
targets passed. The Raylib vendor notice inventory retains 112 verified files
with immutable source URLs, exact source ranges and content hashes.

The installer passed 12 isolated-prefix integration tests covering checksums,
archive paths/links/types, update retention, idempotence, failed staging,
launcher ownership, activation interruption and unsupported platforms. An
actual SDK archive installed into a temporary path with spaces. Its `bin/rill`
symlink found bundled Slang, spirv-val, SDL and the Slang bridge with provider
overrides removed and system package directories disabled. This prompted the
canonical-executable-path fix, also covered by a dedicated CLI regression.
No installer test wrote to the host's home or system installation directories.

After these additions, fresh library/binary/bundled-module checks passed 547,
57, 6, 2 and 3 tests respectively. Clippy and format checks passed. These later
checks supplement the earlier 1,595-test full sweep; the full sweep was not
repeated for the SDK bindings, installer, metadata or documentation changes.

Documentation CI keeps Markdown in `docs/`, builds downloadable review sites
and prepares v1-only GitHub Pages deployment. The local release build with
`--no-default-features` generated 37 pages; 3,570 links and 264 search addresses
passed, as did the Node search behavior checks. A separate repository-subpath
simulation kept 3,556 local links and all search targets under `/rill/`.
Workflow YAML, action versions/inputs and deployment permission/branch guards
were checked. No GitHub workflow ran and Pages remains unconfigured remotely.

The completed local archive is `target/dist/rill-0.1.0-alpha-aarch64-apple-darwin.tar.gz`
with its SHA-256 sidecar. `target/dist/install.sh` and its checksum are prepared
for review alongside it. These generated assets are intentionally untracked.
