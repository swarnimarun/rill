# raylib demos in Rill

Three programs against a system-installed raylib, with no build file: the script
names the header and the library, and the compiler reads the header and links
the library for it.

```console
$ rillc exec examples/raylib/window.rill        # a bouncing rectangle
$ rillc exec examples/raylib/sidescroller.rill  # the game
$ rillc exec examples/raylib/match3.rill        # match 3, assets generated in code
```

The side-scroller: arrow keys walk, `SPACE`/`UP` jumps, `ESC` or the window's
close button quits. Coins print a line when collected, and the score is the
process exit code.

Match 3: click a gem and then a neighbour to swap them; a swap that lines up
three or more clears them, the gems above fall into the gaps and the top of each
column is dealt fresh ones, and a clear that cascades is worth more. `R` deals a
new board.

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

## The match-3 assets are code

`match3.rill` reads nothing from disk, not even a palette. At startup it draws
its own sprites into images with raylib's image API and uploads them as textures:

* six 64×64 gems, one shape per kind - a gradient circle, a gradient square, a
  gradient triangle, a rotated square, a cross, a ring - each in its own three
  colours, which are a `match` over the kind in the source;
* the board's tiles from `GenImageChecked`, one checker square per cell;
* the window background from `GenImageGradientLinear`.

`LoadTextureFromImage` uploads each one and the image is released; the textures
go before the window closes. Nothing is loaded from a file path, so the demo runs
from any directory and has no assets to ship. It is silent: raylib's public API
has no wave synthesis, so there is no generated sound.

```rill
let gem_texture (kind: i32) -> Texture2D <io> = {
    var image = GenImageColor (tile (), tile (), Color (0u8, 0u8, 0u8, 0u8));
    ImageDrawCircleGradient (inout image, Vector2 (32.0f32, 34.0f32), 27.0f32, body, dark);
    var texture = LoadTextureFromImage (image);
    UnloadImage (image);
    texture
};
```

## Running without a window on screen

raylib has no headless driver of its own, so the demos ask for a hidden window
(`SetConfigFlags (FLAG_WINDOW_HIDDEN)`) and run a fixed number of frames when
`RILL_RAYLIB_DEMO` is set, printing a summary at the end:

```console
$ RILL_RAYLIB_DEMO=1 rillc exec examples/raylib/sidescroller.rill
score 1
score 2
score 3
demo frames 240 score 3
```

The demo mode walks the player to the right at a fixed step, so the score is the
same on every run and the test can pin it. `match3.rill` plays itself the same
way, from a fixed seed set *after* the window exists - the platform's start-up
draws from the same generator, which is what made the first version's summaries
differ from run to run:

```console
$ RILL_RAYLIB_DEMO=1 rillc exec examples/raylib/match3.rill
demo frames 213 score 152 moves 24
```

Both games report their score as the process exit code. The environment is read
through libc's `getenv`, declared in the script itself - raylib has no hint API:

```rill
extern "c" let getenv (name: Text) -> Text <io>;
```

`crates/rill-driver/tests/c_import.rs` builds and runs both demos this way, and
skips them when the header is not installed.
