# SDL3 timing game

Click any mouse button when the moving gold line is between the green lines.
Each consecutive hit adds a gold score segment. A mistimed click resets the
score. Release the button before clicking again. Close the window to finish,
or play until the 900-frame round ends, roughly 15 seconds. The program prints
the final score. Negative results indicate SDL initialization, window, or
renderer failure.

Install SDL3 development headers and libraries, pkg-config, Rust, and a C11
compiler. From the repository root:

```sh
cargo run -- build examples/sdl3-direct/main.rill -o /tmp/rill-sdl3-direct
/tmp/rill-sdl3-direct
# Or compile and run in one command:
cargo run -- run examples/sdl3-direct/main.rill
# Select Zig's C compiler, if Zig is installed:
cargo run -- run examples/sdl3-direct/main.rill --zig-cc
```

The game imports the reusable `sdl.rill` binding module, which exports SDL
functions and constants and declares the SDL header and library dependencies.
The build obtains
compiler and linker search flags from `pkg-config sdl3`. No C shim or separately
compiled object is needed. `SDL_Window` and `SDL_Renderer` remain opaque typed
pointers. The declarations match SDL3's boolean results, unsigned flags, byte
color channels, and float coordinates. The generated C includes SDL's real
header, so the C compiler checks these calls against the installed API.

The game uses SDL's software renderer. Event pumping updates the mouse state;
`SDL_HasEvent(SDL_EVENT_QUIT)` detects window closure without exposing the
`SDL_Event` union layout. The game uses the mouse button flags without reading
mouse coordinates, so both coordinate output pointers are null.

`cargo test --test sdl3_direct -- --nocapture` builds and runs a three-frame
variant against the installed SDL3 library with its dummy video driver, through
both `build` and native `run`. It prints an explicit skip reason if SDL3
development files are unavailable.
