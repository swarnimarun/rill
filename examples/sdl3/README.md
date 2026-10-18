# SDL3 demos in Rill

Three programs against a system-installed SDL3, with no build file: the script
names the header and the library, and the compiler reads the header and links
the library for it.

```console
$ rill exec examples/sdl3/window.rill        # a bouncing rectangle
$ rill exec examples/sdl3/plasma.rill        # sine waves, point by point
$ rill exec examples/sdl3/sidescroller.rill  # the game
```

The game: arrow keys walk, `SPACE`/`UP` jumps, `ESC` or the window's close
button quits. Coins print a line when collected, and the score is the process
exit code.

## What SDL3 has to be installed

A library and its headers the compiler can find:

* macOS with Homebrew: `brew install sdl3` installs
  `/opt/homebrew/include/SDL3` and `/opt/homebrew/lib/libSDL3.dylib`, both
  searched by default.
* Linux: `apt install libsdl3-dev` (or the distribution's equivalent) puts the
  header in `/usr/include/SDL3` and the library in `/usr/lib`.
* Anything else: `RILL_INCLUDE_PATH` and `LIBRARY_PATH` move the search.

## How the C side is declared

```rill
@link("SDL3");                          # link -lSDL3, searched in the loader's paths

type SDL3 = @include("SDL3/SDL.h");     # read the header: functions, types, constants
use SDL3::*;                            # their bare names are now in scope
```

`@include` scans the header and its includes once, at lowering:

* every `extern … SDLCALL name(…)` becomes an `extern "c"` function with an
  `<io>` row (C can do anything, so the effects are the conservative ones);
* every `typedef struct`/`union` becomes a record whose C name is kept, so the
  generated code says `typedef SDL_FRect …` instead of declaring a struct of
  its own - the layout is the header's, not the compiler's;
* every `enum` becomes `i32` and its enumerators become constants;
* every `#define NAME <literal>` becomes a constant that emits the literal.

Types that cannot be expressed (variadic functions, function-pointer
parameters, arrays) and platform-only declarations are left out, and a name
that never appears in the header reports as an unknown name at its use.

A `T *` parameter is passed as a place when it is a struct, union or scalar
(`SDL_RenderFillRect (ren, inout rect)`, which emits `&rect`) and as a pointer
value when it is an opaque handle (`SDL_DestroyWindow (win)`).

## Running without a display

`RILL_SDL3_DEMO=1` puts each demo into a fixed-length run that prints a summary
and exits - no window loop, no input needed. With SDL's dummy video driver it
needs no display at all, which is how `crates/rill-driver/tests/c_import.rs`
builds and runs all three:

```console
$ RILL_SDL3_DEMO=1 SDL_VIDEODRIVER=dummy rill exec examples/sdl3/sidescroller.rill
score 1
score 2
score 3
demo frames 240 score 3
```

The demo mode walks the player to the right at a fixed step, so the score is
the same on every run and the test can pin it.
