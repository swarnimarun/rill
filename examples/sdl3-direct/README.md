# SDL3 timing game

Click any mouse button when the moving gold line is between the green lines.
Each consecutive hit adds a gold score segment. A mistimed click resets the
score. Release the button before clicking again. Close the window to finish,
or play until the 900-frame round ends, roughly 15 seconds. The program prints
the final score. Negative results are `-1` for window creation, `-2` for renderer
creation, `-3` for SDL initialization, and `-4` for drawing or presentation.
A failed draw stops the round. Every normal exit destroys the renderer before
the window and calls `SDL_Quit` once; only created resources are destroyed.
These are printed Rill results, not process exit codes.

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
`SDL_HasEvent(u32(SDL_EVENT_QUIT))` detects window closure without exposing the
`SDL_Event` union layout. The game uses the mouse button flags without reading
mouse coordinates, so both coordinate output pointers are null.

`cargo test --test sdl3_direct -- --nocapture` builds and runs a three-frame
variant against the installed SDL3 library with its dummy video driver, through
both `build` and native `run`. It prints an explicit skip reason if SDL3
development files are unavailable.

Boolean negation accepts `not ready` and `!ready`. A braced `if` can omit `else`
when its body returns unit. The lifecycle regression checks clicks, closure,
initialization failures, every draw failure and exact cleanup order at O0/O2.
Real-library tests retain typed-C inspection and exercise Zig when available.

## Generate the selected bindings

With SDL3 development files and libclang installed, the optional importer can
regenerate this binding subset. Use a separate file until you review the result:

```sh
cargo run --locked --features c-import -- import-c \
  --header SDL3/SDL.h --pkg-config sdl3 --link SDL3 \
  --constant SDL_INIT_VIDEO --constant SDL_EVENT_QUIT \
  --function SDL_Init --function SDL_CreateWindow \
  --function SDL_CreateRenderer --function SDL_PumpEvents \
  --function SDL_HasEvent --function SDL_GetMouseState \
  --function SDL_SetRenderDrawColor --function SDL_RenderClear \
  --function SDL_RenderLine --function SDL_RenderPresent \
  --function SDL_Delay --function SDL_DestroyRenderer \
  --function SDL_DestroyWindow --function SDL_Quit \
  -o examples/sdl3-direct/sdl_generated.rill
```

Set `LIBCLANG_PATH` or pass `--clang-resource-dir` if your Clang installation
needs it. Generation is not a lifetime/resource-safety proof. Both the manual
fixture and importer preserve `SDL_EVENT_QUIT`'s C enum-constant type, normally
`i32`. The game converts it to `u32`. Import `sdl_generated as sdl` to use the
generated file. Other selected declarations use the same supported ABI.
This generation-and-use path has an optional integration regression; it reports
an explicit skip when SDL3 development files are unavailable.
