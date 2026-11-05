# Rill Raylib example

This project draws two cubes and a plane in 3D, then a rectangle and text in 2D.
Rill updates the bouncing cube and controls drawing order. The example stops
after 180 frames or when Escape/the window close control is pressed.

With a Rill SDK distribution that includes Raylib:

```sh
rill project check examples/raylib
rill project build examples/raylib
examples/raylib/build/raylib-scene
```

`rill project run examples/raylib --native` and
`rill project watch examples/raylib` use the same bundled dependencies.
The native C compiler is still required. The interpreter does not execute
Raylib calls. A source checkout needs the SDK installed or `RILL_TOOLCHAIN_ROOT`
set to its `libexec/rill` directory. A missing package/header/library reports a
native dependency error; drawing is never replaced with a fabricated result.

The [Raylib 6.2 header](https://github.com/raysan5/raylib/blob/6.2/src/raylib.h)
passes `Color`, `Vector2`, `Vector3` and `Camera3D` by value. Current Rill does not
support those aggregate ABI forms, so `std.raylib` calls a small C bridge through
checked scalar declarations. It provides window lifecycle, frame scopes, RGBA
colors, rectangles, static C-string text, camera setup, cubes, planes and image
capture. It does not expose every Raylib function.

All calls must run on the application's main thread. The bridge has one window
and one camera. Call `InitWindow` once, then `CloseWindow` once after balancing
each `BeginDrawing`/`EndDrawing` and `BeginMode3D`/`EndMode3D`. Do not mix direct
Raylib lifecycle calls with this wrapper. Rill does not prove this foreign
resource lifetime or automatically close the window.

Operations return `bool`. `WindowShouldClose` returns `-1` for an invalid call,
`0` to keep running, and `1` when closure is requested. `Status` reports the last
error since initialization: `0` none, `1` invalid state/argument, `2` native
window initialization, `3` image export. Some platform failures terminate inside
Raylib before a return is possible. This example therefore requires a usable
display and OpenGL context; it has no headless/software substitute.

`SetCamera` takes position, target and up-vector components, projection size
and a perspective flag. Perspective FOV must be between 0 and 180 degrees;
orthographic size must be positive. The target must differ from the position,
and up must not be zero or parallel to the view direction. Raylib's 3D coordinate
system differs from the SDL GPU engine example; use each example's camera math
with its own renderer.

`RillRaylib_Capture(c"scene.png")` exports the current frame when called after
`EndMode3D` and before `EndDrawing`. The bridge flushes pending drawing and frees
the temporary Raylib image. Capture paths are explicit; this example does not
write files during normal runs. A successful Rill main prints `0`; this printed
value is separate from the native process exit status.

To verify the C boundary without a GPU, run the SDK CTest target `raylib_sdk`.
The optional native test `raylib_sdk_tests --window /tmp/rill-raylib.png` creates
a real window, checks lifecycle/argument failures, captures the 2D/3D frame and
checks actual green rectangle and orange cube pixels. It fails if window or
GPU access is unavailable.

Set `RILL_RAYLIB_HIDDEN=1` for a hidden window during bounded hardware tests.
It still creates the real OpenGL context and requires GPU/display access.
