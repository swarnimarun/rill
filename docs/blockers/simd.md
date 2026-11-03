# Explicit vectors and SIMD targets

Rill supports an initial explicit `u8x16` value and a target-specific C path.
This is not automatic loop vectorization or completion of P10. The interpreter
and default C output implement vector operations lane by lane. The C backend
can explicitly select AArch64 NEON for AND, OR, XOR and lane-wise equality.
The load, lane access, any/all reductions and value packing remain scalar.
The checked typed path uses a 16-byte vector struct. Its NEON helper loads and
stores those byte arrays; the uniform fallback additionally packs/unpacks its
nodes. Neither representation establishes a performance guarantee. All eight
existing operations now share independently checked operand/result signatures,
source-order evaluation and compact typed execution, including generic/nominal
values, root/field loans and restricted owner programs. No vector semantics
are added by this migration.

## Operations and checks

`u8x16_load(bytes, offset)` loads a checked 16-byte region from a fixed byte
value. `u8x16_lane(vector, index)` returns a lane as an `i32` from 0 to 255.
`u8x16_and`, `u8x16_or` and `u8x16_xor` are lane-wise bit operations.
`u8x16_eq` produces 0 or 255 per lane; `u8x16_any` and `u8x16_all` treat a
nonzero lane as true. Lane 0 is the first source byte regardless of endianness.
Vector values can be passed to functions and stored in supported ADTs. There
are no vector stores, borrowed-slice loads or vector C FFI in this subset.

A load requires a nonnegative offset, at least 16 bytes, and an offset no
greater than `length - 16`; a lane index must be 0 through 15. Invalid accesses
trap. Both backends evaluate arguments in source order. Vector operations do
not imply heap allocation, pointer alignment, wrapping integer arithmetic, or
fast-math. Floating point, arithmetic, shifts, casts and cross-lane arithmetic
reductions need their own semantics before being implemented.

## Selecting the C path

Default C emission uses portable C11 scalar operations. `--simd scalar` on `rill build` explicitly selects that default. `--simd neon` passes
`-DRILL_SIMD_NEON=1` to the selected C compiler. `emit-c` does not select a
vector target; defining `RILL_SIMD_NEON` while compiling its output requests the
same path. Defining the macro, even to zero, selects NEON. The generated source
rejects the request if the C compiler does not identify an AArch64 target with
NEON or lacks the needed intrinsic header. The build does not add target flags
or inspect the Rust build host.

This is explicit target selection, not runtime CPU or OS capability detection.
The caller must choose a compiler target whose execution environment supports
the selected instructions. No instruction is used as a runtime probe. A binary
intended for machines with optional capabilities needs a baseline-safe runtime
dispatch design before wider SIMD targets can be promised. The default scalar
path is not itself proof of SIMD, though an optimizing C compiler may choose to
vectorize it.

Clang documents vector extensions and target-specific runtime feature checks;
GCC documents x86 runtime feature detection. These are compiler extensions,
not portable C11 interfaces. Availability and accepted feature names must be
verified per toolchain. See the [Clang language extensions](https://clang.llvm.org/docs/LanguageExtensions.html)
and [GCC x86 built-ins](https://gcc.gnu.org/onlinedocs/gcc/x86-Built-in-Functions.html).

## Evidence collected

At an earlier uniform-backend checkpoint on a host reporting Darwin `arm64`,
Apple clang 17.0.0
(`clang-1700.6.4.2`) compiled and ran the dynamic-input fixture in
`cargo test --test u8x16_neon`. The test compares all 16 lanes for AND, OR,
XOR and equality with independently computed results in scalar and selected
NEON modes at `-O0` and `-O2`; it also checks invalid load and lane traps.
The `-O2 -DRILL_SIMD_NEON=1 -S` output for the XOR fixture contained
`eor.16b`. The test also removes the compiler's NEON macro to check that an
explicit unsupported-target request fails for typed and fallback C output.
Host hardware versus virtualization was not independently verified. This is
not evidence for other compilers, targets or a performance improvement, and is
not a fresh native-execution result for the new typed vector helper.

The current typed migration was checked on x86_64 Linux with scalar execution
and explicit unsupported-NEON-target rejection. AArch64 hardware/emulation was
not available for a fresh NEON run. See the [typed-vector verification](../verification/2026-11-03-typed-vectors.md).

## Remaining evidence and work

- Test every supported SIMD target with its documented C toolchain on real
  hardware or a clearly identified emulator. A successful cross-compile alone
  is not execution evidence. Verify optimized, non-constant-folded fixtures by
  inspecting target assembly; successful compilation or speed alone is not
  proof of vector instructions. Report unavailable hardware as not tested.
- Keep randomized scalar/interpreter/NEON comparisons for byte operations,
  including lane order, masks, boundaries, traps, nested values and calls, at
  multiple optimization levels. Preserve bounds and argument evaluation order.
- Add documented capability selection and baseline-safe runtime dispatch before
  supporting optional features across a range of CPUs and operating systems.
  Record target, compiler, flags and feature policy in build/cache identities.
- Measure packing/copying and dispatch overhead on identical workloads before
  claiming performance improvement. Larger packed values contribute to the
  existing C stack budget.
- Specify integer arithmetic variants, floating-point semantics, alignment,
  memory operations and target ABI before adding them. General vectorization
  additionally needs proven aliasing, bounds, arithmetic legality and feature
  availability. None of this completes P2 ownership or the P10 gate.
