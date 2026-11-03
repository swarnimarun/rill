# Typed SDL3 capture matrix

[`scripts/capture-sdl3-matrix.py`](../scripts/capture-sdl3-matrix.py) is an
**opt-in acceptance tool**, prepared for the F3 integration checkpoint. Its
presence is not evidence that typed raw foreign transport, this harness, or
any new captures have passed. Run it only after the integrated F2 compiler is
built and verified. P1 and P5 remain open.

The existing [`capture-sdl3.sh`](../scripts/capture-sdl3.sh) command and its
default direct/collector behavior are unchanged. Old capture output must not
be reused or overwritten.

## Prerequisites and invocation

Use Linux with a GNU-compatible `--wrap` linker, Python 3, GNU `timeout`, Git,
pkg-config, SDL3 development files, a C11 compiler, Clang/libclang, and an
**already-built `c-import`-enabled Rill executable** from the integrated
checkpoint. Configure the same SDL/libclang environment used by the native
tests. A relocated installation may require `PKG_CONFIG_SYSROOT_DIR`,
`PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`, and `LIBCLANG_PATH` as described in the
[earlier real-SDL report](verification/2026-11-03-sdl3-native.md).

The matrix never invokes Cargo, installs packages, or creates a compiler build
cache. Reserve an approved build target separately, build Rill there, and
record the binary's source checkout/build provenance before running:

```sh
export RILL_BIN=/path/to/already-built/rill
python3 -B scripts/capture-sdl3-matrix.py /tmp/new-sdl3-f3-gcc
python3 -B scripts/capture-sdl3-matrix.py /tmp/new-sdl3-f3-clang --cc clang
```

Each output path must be new, including when retrying a failed run. Existing
files, directories, symlinks and dangling symlinks are refused. Partial evidence
is retained on failure; no artifacts are deleted. The output parent must exist.
Paths containing spaces are supported. Compiler selections are single
executable paths, never shell command strings. Arguments use direct process
argv; pkg-config flags are parsed with `shlex`, never evaluated by a shell.

`--rill` overrides `RILL_BIN`; `--cc` overrides `CC`; `--clang` overrides
`CLANG_PATH`. The importer resource directory comes from
`--clang-resource-dir`, then `RILL_TEST_CLANG_RESOURCE_DIR`, then the selected
Clang's `-print-resource-dir`. `--frame` overrides `RILL_CAPTURE_FRAME` and
defaults to 40, with bounds 1–900. Both optimization levels are always run.

## Required gates

The command snapshots the original direct game, its hand-written bindings,
the collector/shim, and the capture harness. It uses libclang to generate the
same explicitly selected SDL binding subset as `tests/sdl3_direct.rs`. The
generated game differs only by converting `SDL_EVENT_QUIT` from the imported
`i32` enum constant to the event query's `u32` argument. The adaptation fails
closed if the source call changes. Game timing, drawing and input queries are
not rewritten or replaced.

For **direct, generated, and collector at both `-O0` and `-O2`**, the tool:

1. Retains `rill emit-c` output and rejects empty C or the uniform representation
   identifiers `rl_value`/`RL_CAP` before building the executable.
2. Compiles that exact retained C against the real SDL library, with the same
   optimization on the capture wrapper and collector shim. Incompatible pointer
   types and implicit function declarations are C errors. There is no stub SDL.
3. Runs under `SDL_VIDEODRIVER=dummy` and `SDL_RENDER_DRIVER=software`. The
   existing `SDL_RenderPresent` wrapper saves real pixels before presentation
   and posts an ordinary quit event after the selected frame.
4. Requires normal exit, exact `0\n` stdout, the wrapper's frame/dimension/
   software-renderer/dummy-driver report, expected game dimensions, and
   distinctive background/gate/player/target pixels.
5. Decodes the actual BMP to canonical top-down RGBA and writes an unmodified,
   lossless PNG. No resize, overlay, annotation, interpolation, or mock redraw
   is performed. Unsupported/truncated BMP formats are rejected explicitly.
6. Requires exact dimensions and RGBA bytes to match between direct/generated
   at each optimization and between O0/O2 for each game. The collector is a
   different scene and is never compared to the direct game.

Normal commands have a 60-second timeout; captures have 30 seconds, followed by
a five-second termination grace period. The importer's own worker deadline
still applies. Missing dependencies or failed checks are **failures**, not
successful skips. An interrupted/failed run is not acceptance evidence.

## Evidence and remaining verification

Each `O0/{direct,generated,collector}` and `O2/{direct,generated,collector}`
directory retains `program.c`, dependency metadata, the exact executable,
`frame.bmp`, and `frame.png`. Object files, generated bindings, source copies,
and stdout/stderr for each command are also retained. `commands.jsonl` records
the exact argv, cwd, environment overrides, status and timing. Each command has
a start record written before launch and a matching result record, including
spawn errors and interruptions. An interrupted result does not claim that all
descendants have terminated.

`manifest.json` records pass/failure, captured dimensions, pixel probes,
canonical RGBA/BMP/PNG SHA-256 values, comparison outcomes, C transitive-header
content hashes, input and artifact hashes, binary/compiler/Git/pkg-config
identities and versions, SDL version, checkout commit/diff/status and relevant environment.
The Rill binary's hash does **not** establish that it came from the recorded
checkout. Preserve the separate build provenance. The environment record and
dependency files are diagnostic evidence, not a hermetic-toolchain certificate;
shared-library resolution still depends on the configured host.

Inspect all six PNGs visually after a successful run. The manifest's pixel
probes do not replace visual review. These are native software-framebuffer
captures, not physical desktop screenshots, input-interaction testing, or GPU
rendering evidence. They do not establish other targets or cross-compiler
equivalence. Compare canonical dimensions/RGBA hashes from separate compiler
runs only when sources, frame, SDL and other relevant inputs match.

This harness compiles retained emitted C directly so the representation check
and executed program have the same C bytes. It does not replace the separate
three-frame direct/generated **Rill `build` and `run` CLI** tests, their O0/O2
and typed-representation gates, the full F2/F3 native/optimized/importer
regression matrix, or the no-host-FFI evaluator contract.

The harness's standalone tests cover BMP encodings/orientation/padding,
malformed formats, PNG byte preservation/CRC, overwrite refusal, the narrow
source adaptation, uniform-C rejection, escaped dependency paths, and command
attempt/result logging. Process-launch tests replace `subprocess.run`; they do
not execute external tools:

```sh
python3 -B -m unittest discover -s scripts -p 'test_sdl3_capture_matrix.py'
```

The tiny format fixtures in those tests are not SDL results or screenshots.
Record actual test and capture results separately before making acceptance
claims. Runtime evidence is recorded separately in the
[integrated native/SDL checkpoint](verification/2026-11-03-typed-native-raw-integration.md):
12 actual captures passed across GCC/Clang and O0/O2. The source contract above
still requires fresh validation for a changed compiler or environment.
