# Incremental build measurements

Measured on arm64 macOS with Apple Clang 17 using the release compiler.
Binary SHA-256: `b2ebf19f620a2d0bf8ca5e4b713445c764c5c248bd99586319ac37eca2083030`.
This repeat includes the subsequent string-equality feature.

```sh
cargo build --locked --release --bin rill --target-dir target/performance
python3 scripts/bench-build.py --rill target/performance/release/rill \
  --samples 5 --unused 1000 5000 --reached 900 --incremental
```

Each timed command has one warmup and five samples. Medians include process
startup and warm filesystem caches. The script verifies cache hits explicitly,
compares exact generated C, and checks independently expected program output
outside timing at O0/O3. Synthetic source edits append a new comment to an
imported library or reached source, forcing a fresh exact-graph miss each time.
Fixtures/cache/output live in a canonicalized temporary directory. No repository
source is edited. The broader test suite was active during this run, so these
are local observations rather than isolated-machine performance guarantees.

| Workload | Normal emit C | Cache-hit emit C | Normal build O3 | Cache-hit build O3 |
| --- | ---: | ---: | ---: | ---: |
| Factorial | 3.264 ms | 2.627 ms | 50.246 ms | 49.483 ms |
| Standard-library utilities | 10.573 ms | 2.718 ms | 75.611 ms | 67.543 ms |
| Standard-library callbacks | 5.492 ms | 2.537 ms | 55.785 ms | 52.024 ms |
| 1,000 unused declarations | 15.944 ms | 3.105 ms | 62.439 ms | 49.685 ms |
| 5,000 unused declarations | 72.570 ms | 6.849 ms | 117.776 ms | 52.562 ms |
| 900 reached leaves, 963 emitted functions | 30.870 ms | 4.660 ms | 188.663 ms | 158.631 ms |

The 5,000-declaration workload reuses checked C about 10.6 times faster and builds
about 2.24 times faster on a hit. Its emitted C contains only two functions.
The reached-function case still spends most build time compiling/linking C.
The small factorial case gains less than one millisecond. Direct source caching
remains opt-in.

Changed-source emission medians, including checking and cache publication, were
26.763 ms for 1,000 unused declarations, 82.523 ms for 5,000, and 41.613 ms for
900 reached leaves. These misses can cost more than uncached emission. There is
no per-module checked-body reuse or object cache in this increment. `check`
continues to check every loaded declaration; native compilation/linking runs
on every build, including hits.

An earlier warm run before string equality observed 109.415 to 9.075 ms emission
and 182.862 to 81.301 ms builds for 5,000 unused declarations. Its compiler SHA
was `2f5b510100fb6add59bfd5beb6ac18bdeafad1139a690a1d17c121f1c4b53e0b`.
Absolute timings vary with host activity; both runs observe about 2.2 times
faster full builds on a hit. The difference between runs is not evidence that
string equality made compilation faster.

Raw timing JSON was retained locally outside version control during verification.
Re-run the script for current ranges and samples on another host. These results
do not measure cold filesystem/SDK caches, native-header-heavy libraries, GPU
compilation, compiler memory, native reload latency or runtime execution speed.
