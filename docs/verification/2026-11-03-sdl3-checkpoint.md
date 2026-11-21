# SDL3 continuation on macOS

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](2026-11-04-ernie-integration.md).

This increment checks the existing native application and fixes failures found
during that work. It does not complete a PLAN.md phase. The language remains
experimental, with the limits in [phase status](../phase-status.md).

## Scope and implementation

The acceptance checks for this increment are actual SDL3 execution through
direct and generated bindings, failure results with exact cleanup, real pixel
checks across C optimization levels, and native ABI checks that retain control
of the compilation action. Each change has a separate regression:

- The direct game previously ignored drawing/presentation failures. It now
  stops at the first failure, prints `-4`, and destroys the renderer, window
  and SDL state in order. Quit events stop work before another draw.
  [`sdl3_lifecycle.rs`](../../tests/sdl3_lifecycle.rs) checks 15 scenarios at
  both `-O0` and `-O2`, including held clicks and all drawing failure sites.
  These cases use a C fixture, not the actual SDL library.
- Native `check` and `emit-c` previously accepted mismatched declarations when
  `--c-arg -E` or a similar driver action skipped type checking. Shared argument
  validation now rejects action, language and output overrides for checking
  and building. [`native_validation.rs`](../../tests/native_validation.rs)
  checks rejection, unchanged existing outputs, ordinary flag acceptance and
  header mismatches even with warning suppression. Configured headers and
  toolchains remain trusted.
- Framebuffer capture failed on macOS at GNU `realpath -m`, and its linker
  interception also required GNU tooling. The script now uses portable Bash
  and Python with compile-time presentation-call redirection. It captures
  both original games and exits through their normal SDL quit handling.
  [`sdl3_capture.rs`](../../tests/sdl3_capture.rs) checks actual background,
  gate, moving line, score area, player and target pixels, then compares BMPs
  byte for byte between `-O0` and `-O2`. It also checks overwrite rejection.
  Python cancellation cleanup was exercised by terminating the supervisor
  and verifying that its separate game process no longer existed.

## Environment and commands

The native runs used AArch64 macOS, Rust 1.99.1, Apple Clang 17, SDL3 3.4.18,
and the installed Zig C compiler. No physical keyboard/mouse input or GPU
rendering was tested. Real captures used SDL's software renderer and dummy
video driver. Both framebuffer images were also inspected visually.

On this host, libclang initially failed to find `AvailabilityMacros.h`.
Selecting the active SDK fixed the generated-binding integration:

```sh
export SDKROOT="$(xcrun --show-sdk-path)"
cargo test --locked --features c-import \
  --test sdl3_direct --test sdl3_capture --test sdl3_lifecycle -- --nocapture
```

All five tests passed. The real-library direct and generated tests each build
and execute a three-frame variant via both `build` and native `run`; the third
real-library test does the same with `zig cc`. All print `0`.

```sh
cargo build --locked
RILL_BIN="$PWD/target/debug/rill" scripts/capture-sdl3.sh /tmp/rill-sdl3-o0
RILL_BIN="$PWD/target/debug/rill" RILL_CAPTURE_OPT_LEVEL=2 \
  /bin/bash scripts/capture-sdl3.sh /tmp/rill-sdl3-o2
```

Both captures passed on stock macOS Bash 3.2, including paths with spaces.
Frame 40 of the direct game has the gold line at x=313, between the green
lines at x=280 and x=360. Frame 1 was also captured successfully. The collector
shows its player at (300, 220) and target at (100, 100).

The unmodified direct game's full 900-frame desktop round also completed at
`-O2`, printed `0` and produced no stderr, with `SDL_VIDEODRIVER` and
`SDL_RENDER_DRIVER` unset. The first sandboxed attempt printed `-3` with macOS
GUI-service connection errors. Running the same built executable outside the
sandbox succeeded. This establishes desktop startup, drawing and normal
shutdown on this host; no physical input was supplied.

The [syntax tour](../syntax-tour.md) contains five complete programs. Each
passed `check`, `run` and an `-O2` native build with matching output.

## Complete regression coverage

Final coverage includes all 83 test executables in each configuration, plus
doc tests:

| Configuration | Final passing cases | Remaining failures |
| --- | ---: | ---: |
| Default | 544 | 0 |
| `c-import` | 577 | 0 |

For the complete runs, Cargo built and enumerated its test executables with
`cargo test --locked --no-run --message-format=json`, adding
`--features c-import` for the importer configuration. Every executable ran with
`--test-threads=2`, with three targets running at a time. Doc tests ran through
`cargo test --locked --doc` in both configurations.

The importer run initially had six failures in its binary test target: a
200 ms fixture deadline expired before startup and poisoned the shared fixture
mutex. A subsequent rerun also exposed a 3-second expected-error deadline under
the concurrent C compilation load. Only test allowances changed. Timeout
fixtures now get five seconds and retain 15-second completion bounds; other
worker fixtures get ten seconds. Startup markers, exact error checks and
workspace/descendant cleanup assertions remain. The final affected-target run
passed all ten tests:

```sh
cargo test --locked --features c-import --bin rill -- --test-threads=2 --nocapture
```

The table combines the complete runs with that final affected-target rerun.
Production importer deadlines and supervision behavior did not change.

Three distinct filesystem-dependent cases report explicit capability skips
because this macOS filesystem/sandbox rejects invalid-byte filenames with
`EPERM` or `EILSEQ`. The default suite encounters two; the importer adds one
subcase. Valid relative paths and raw argument/protocol bytes remain covered.
No real SDL3 or Zig test was skipped.

Formatting, strict all-target Clippy in both default and `c-import`
configurations, shell syntax and final diff checks also passed. The configured
C compiler still emits existing cosmetic warnings for some ordinary examples;
the desktop application itself produced no stderr.

## Remaining boundaries

SDL resources still use manual cleanup. Raw foreign pointers have no checked
lifetime, effect or ownership contract, and the restricted owner backend
rejects foreign declarations. The renderer integration is evidence for this
application on this target, not the complete P5/P6 gates or stable ABI support.
The earlier Linux evidence remains in the [original report](2026-11-03-sdl3-native.md).
