# Live functions in a persistent renderer

This Rust embedding example keeps one host process and Raylib window alive
while replacing pure native Rill functions. The moving rectangle's state lives
in a checked Copy record. Each completed frame saves that record to a JSON
checkpoint, so a later host process resumes its ticks and position.

The ordinary `rill project watch` command still rebuilds and restarts programs.
This example uses the separate experimental `live_reload::LiveSession` API on
macOS and Linux. It does not complete a PLAN phase or establish a stable ABI.

## Run and edit

From the repository root, select an installed alpha SDK through
`RILL_TOOLCHAIN_ROOT`, or provide `rill-raylib` through system pkg-config. The
example requires a C11 compiler and a real window/GPU driver. The SDK resolver
supplies the actual headers and ordered linker arguments.

```sh
mkdir -p target/reload-renderer
cp examples/reload-renderer/logic.rill target/reload-renderer/logic.rill
cargo run --example native_reload_renderer -- \
  --source target/reload-renderer/logic.rill \
  --checkpoint target/reload-renderer/state.json
```

Edit `target/reload-renderer/logic.rill` while the window runs. Change
`state.x + 1` to `state.x + 3`. After the next successful build, movement uses
the new update function without resetting ticks, position or the window.
The host polls source bytes between frames and reports accepted generations.
Candidate compilation pauses the frame loop; it does not run on a worker.
This host watches one pure library file and its locally declared helpers.
It does not discover an imported project graph or run native/shader recipes.

Change the `color` function's result from `70` to `199`. The rectangle keeps
the previous color because same-schema edits select only `update`. Its new body and helpers
are a separate native unit; the old `color` unit remains loaded and callable.
The printed `update` and `color-generation` values show the difference.
Invalid source prints a rejection and the previous functions keep running.

To exercise the example's explicit state migration policy, replace the watched
source while the same window runs:

```sh
cp examples/reload-renderer/logic-speed.rill target/reload-renderer/logic.rill
```

The first line, `// state-version: 2`, is a convention interpreted by this Rust
host. It is an ordinary Rill comment, not language syntax. Version 2 adds
`speed` to `Scene`. The host replaces `update` and `color` together, checks both
new signatures and migrates the previous ticks and x with `speed = 2`. Code and
state publish together only after validation and migration succeed. Restoring
the version-1 source now rejects because this host has no reverse migration.
The existing version-2 functions, state and window remain usable.

Escape closes the window normally. Run the same command again with the same
source and checkpoint to resume the saved state. A new process creates a new
window once; it does not restore old OS/GPU handles. Generation counters start
again at zero, while ticks and position come from the checkpoint. Delete only
the example checkpoint when a fresh state is wanted.

## Logic checks without graphics

```sh
cargo run --example native_reload_renderer -- --headless --frames 3 \
  --source target/reload-renderer/logic.rill \
  --checkpoint target/reload-renderer/state.json --opt-level 3
cargo test --locked --test native_reload_renderer -- --nocapture
```

`--headless` runs native logic and checkpoints only. It creates no window,
renderer or GPU and provides no graphics evidence. `--frames N` bounds either
mode; omitting it runs until the window closes. `--interval-ms N` sets the poll
delay after each frame. The actual renderer also targets 60 FPS.

Tests share the example's source, state-schema and reload-loop module. A
GPU-independent C shared-library fixture exercises the same `dlopen`/`dlsym`
loader and fixed dispatcher used by the host. It counts real open/frame/close
calls while the reload loop checks retained color, state migration and
checkpoint restoration across two host processes. The fixture draws nothing.
A separate test uses the actual Raylib bridge and window. It reports an
explicit skip if the provider or device is unavailable. To require real graphics:

```sh
cargo build --locked --example native_reload_renderer
RILL_REQUIRE_GPU=1 \
RILL_RELOAD_RENDERER_EXE="$PWD/target/debug/examples/native_reload_renderer" \
  cargo test --locked --test native_reload_renderer \
  real_raylib_window_runs_without_restarting_for_logic_replacement -- --nocapture
```

The test launches the standalone host so native window calls run on its main OS
thread. It writes the source while that process runs, verifies selective
replacement and checks a real PNG readback. It uses a hidden real GPU window.

Set `RILL_RELOAD_CAPTURE` to a PNG path to capture each real rendered frame to
that file, replacing the previous capture. Readback and temporary image cleanup
remain in the permanent C adapter.

## Boundaries

`native/renderer.c` includes the SDK's real `rill/raylib.h` and exports the
fixed dispatcher declared in `native/renderer.h`. The host compiles and loads
that adapter once, balances frame scopes and closes the window once before
unloading. `--renderer path` selects an already compiled trusted adapter with
that exact header contract. Native code and the selected C compiler are
trusted.

Only Copy state crosses reload units. The checkpoint contains schema, version
and values, with no pointers, owned Rill allocations, native handles or code
addresses. The host opens the renderer before validating/restoring the
checkpoint; a malformed checkpoint aborts startup, and dropping the host closes
an adapter that already opened successfully. Writes replace a same-directory
temporary file after syncing its contents. This is not a database or a guarantee
against every power-loss failure.

Every reload occurs after the previous synchronous Rill calls and drawing
scope return. Renderer code, driver callbacks and GPU resources remain in the
permanent adapter and its Raylib dependency. No pointer into a Rill generation
is retained by the renderer. This does not implement shader reload, active
frame patching, arbitrary concurrent callbacks, GPU resource migration or
destructor-bearing state migration. Arithmetic/depth traps in native Rill code
still terminate the host process; a rejected candidate does not recover from
an executed trap. See [native reload](../../docs/native-hot-reload.md).
