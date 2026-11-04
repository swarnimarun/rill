# Native voxel scale limits

The CPU reference of the SDL3 voxel example keeps its simulation in Rill and
its world at 8×8×8. This integration slice provides the CPU launcher.
This is an example core, not completion of the P6 or P10 gates.

## Evidence from the example

On AArch64 macOS during the 3 November 2026 implementation:

- Initial native C emission rejected the combined game loop with
  `E-C-STACK: estimated frame for 'M$1$play' is 3573504 bytes (1 MiB limit)`.
- Splitting functions got the example through per-function checks. The first
  unoptimized native smoke then exited with SIGSEGV. AddressSanitizer identified
  stack overflow along the lighting gather/ray/distance call chain.
- At that historical checkpoint, native foreign calls used the uniform C value representation.
  Every scalar temporary has capacity for the largest inline value. Two byte
  buffers in a 512-cell volume therefore increase scalar frame costs too.
  Per-function estimates do not establish a bound on cumulative call-stack use.

The example responds by keeping function work small and shortening ray-gather
call chains, with real SDL smoke comparisons at `-O0` and `-O2`. It does not raise
stack limits, disable checks or move simulation into C.

## Next steps before a larger engine

The upstream v1-ernie compiler now implements checked foreign scalar/pointer
lowering while retaining header validation and foreign lifetime limitations.
The integration checks compare native call behavior at both optimization levels. Add cumulative stack or
frame-cost evidence rather than treating the present per-function guard as a
whole-program guarantee.

Grow storage only with a supported collection/ownership contract. Current
foreign programs cannot use the restricted owned byte-buffer path, and named
fixed-buffer parameter types, floating-point Rill arithmetic, streaming chunks,
rigid-body dynamics and GPU compute are separate work. See
[c-interop.md](../c-interop.md), [owned allocation](owned-allocation.md), and the
[example's exact supported behavior](../../examples/sdl3-voxel/README.md).

