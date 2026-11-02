# SDL3 collector

Move the blue square with the arrow keys to collect gold squares. The window
title shows the score. Press Escape or close the window to quit.

This example needs Rust, a C11 compiler, pkg-config, and SDL3 development files.
From the repository root, build it with these Bash commands:

```bash
cc -std=c11 -c examples/sdl3/shim.c $(pkg-config --cflags sdl3) -o /tmp/rill-sdl3-shim.o
link_args=()
for arg in $(pkg-config --libs sdl3); do link_args+=(--link-arg "$arg"); done
cargo run --quiet -- build examples/sdl3/collector.rill -o /tmp/rill-collector \
  --link-arg /tmp/rill-sdl3-shim.o "${link_args[@]}"
/tmp/rill-collector
```

`cargo test --test sdl3_game` runs 80 frames against a headless C stub. It
checks movement, bounds, collection and loop exit without loading SDL or opening
a window.

Game state, movement, bounds, collision checks, and the game loop are in Rill.
The C shim owns SDL objects and handles input and drawing. Its declarations are
manually written foreign contracts. The
reference interpreter cannot call them, so this example requires C emission.
