# Compile and build latency observations

This measures program compilation on the standard-library checkpoint
`951c9f38`, using an optimized Rill compiler on arm64 macOS with Rust 1.99.1
and Apple Clang 17. It changes no compiler behavior. The measured compiler
binary has SHA256
`6a31b591a9da0a94e2844434552bbb3e7c7d44bc9fce14c0ca3d4664d50571e3`.

## Method

```sh
cargo build --locked --release --bin rill
python3 scripts/bench-build.py --debug-rill target/debug/rill --samples 5
```

The runner uses structured process arguments and temporary fixtures/output
paths reused across samples, with one warmup and five timed samples per command.
Times are wall-clock
milliseconds including subprocess startup. Filesystem caches are warm. No
incremental cache or persistent object reuse is involved. Program execution
occurs after timing, verifying independent expected stdout at both C and Rill
build O0/O3. Failed checking/building/execution fails the benchmark.

`check` loads/parses/checks the source graph. `emit-c` additionally performs
ordinary backend checking, typed lowering, verification and C emission. The
C-only phase compiles and links already-emitted C. Full `rill build` repeats
the actual CLI preparation/emission and invokes the C driver through stdin.
These are independent measurements, not additive profiler slices.

The factorial/stdlib cases use checked-in examples. Synthetic unused-library
cases declare one exported answer function plus 100/1,000/5,000 unused private
functions, and an importing entry. All declarations are checked. Reached-code
cases use 100/900 leaf functions with grouped sums to keep source nesting and
runtime call depth bounded. Inputs/results fit i32; expected sums are computed
independently in Python. These tiny homogeneous bodies do not model complex
generic application code, C header import or arbitrarily large projects.

## Release compiler medians

| Workload | Check | Emit C | C-only O0 | C-only O3 | Full build O0 | Full build O3 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Factorial | 4.17 | 3.80 | 71.22 | 74.17 | 74.58 | 77.44 |
| Stdlib utilities | 12.52 | 18.30 | 79.26 | 103.38 | 100.14 | 118.70 |
| Stdlib Copy callbacks | 6.17 | 8.69 | 72.24 | 78.96 | 81.58 | 86.73 |
| 100 unused declarations | 5.56 | 6.58 | 70.47 | 73.20 | 76.34 | 80.23 |
| 1,000 unused declarations | 19.51 | 27.37 | 69.58 | 72.67 | 96.47 | 100.25 |
| 5,000 unused declarations | 77.40 | 115.63 | 69.40 | 73.82 | 189.47 | 188.95 |
| 100 leaves, 109 emitted functions | 6.81 | 9.69 | 81.59 | 93.41 | 90.15 | 101.99 |
| 900 leaves, 963 emitted functions | 31.35 | 50.56 | 165.32 | 240.76 | 214.73 | 293.49 |

The observed full-build O3 ranges over five samples were:

| Workload | Minimum | Maximum |
| --- | ---: | ---: |
| Factorial | 76.14 | 79.16 |
| Stdlib utilities | 116.68 | 120.77 |
| Stdlib callbacks | 84.61 | 87.41 |
| 100 unused | 78.82 | 81.50 |
| 1,000 unused | 98.94 | 102.59 |
| 5,000 unused | 185.94 | 191.63 |
| 100 leaves | 100.65 | 104.08 |
| 900 leaves | 290.70 | 294.54 |

All three unused-library cases emit exactly two functions and byte-identical
4,107-byte C, SHA256
`3bd48721ea211bde122d435d70e26a435a013461a89e06cd4ea9ff319cfd9b16`.
The extra checking/emission time therefore does not reflect extra native code.
The 963-function case emits 441,115 bytes of C; actual C compilation/linking
accounts for most of its full build time in this experiment.

## Debug compiler and compiler-tool builds

The unoptimized Rust compiler binary takes 55.25/95.16 ms to check/emit the
stdlib utilities example, versus 12.52/18.30 ms in release. For the 5,000-unused
case, debug takes 494.95/799.62 ms versus 77.40/115.63 ms. Benchmark program
compilation with a release Rill compiler; `cargo run` defaults to a debug binary.

Building the optimized compiler itself took 16.898 seconds with existing
target/dependency caches. Repeating unchanged `cargo build --release` took
76.5 ms externally, with Cargo reporting 0.03 seconds internally. A clean
checkout/toolchain/dependency build was not measured. These Rust/Cargo tool
build timings are separate from Rill program compilation.

## Limits and GPU meaning

The first synthetic reached fixture used 1,000 leaves, which also generated
group helpers and exceeded the current **1,024 reachable typed-instance cap**.
It was rejected with `E-TYPED-INSTANCES`, not timed as a successful build. The
default reached benchmark now uses 900 leaves/963 instances. Large unused
declaration counts must not be described as support for equally large emitted
programs. Module/source/checker budgets also remain applicable.

Every Rill rebuild currently rechecks its imported graph and compiles one C
translation unit. Persistent semantic caches, separate object compilation and
public-interface invalidation remain open. No Rust/Zig whole-project build-speed
comparison, cold-cache result, memory benchmark or new runtime-speed claim is
established here.

The four optional GPU branches in the standard-library regression record are
unrelated to these pure compilation timings. Three require real display/GPU
preflight and returned status 77 because the video driver added no displays:
readback/render agreement, building-lean simulation, and native window/input.
The 256-cubed world capacity case was not enabled. Their early returns count as
passes in Rust's runner, not ignored tests; they do not establish GPU execution.
On a display/GPU-capable macOS host with SDL3/Metal, require actual execution:

```sh
SDKROOT="$(xcrun --show-sdk-path)" RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 cargo test --locked --test sdl3_voxel_gpu -- --nocapture
```

`RILL_REQUIRE_GPU=1` makes unavailable hardware a failure. The capacity flag is
presence-based, so even `RILL_TEST_GPU_256=0` enables that case. SDKROOT supplies
C headers; it does not create display/GPU access. CPU/C and dummy-renderer tests
are separate and were executed, as recorded in
[the standard-library verification](2026-11-04-stdlib-foundations.md).
