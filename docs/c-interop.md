# C interoperability

Rill emits C11 and uses the selected target C compiler to check explicit native
declarations. This experimental subset does not complete the P5 gate in
[PLAN.md](../PLAN.md).

## Declare dependencies and signatures

Place dependency directives in the leading preamble, alongside imports:

```rill
@include("SDL3/SDL.h");
@link("SDL3");
@pkg_config("sdl3");

extern const SDL_INIT_VIDEO: u32;
extern fn SDL_Init(flags: u32) -> bool;
extern fn SDL_CreateWindow(title: cstr, w: i32, h: i32, flags: u64) -> ptr<SDL_Window>;
extern fn SDL_DestroyWindow(window: ptr<SDL_Window>) -> unit;
extern fn SDL_Quit() -> unit;

fn main() {
    if SDL_Init(SDL_INIT_VIDEO) {
        let window = SDL_CreateWindow(c"Rill", 640, 400, u64(0));
        let result = if is_null(window) { 1 } else {
            SDL_DestroyWindow(window);
            0
        };
        SDL_Quit();
        result
    } else { SDL_Quit(); 2 }
}
```

`@include` names a header, `@link` names a library, and `@pkg_config` resolves
compiler and linker flags from an installed package. They are distinct inputs.
Source annotations declare dependencies; repeated `@link` names and libraries
already supplied by pkg-config are not added twice. Use explicit CLI
`--link-arg` arguments when raw linker ordering or repetition matters.
Headers, packages and libraries must exist for the selected target. Rill passes
arguments directly to processes, without evaluating a shell. Header names and
dependency names are restricted to prevent directive or option injection.

Declare each selected function and constant explicitly. Rill does not yet parse
headers to import declarations automatically. Function compatibility checks
compare C types against the actual header prototype, including widths,
signedness, bool, pointer pointees and constness. Constants also require a
compatible declared type. Merely matching size does not establish ABI
compatibility. These checks currently require compiler extensions supported by
Clang and GCC-compatible compilers, including Zig's C compiler.

| Rill foreign type | C representation |
| --- | --- |
| `i32` | `int32_t` |
| `u8`, `u32`, `u64` | `uint8_t`, `uint32_t`, `uint64_t` |
| `f32` | `float` |
| `bool` | `_Bool` |
| `cstr` parameter | `const char *` |
| `ptr<CType>` | `CType *` |
| `const_ptr<CType>` | `const CType *` |
| `unit` result | `void` |

`cstr` results and `unit` parameters are not supported.

Opaque pointees such as `SDL_Window` retain their C identity. Pointee scalar
names use the matching C type. Raw pointers are copyable nullable handles;
they carry no Rill ownership or lifetime proof. Use `c_null()` in a concrete
pointer context and `is_null(pointer)` for a null test. This subset provides
no general pointer dereference, pointer arithmetic or C aggregate storage.
`u8(n)`, `u32(n)`, `u64(n)` and `f32(n)` explicitly convert an `i32` expression
for foreign calls; unsigned conversions reject negative or out-of-range inputs.
They do not imply a general numeric language or an implicit integer/bool cast.

A `cstr` argument is a direct NUL-free UTF-8 literal such as `c"Rill"`, with
program-lifetime storage. Supported escapes are `\n`, `\r`, `\t`, `\\` and `\"`.
Ordinary Rill strings permit embedded NUL and are a separate type.

## Reusable binding modules

Put dependencies and declarations in a local binding module, then import it:

```rill
// native.rill
@include("SDL3/SDL.h");
@pkg_config("sdl3");
export extern const SDL_INIT_VIDEO: u32;
export extern fn SDL_Init(flags: u32) -> bool;
extern fn SDL_Quit() -> unit;
export fn shutdown() { SDL_Quit(); }
```

```rill
// main.rill
import native;
fn main() {
    let ready = native.SDL_Init(native.SDL_INIT_VIDEO);
    native.shutdown();
    if ready { 0 } else { 1 }
}
```

Foreign declarations are private by default. `export extern fn` and
`export extern const` expose qualified calls and constants through the normal
module visibility rules. An exported Rill wrapper can call a private foreign
function, as `shutdown` does above. Imported modules cannot define `main`.

The loader aggregates dependency directives transitively, so importing a native
binding also selects native execution for `run`. Calls retain the original C
symbol name rather than the internal name used for ordinary Rill functions.
Repeated declarations of the same C symbol coalesce when their declaration
kind and exact type signature agree; parameter names do not affect agreement.
Conflicting signatures, pointer constness or function/constant kinds report
`E-MODULE-EXTERN`. Module aliases do not create separate C symbols.

The [direct SDL3 example](../examples/sdl3-direct/README.md) separates its game
from [a reusable SDL binding](../examples/sdl3-direct/sdl.rill). Binding modules
still require explicit declarations and manual foreign resource cleanup.

## Check, build and run

```sh
cargo run -- check examples/sdl3-direct/main.rill
cargo run -- emit-c examples/sdl3-direct/main.rill > /tmp/rill-sdl3.c
cargo run -- build examples/sdl3-direct/main.rill -o /tmp/rill-sdl3
cargo run -- run examples/sdl3-direct/main.rill
```

With source dependency directives, `check` and `emit-c` also invoke the selected
C compiler to validate the declarations against headers. They do not link a
finished executable. `build` compiles and links; `run` automatically builds a
temporary native executable and executes it. Pure programs use the interpreter
unless `run --native` or compiler/link arguments are supplied. The interpreter rejects reached foreign
calls. Manual externs without dependency directives need a native run.
Without source directives, `check` performs Rill checks only, even when
`--cc` or `--c-arg` is supplied.

Choose the compiler with `--cc executable` or `--zig-cc`; these options cannot
be combined. `--cc` takes precedence over `CC`, which otherwise defaults to
`cc`. Repeated `--c-arg argument` supplies include paths, target flags or other
compiler arguments. Repeated `--link-arg argument` supplies linker arguments
for `build` and native `run`. Use the same target configuration when checking
and building. Cross-target executables need an appropriate runtime to execute.

Legacy manual linking remains available:

```sh
cc -std=c11 -c examples/ffi/add.c -o /tmp/rill-add.o
cargo run -- build examples/ffi/add.rill -o /tmp/rill-ffi --link-arg /tmp/rill-add.o
/tmp/rill-ffi
```

Without included headers, manual signatures remain programmer assertions.

## Safety and evidence

Analysis is split between Rill inference and move checks, and C target signature
checks. Header compatibility cannot prove pointer validity, retention, resource
ownership, effects, blocking, thread rules or foreign function behavior. The
programmer must check nulls, obey the library contract and destroy foreign
resources on each relevant path. Foreign handles do not gain automatic Rill
cleanup. There is no enforceable unsafe/effect boundary yet, and foreign
declarations remain unsupported in restricted Rill owner programs.

The [direct SDL3 game](../examples/sdl3-direct/README.md) calls the real library
without a C shim. `tests/sdl3_direct.rs` builds a three-frame headless variant
and runs it against installed SDL3, checking resource creation, rendering,
events, mouse-state queries and cleanup. It reports a dependency skip when
SDL3 development files are unavailable. It does not simulate physical clicks.
`tests/native_modules.rs` checks private wrappers, exported native symbols,
visibility, compatible duplicate declarations and rejected conflicts, including
C execution at `-O0` and `-O2`. `tests/c_import_direct.rs` covers direct header declarations and negative
compatibility cases. See [remaining FFI work](blockers/ffi.md) for the open gate.
