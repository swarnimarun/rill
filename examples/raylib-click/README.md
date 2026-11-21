# Click challenge

Click the red square as many times as possible in 30 seconds. The score and
remaining time appear above it. Close the window or press Escape to exit.

The Rill source contains the score, hit test, score update and timer rule. The
C shim stores the clock start time and adapts raylib's Boolean and by-value
`Color` APIs to the narrow
C FFI supported by Rill. The shim is trusted code; neither its ABI nor its
resource lifetime is verified by Rill.

Install raylib with `pkg-config`, a C11 compiler and Rust, then build from the
repository root:

```sh
cc -std=c11 -I"$(pkg-config --variable=includedir raylib)" -c examples/raylib-click/shim.c -o /tmp/rill-click-shim.o
cargo run -- build examples/raylib-click/main.rill -o /tmp/rill-click --link-arg /tmp/rill-click-shim.o --link-arg "-L$(pkg-config --variable=libdir raylib)" --link-arg -lraylib
/tmp/rill-click
```

The final command opens a window. The interpreter cannot run this example
because it calls foreign functions. The linked raylib library must remain
available when the executable runs; platform-specific static raylib builds may
need additional system libraries.
