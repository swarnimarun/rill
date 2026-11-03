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

Declare each selected function and constant explicitly, or generate a binding
module with the optional selected-declaration importer below. Function compatibility checks
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
contain selected declarations and still require manual foreign resource cleanup.

## Import selected declarations

Build the optional importer and install libclang 12 or newer:

```sh
cargo build --locked --features c-import
cargo run --locked --features c-import -- import-c \
  --header api.h --function api_add --constant API_FLAG \
  --c-arg -I --c-arg /path/to/headers -o native.rill
```

The default compiler build needs no libclang. `clang-sys` 1.8.1 discovers the
shared library at runtime; set `LIBCLANG_PATH` when it is outside normal paths.
If Clang's builtin headers are not found, pass
`--clang-resource-dir /path/to/clang/resource-directory` (the result of
`clang -print-resource-dir`, not its `include` subdirectory). This option affects
only libclang's builtin headers; it does not select a target or sysroot.

Repeat `--header`, `--function`, `--constant`, `--pkg-config` and `--link` as
needed. Selection is explicit and limited to 256 unique symbols. `-o` publishes
an ordinary binding module atomically after all checks pass; omitted `-o`
writes the complete checked module to stdout. An existing file is not changed
on extraction, validation, or write failure. No synthetic `main` is published.
Parameter names are generated as `arg0`, `arg1`, etc. so unnamed or keyword C
parameters cannot break the Rill parser.

The importer uses structural libclang queries, real preprocessing and canonical
C types. It accepts only types represented by the existing foreign subset:
exact target `int32_t`, `uint8_t`, `uint32_t`, `uint64_t`, `float`, `_Bool`,
`void` results, static `const char *` arguments, and one-level pointers to
supported scalars, `void`, or named record typedefs. Record pointee identity and
constness are retained. Equal size does not make `unsigned long long`
compatible with `uint64_t` when the latter is `unsigned long`.

By-value aggregates, enum parameters/results, callbacks, variadics,
unprototyped functions, static functions, non-default calling conventions,
volatile/restrict or address-space-qualified types, nested pointers, array
parameters and struct-tag-only pointers are rejected. A small C shim remains
the explicit workaround. A rejected selection reports its name and source
location where available.

Constants are object macros or enum constants with a frontend-foldable
integer/bool expression. Function-like macros, runtime variables, pointer and
floating constants are not imported. Clang determines their type and value;
the selected target compiler must agree on both at generation time. This is
stronger than manually written integer constants, whose existing checks accept
any representable integer expression. An enum constant may therefore import
as `i32` even if an API consumes `u32`; use an explicit `u32(...)` conversion.

Package preprocessing flags and `--c-arg` values are passed in the same order
to libclang and the selected C compiler (`--cc`, `CC`, or `--zig-cc`). The
supported import flags are `-I`, `-D`, `-U` (joined or separate), `-isystem`,
`-iquote`, `-idirafter`, `-include`, `-imacros`, `-isysroot`, `--sysroot`,
`-target`, `--target`, `-arch`, `-m32`, `-m64`, `-march=`, `-mcpu=`, `-mabi=`,
`-mfpu=`, `-mfloat-abi=`, `-fshort-wchar`, `-fshort-enums`, `-fsigned-char`,
`-funsigned-char`, `-fms-extensions`, `-nostdinc`, `-pthread` and `-std=c11`.
Other flags are rejected rather than silently dropped. In particular `-E`,
output flags, response files and extra C inputs cannot bypass semantic checks.
Use structured arguments for paths with spaces, as in the example above.

Generated modules do not store arbitrary compiler flags or freeze the header
configuration. Use the **same target, sysroot, defines and include paths** for
subsequent checks/builds; regenerate after changing them. Generation checks
frontend/target scalar and pointer widths, exact function signatures and exact
constant types/values, then validates the module as an imported library through
Rill's normal resolver/checker. This is not a portable binding-package format.
Future checks retain the ordinary native-module contract, including
representability checks for integer constants.

### CLI worker isolation and limits

On Unix, `import-c` runs the complete generation operation in a separate
instance of the same Rill executable: package resolution, libclang parsing,
Rill module checking and final target-C checking. The parent publishes nothing
until it receives a complete, valid response and the worker exits successfully.
A crash, malformed/truncated/trailing response, timeout or output-limit error
leaves existing output untouched. There is no worker executable searched on
`PATH`, worker-path environment override, or unbounded in-process fallback.

The default wall deadline is **30,000 ms**. Use `--timeout-ms N` to select
**1 through 600,000 ms**; it covers package lookup and the target compiler too.
The parent uses nonblocking pipes and stops the worker's process group on a
timeout, error or normal worker exit, including remaining ordinary descendants.
It retains the unreaped leader until group cleanup so a reused process ID cannot
be mistaken for that group. SIGINT and SIGTERM request the same cleanup and an
`E-C-IMPORT` cancellation error. A parent-owned private temporary directory is
removed on these paths, including temporary target-check objects; worker core
dumps are disabled. Relative compiler/header/resource paths retain the caller's
working directory and raw Unix path bytes.

The private, versioned IPC protocol bounds requests to **1 MiB**, generated
modules to **1 MiB**, each request list to **1,024 items**, and captured worker
diagnostics to **64 KiB**. Generated declaration lines are limited to **64 KiB**.
Selected declarations remain limited to 256 symbols and 128 function parameters;
frontend diagnostics and cursor traversal retain their own limits. Continuous
output and children that never read input cannot block deadline checks. The
parent rejects output outside the shallow generated-extern subset before its
final Rill check. The timeout covers worker execution, not initial CLI argument
parsing, bounded parent protocol validation or final output publication. OS
scheduling and process termination are not a realtime timing guarantee.

This is **not a memory, CPU, filesystem or security sandbox**. Headers and
configured compilers remain trusted toolchain inputs; no total memory or disk
quota is enforced. A descendant deliberately escaping its process group is
outside this boundary. SIGKILL of the parent, host failure or filesystem errors
can prevent cleanup. Non-Unix `import-c` currently reports that isolation is
unsupported rather than silently running without the boundary; ordinary
compiler commands and default builds remain available.

The public `c_import::selected::generate` library API deliberately remains
in-process, with no hard deadline or crash isolation. Library callers must
provide their own process boundary if they need one. Neither API infers foreign
ownership, lifetimes, effects or resource cleanup, and P5 remains open.

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
