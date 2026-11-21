# A small 2D scene engine

This turn-based game exercises a reusable Rill scene graph. Collect the star
with the player (`@`). The backpack (`o`) follows the player through its parent
transform. Type `h`, `j`, `k` or `l` for left, down, up or right and press
Enter. A line can contain multiple moves. Quit with `q` or EOF to print the score.

```sh
cargo run --locked -- run examples/scene-engine/main.rill
printf 'lllq' | cargo run --locked --quiet -- run examples/scene-engine/main.rill
```

The scripted run prints four frames and a final score of 1. The terminal uses
the C library for character I/O. The engine, rasterizer and game logic are Rill.

* `scene.rill` holds up to 16 ordered nodes in one 97-byte Copy value. Spawn
  a root or attach to an earlier node. Failure returns None before mutation.
  IDs stay valid for the lifetime of the scene. The model supports translation,
  rectangular sprites, clipping in the renderer and half-open hit testing.
* `game.rill` creates the player, backpack and target. A command advances one
  turn. Contacts update the target; insertion order sets draw order on the
  20 by 8 byte-cell display.
* `main.rill` renders frames on stdout and accepts commands on stdin. The
  [existing SDL voxel application](../sdl3-voxel/README.md) exercises a
  separate real-time renderer and input loop.

Run `cargo test --locked --test scene_engine` for interpreter/C agreement on
nested transforms, bounds, capacity, collisions and actual game frames. The
next engine work is asset loading, removal with generation-checked handles,
event dispatch and integration with an interactive graphical scene editor.

## SDL3 window

With SDL3 development files, pkg-config and a C compiler installed, run from
the repository root:

```sh
cargo run --locked -- run examples/scene-engine/window.rill \
    --c-arg -Iexamples/scene-engine --link-arg examples/scene-engine/scene_window.c
```

Move the blue player using arrow keys or `h`/`j`/`k`/`l`. The green backpack
follows one cell below. Collect the gold target to increment the score in the
window title. Escape, `q` or closing the window exits and prints the score.
A window or rendering failure prints `-1` and releases acquired SDL resources.

The window entry imports the same `game` and `scene` modules as the terminal
entry. Rill handles movement, collisions and the 20-by-8 framebuffer. The
header-checked C adapter maps SDL events to commands and paints the final
cells into a 640-by-256 window using SDL's software renderer. Repaints of an
exposed window preserve the current turn.

`cargo test --locked --test scene_sdl -- --nocapture` uses SDL's dummy video
driver, sends three right-arrow events and a quit event, and checks the actual
rendered colors and final score at C optimization levels 0 and 3. It also
forces video initialization and frame presentation failures and checks cleanup. The test reports a
skip if pkg-config or the SDL3 development files are unavailable.
