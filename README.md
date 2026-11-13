# Rill

Rill is an experimental native language for graphics, games and tools. The
compiler is written in Rust and emits C11. The goal is ML-style inference,
functions and pattern matching with explicit storage, predictable allocation
and checked resource use. The current sequential subset implements only part
of that goal; syntax, library interfaces and ABI APIs remain unstable.

Start with the [syntax tour](docs/syntax-tour.md),
[feature matrix](docs/language-features.md) or
[Rust migration guide](docs/rust-to-rill.md). Contributors should read the
[delivery status](docs/phase-status.md), [active roadmap](docs/CONTINUATION_PLAN.md)
and [performance plan](docs/build-performance.md). [PLAN.md](PLAN.md) holds the
long-term design and phase gates, including features not implemented.

## Run from source

Install a Rust toolchain and a C11 compiler available as `cc`, then run:

```sh
cargo run --locked -- check examples/factorial.rill
cargo run --locked -- run examples/factorial.rill
cargo run --locked -- build examples/factorial.rill -o /tmp/factorial
/tmp/factorial
```

`run` builds and executes native code by default. Both `run` and `build` use
`-O3`; `--opt-level 0` selects an unoptimized build. `run --interpret` uses the
checked evaluator and rejects native dependencies. Select a compiler with
`--cc <executable>`, `CC`, or `--zig-cc`. Arguments are passed without shell
parsing. See [native builds](docs/native-builds.md) for optimization contracts.

The default Cargo build includes C import support. libclang is loaded when
headers are imported; pure programs do not need it. `--no-default-features`
builds retain pure Rill and explicit extern support.

## Language example

```rill
fn main() {
    let mut value = 0;
    while value < 6 {
        value <- value + 1;
    };
    if value == 6 { value * 7 } else { 0 }
}
```

Braces delimit blocks and semicolons separate statements. A final expression
supplies a block's value; a trailing semicolon produces unit `()`. Newlines do
not separate statements. Legacy expression syntax remains accepted.

The supported subset includes rank-one inference with a value restriction,
generic algebraic types, exhaustive matches, finite Copy records, closures
with immutable Copy captures, currying and call-scoped `read`/`inout` loans.
Closed parameter and result annotations constrain types. Local annotations
and early `return`, `break`, `continue` and `?` are still proposals.

Integer arithmetic traps on overflow and division by zero in every build mode.
Explicit `f32_*` operations provide binary32 arithmetic; float literals and
overloaded float operators remain unsupported. Static UTF-8 strings compare
by byte contents. Hosted boxes and fixed-length owned bytes have restricted
moves, borrows and cleanup, with recoverable allocation through `std.alloc`.

Owner programs remain acyclic and monomorphic and reject owned records and
foreign declarations. There are no general slices, growable vectors/strings,
effect checker or checked foreign resource lifetimes. Calls currently trap
above 32 active Rill calls, including `main`; this temporary guard does not
establish general stack safety. See the [ownership limits](docs/blockers/ownership.md),
[allocation contract](docs/blockers/owned-allocation.md) and
[stack-safety blocker](docs/blockers/stack-safety.md).

## Projects, libraries and tools

```sh
cargo run --locked -- new my-tool
cargo run --locked -- project check my-tool
cargo run --locked -- project run my-tool
cargo run --locked -- project build my-tool
cargo run --locked -- project watch my-tool
```

[Project manifests](docs/projects.md) select entry points, outputs and explicit
native/shader recipes. Watch builds a candidate before restarting the process
and keeps the old process when a build fails. Exact unchanged pure source graphs
can reuse checked generated C; native compilation and linking still run.
Changed graphs recheck fully. See [build scaling](docs/build-performance.md).

File modules use explicit imports and exports. The compiler bundles
[19 library modules](stdlib/README.md), including Option/Result, checked math,
text/byte utilities, JSON, allocation, supplied CLI-token parsing and graphics
bindings. Runtime argv, general I/O and a package manager remain open. Generic
Copy helpers and owner-compatible modules have different limits; the
[library guide](docs/standard-library.md) explains which can be combined.

Source `@include`, `@link` and `@pkg_config` directives support native builds.
The importer generates referenced supported C declarations; explicit `extern`
declarations and selected `import-c` bindings remain available. Target header
checks establish signature compatibility, not pointer lifetimes, ownership or
foreign effects. Unsupported ABI forms reject. See [C interop](docs/c-interop.md).

