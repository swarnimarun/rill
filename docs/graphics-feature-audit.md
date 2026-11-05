# Graphics feature audit

This audit separates the language work needed for a practical renderer from
facilities already available through C bindings. Rill is an experimental
language. A working renderer does not establish Rust or Zig feature parity,
general memory safety across a foreign API, or completion of PLAN phases.

The compiler emits C11. Rill can own application logic, finite scene values,
camera transforms and GPU command sequencing while a small native adapter owns
resource handles and translates Rill values to the graphics API's C layouts.
The existing [voxel GPU example](../examples/sdl3-voxel/README.md) already
uses SDL GPU compute with external Metal shaders. It is a real GPU program,
but its native adapter owns float camera data and shader/resource layouts.

## Existing and missing capabilities

| Renderer requirement | Available | Remaining boundary |
| --- | --- | --- |
| Scalar math | Checked `i32`, `f32(i32)`, explicit binary32 operations and predicates | Float literals, overloaded arithmetic/ordering, unsigned arithmetic, `f64`, wrapping arithmetic, transcendental library operations |
| Vectors and matrices | Finite Copy records, generic functions, field reads/writes and disjoint field loans; vector/matrix math can be written using explicit float operations | No built-in matrix type, float SIMD, general arrays or vector math library |
| Geometry and uploads | Bounded Copy bytes, owned byte buffers, binary helpers and scalar C calls | No growable mesh container, typed slice, pointer dereference/arithmetic or general C aggregate argument ABI; a native adapter must lay out uploads |
| GPU resources | Exact nullable raw C pointer transport; supported SDL C declarations can be checked against headers | No checked foreign resource lifetime, automatic GPU destruction or owning GPU handle abstraction |
| Shader execution | Existing SDL GPU voxel compute example loads external MSL files and dispatches a compute pipeline | Rill source is not a GPU shader language; the checker does not prove address spaces, barriers or GPU race safety |
| Shader toolchain | `rill shader check/build/inspect`, Slang to SPIR-V/MSL, GLSL/HLSL to SPIR-V, SPIR-V validation, reflection/dependency metadata and atomic artifact publication | Cross-backend compilation does not itself prove host/shader layout compatibility; reflection must be checked against pipeline declarations. Explicit `msl-source` bundles still need runtime validation |
| Build and assets | Version 2 project native/shader recipes, checked generated-C reuse, shader inputs in rebuild/restart watch and bundled tool/package discovery | Native sources compile on every candidate, including frontend cache hits. Native object and shader compilation caches are not implemented; graph/input budgets still apply |
| Hot reload | Project rebuild/restart and a restricted versioned native function reload path | Existing function reload excludes reached foreign calls and GPU resource state; safe shader/pipeline replacement needs resource compatibility checks and GPU synchronization |
| Errors and lifecycle | Exhaustive enums, `std.option`, `std.result`, recoverable supported allocation and call-scoped loans | GPU errors need explicit checks at the binding boundary; exceptions, automatic foreign cleanup, async tasks and an effect system are not implemented |
| Large libraries | Explicit exports, local module loading and reachable monomorphic instances | General dependency packages and a stable serialized library interface are not implemented; current graph/instance budgets still apply |

See [language features](language-features.md), [C interop](c-interop.md),
[native builds](native-builds.md) and [PLAN.md](../PLAN.md) for the precise
general boundaries. Do not bypass a rejected ABI form by inventing an integer
handle or a guessed signature. Keep unsupported aggregates inside a checked
native adapter and expose supported scalar signatures.

## Binary32 value operations

```rill
type Vec3 = { x: f32; y: f32; z: f32 }

fn dot(a: Vec3, b: Vec3) {
    f32_add(
        f32_add(f32_mul(a.x, b.x), f32_mul(a.y, b.y)),
        f32_mul(a.z, b.z)
    )
}

fn main() {
    let tenth = f32_div(f32(1), f32(10));
    f32_is_finite(tenth) && f32_lt(tenth, f32(1))
}
```

`f32` means IEEE binary32. `f32(i32)` rounds to binary32. Arithmetic operations
are `f32_add`, `f32_sub`, `f32_mul`, `f32_div` and unary `f32_neg`.
The ordering predicates are `f32_lt`, `f32_le`, `f32_gt` and `f32_ge`.
`f32_is_finite` rejects NaN and both infinities. Every operation accepts only
`f32` arguments, and operands evaluate once in source order. Ordinary numeric
operators retain their existing `i32` inference. Write
`f32_div(f32(1), f32(10))`, since `0.1` is not accepted source syntax yet.

Division by zero follows IEEE rules and does not use the integer divide trap.
For example, `f32_div(f32(1), f32(0))` is positive infinity, while
`f32_div(f32(0), f32(0))` is NaN. Ordered comparisons involving NaN are false.
Same-type `==` and `!=` use numeric IEEE equality: negative zero equals positive
zero, and NaN is unequal to itself. NaN payloads and signs are not preserved
as an observable contract. Float bit casts are not provided.

The AST evaluator, typed evaluator and both C backends implement these value
operations. A reached foreign function or constant still needs native execution,
including foreign functions taking or returning `f32`. Unsigned conversion,
pointer, owner/native-mixing and C aggregate restrictions are unchanged. The
newer control-result proof and source-origin map also retain their existing
exclusion of float/native representations; ordinary final-expression returns
are the supported form for float math functions.

Each emitted arithmetic result passes through a volatile C `float` before the
next operation. Multiplication followed by addition therefore rounds twice,
rather than contracting into a fused multiply-add. Conversion uses the same
binary32 storage boundary. This contract assumes the host's default
round-to-nearest floating-point environment. Generated programs that use the
operations or conversion reject `-ffast-math` and `-ffinite-math-only` through
their corresponding compiler macros. Arbitrary unsafe individual floating-point
flags or foreign code that changes the floating-point environment are outside
this guarantee. The helpers require IEEE binary32 and have no `libm` dependency.

Displayed values use nine significant digits with C `%.9g` fixed/scientific
selection. Thus one tenth prints `0.100000001`, negative zero prints `-0`,
infinities print `inf` or `-inf`, and all NaNs print `nan`. This preserves
native output precision rather than using Rust's shortest decimal spelling.

## Next language work worth evaluating

Float source syntax and an explicit numeric inference policy would make camera
and material code easier to read. Add those after deciding whether literals
default to `f32`, require suffixes, or use contextual inference. Reusing integer
inference silently would change existing programs.

Typed slices and growable containers would remove much of the mesh-upload
adapter. They depend on initialization, ownership, allocation failure and
cleanup rules. A checked foreign resource wrapper needs a destructor contract
and an explicit relationship between device lifetime and buffers/pipelines.
These changes are more useful than accepting arbitrary raw-pointer operations
without lifetime checks.

Shader reflection should produce a versioned, inspectable artifact describing
entry points, stages, resource slots and host-visible layouts. Pipeline reload
must reject incompatible replacements and wait until GPU users of old resources
finish. This is separate from replacing a CPU function in the existing native
reload protocol.

## Verification

`tests/f32_arithmetic.rs` checks arithmetic, decimal display, integer conversion
rounding, NaN/infinity/signed-zero behavior, source-order snapshots, finite Copy
products, generics and separate multiply/add rounding across both evaluators and
both C backends at `-O0` and `-O3`. It also checks unsafe compiler-flag rejection
and preserves rejection of implicit operators. The independent typed verifier's
mutation tests in `src/typed/floats_tests.rs` reject incorrect operation arity,
operand/result types and invalid nested operands. GPU acceptance remains a
separate hardware/toolchain test, not evidence supplied by these numeric tests.
