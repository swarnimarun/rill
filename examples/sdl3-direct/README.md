# SDL3 timing game

Click any mouse button when the moving gold line is between the green lines.
Each consecutive hit adds a gold score segment. A mistimed click resets the
score. Release the button before clicking again. Close the window to finish,
or play until the 900-frame round ends, roughly 15 seconds. The program prints
the final score. Negative results report a failure: `-1` for window creation,
`-2` for renderer creation, `-3` for SDL initialization, and `-4` for drawing or
presentation. A failed draw stops the round immediately. Every normal exit,
including those failures, destroys the renderer before the window and calls
`SDL_Quit` once. Only resources that were created are destroyed. These numbers
are printed Rill results, not process exit codes.

Install SDL3 development headers and libraries, pkg-config, libclang 12 or newer,
Rust, and a C11
compiler. From the repository root:

```sh
cargo run -- build examples/sdl3-direct/main.rill -o /tmp/rill-sdl3-direct
/tmp/rill-sdl3-direct
# Or compile and run in one command:
cargo run -- run examples/sdl3-direct/main.rill
# Select Zig's C compiler, if Zig is installed:
cargo run -- run examples/sdl3-direct/main.rill --zig-cc
```

Boolean negation accepts `not ready` and `!ready`. A braced `if` can omit `else`
when its body returns unit. The game conditionally creates its resources,
keeps the appropriate failure result and calls `SDL_Quit` once at the end.

The game imports `sdl.rill`, which contains only `@include`, `@link` and
`@pkg_config`. The compiler reads the header at build time and automatically
exports referenced SDL functions and constants. No handwritten externs or
symbol-selection list is needed.
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

`cargo test --test sdl3_lifecycle` uses a deterministic C fixture without an
SDL3 installation. It checks a held click, a mistimed click, window closure,
each initialization failure, every drawing failure, and exact destruction
order at `-O0` and `-O2`. The fixture does not establish actual SDL rendering or
physical input behavior.

## Explicit binding files

Automatic `@include` import is the normal path. The separate `import-c` command
can also generate an ordinary binding file when an explicit artifact is useful:

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
needs it. Generation is not a lifetime/resource-safety proof. Both automatic and
explicit import preserve `SDL_EVENT_QUIT`'s actual C enum-constant type,
normally `i32`; the game already converts it to `u32`. To use the explicit file,
import `sdl_generated as sdl`. Other selected declarations use the same supported ABI.
This generation-and-use path has an optional integration regression; it reports
an explicit skip when SDL3 development files are unavailable.

Automatic source import discovers the host SDK on macOS unless target/sysroot
options are supplied. The separate explicit `import-c` command may need the
active SDK location to find system headers:

```sh
export SDKROOT="$(xcrun --show-sdk-path)"
cargo test --locked --features c-import --test sdl3_direct -- --nocapture
```

Successful checks and builds publish native declarations for tool-free editor
analysis. Run `cargo run -- check examples/sdl3-direct/main.rill` before opening
the include-only example in Helix. After adding a new C name or changing header
configuration, run the check again to refresh the editor snapshot.

The real-library tests also exercise Zig when it is installed. To capture both
games without opening windows, run from the repository root:

```sh
cargo build --locked
RILL_BIN="$PWD/target/debug/rill" scripts/capture-sdl3.sh /tmp/rill-sdl3-captures
cargo test --locked --test sdl3_capture -- --nocapture
```

Use a new capture directory. The script runs on macOS and Linux with Bash,
Python 3, pkg-config, SDL3 and a C11 compiler. It saves real software-rendered
BMP files. The regression checks the gate, moving line, player and target
pixels, and requires identical frames at `-O0` and `-O2`.