[Scalar and Copy-wire C exports](docs/c-exports.md), [interpreter hooks](docs/hot-reload.md)
and [native generation replacement](docs/native-hot-reload.md) are separate
experimental APIs. The scalar native API accepts closed `i32` functions. A
separate bounded Copy-wire API carries closed Copy values. `live_reload::LiveSession`
adds versioned host-owned Copy state and explicit migration between exclusive
host calls on macOS/Linux. These APIs do not migrate owners, active callbacks,
foreign resources or GPU resources. See the
[stateful reload renderer example](examples/reload-renderer/README.md).

The [formatter](docs/source-format.md) writes validated output to stdout;
`--check` checks formatting without editing files. The
[LSP and editor packages](docs/editor-support.md) provide diagnostics,
definitions, completion, signature help and inferred hints. Helix is the
primary tested editor; Neovim, VS Code and Tree-sitter integrations are included.

## Install the alpha SDK

The `v0.1.0-alpha` distribution recipe bundles SDL3, Raylib, Slang, SPIR-V
validation, native bridges, headers, libraries, examples and licenses. A platform
C11 compiler/SDK is still required. The prepared target matrix is macOS arm64,
macOS x86_64 and Linux x86_64 with glibc. The
[verification record](docs/verification/2026-11-05-graphics-distribution.md)
distinguishes local macOS arm64 evidence from targets awaiting execution.

From this checkout, install a published release into `$HOME/.local`:

```sh
sh tools/install/install.sh --version v0.1.0-alpha
```

This requires published release assets; packaging support alone does not mean
an asset is available. For a locally built archive:

```sh
sh tools/install/install.sh --version v0.1.0-alpha \
  --archive target/dist/rill-0.1.0-alpha-aarch64-apple-darwin.tar.gz \
  --checksum target/dist/rill-0.1.0-alpha-aarch64-apple-darwin.tar.gz.sha256
```

Choose the host's asset name. The installer verifies SHA-256, retains versioned
SDK directories and selects one through `bin/rill`. Add `$HOME/.local/bin` to
`PATH` if needed; shell profiles are not edited. See
[installation](docs/install.md) and [distribution](docs/distribution.md) for
custom prefixes, offline use and building archives.

## Try the graphics examples

Copy an installed example from `share/rill/examples` to a writable directory,
or use this checkout:

```sh
rill project run examples/raylib
rill project run examples/engine-3d
rill shader build examples/engine-3d/rill.shaders.json
```

The [Raylib example](examples/raylib/README.md) uses a scalar bridge for 3D
drawing and text. The [SDL3 engine](examples/engine-3d/README.md) keeps scene,
camera, mesh and transform logic in Rill while a C11 adapter handles GPU
descriptors, uploads and cleanup. Its shaders use external
[Slang tooling](docs/shaders.md). Raylib's OpenGL renderer does not consume
those SPIR-V/MSL artifacts. The [voxel sandbox](examples/sdl3-voxel/README.md)
uses external Metal compute shaders and has a bounded Rill CPU reference.

These examples do not establish checked GPU lifetimes or implement the proposed
Rill shader language. See the [graphics audit](docs/graphics-feature-audit.md)
before expanding that boundary.

## Documentation and contributing

```sh
cargo run --locked -- docs --language
cargo run --locked -- docs my-tool
cargo run --locked -- project docs my-tool --open
```

The [docs command](docs/docs-command.md) generates offline HTML with symbol and
section search without running project code or native dependency tools. Output
defaults to `build/docs/index.html`. Keep guides in `docs/` and library docs
beside `stdlib/`; generated HTML stays out of Git.
[Documentation maintenance](docs/docs-maintenance.md) explains validation and
the prepared GitHub Pages workflow.

Before changing code, read [AGENTS.md](AGENTS.md), [PLAN.md](PLAN.md),
[phase status](docs/phase-status.md) and the [active roadmap](docs/CONTINUATION_PLAN.md).
The [active roadmap](docs/CONTINUATION_PLAN.md) starts with repeatable regression
checks and performance baselines, then bounded ownership/library improvements.
Broader backends and advanced features need their stated prerequisites.

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

Tests compile C using `cc`. SDL3, Zig and GPU checks depend on their tools and
devices and may report skips. Record those separately from executed tests.
See [phase status](docs/phase-status.md) for evidence and current CI limits.
