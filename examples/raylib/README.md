# raylib demos in Rill

Two programs against a system-installed raylib, with no build file: the script
names the header and the library, and the compiler reads the header and links
the library for it.

```console
$ rill exec examples/raylib/window.rill        # a bouncing rectangle
$ rill exec examples/raylib/sidescroller.rill  # the game
```

The game: arrow keys walk, `SPACE`/`UP` jumps, `ESC` or the window's close
button quits. Coins print a line when collected, and the score is the process
exit code.

## What raylib has to be installed

A library and its header the compiler can find:

* macOS with Homebrew: `brew install raylib` installs
  `/opt/homebrew/include/raylib.h` and `/opt/homebrew/lib/libraylib.dylib`, both
  searched by default.
* Linux: `apt install libraylib-dev` (or the distribution's equivalent) puts the
  header in `/usr/include` and the library in `/usr/lib`.
* Anything else: `RILL_INCLUDE_PATH` and `LIBRARY_PATH` move the search.

## How the C side is declared

```rill
@link("raylib");                     # link -lraylib, searched in the loader's paths

type RL = @include("raylib.h");      # read the header: functions, types, constants
use RL::*;                           # their bare names are now in scope
```

raylib marks its prototypes `RLAPI` and hands records over by value, which is
what the import reads:

* every `RLAPI …` prototype becomes an `extern "c"` function with an `<io>` row
  (C can do anything, so the effects are the conservative ones), and the marker
  is dropped the way SDL's `SDLCALL` is;
* every `typedef struct` becomes a record whose C name is kept, so `Rectangle`,
  `Vector2` and `Color` are values the program builds and passes the way raylib
  expects - `ClearBackground (Color (18, 20, 28, 255))`;
* every `enum` becomes `i32` and its enumerators become constants, so
  `KEY_ESCAPE` and `FLAG_WINDOW_HIDDEN` are ordinary names;
* every `#define NAME <literal>` becomes a constant that emits the literal, so
  `PI` is usable as written.

Types that cannot be expressed (variadic functions, function-pointer parameters,
arrays) are left out, and a name that never appears in the header reports as an
unknown name at its use.

## Running without a window on screen

raylib has no headless driver of its own, so the demos ask for a hidden window
(`SetConfigFlags (FLAG_WINDOW_HIDDEN)`) and run a fixed number of frames when
`RILL_RAYLIB_DEMO` is set, printing a summary at the end:

```console
$ RILL_RAYLIB_DEMO=1 rill exec examples/raylib/sidescroller.rill
score 1
score 2
score 3
demo frames 240 score 3
```

The demo mode walks the player to the right at a fixed step, so the score is the
same on every run and the test can pin it. The environment is read through
libc's `getenv`, declared in the script itself - raylib has no hint API:

```rill
extern "c" let getenv (name: Text) -> Text <io>;
```

`crates/rill-driver/tests/c_import.rs` builds and runs both demos this way, and
skips them when the header is not installed.
