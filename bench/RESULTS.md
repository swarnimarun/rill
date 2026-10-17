# Benchmark results

All programs read the same `/tmp/bench.json` (576,943 bytes, 4,000 nested
objects with escapes) and run the **identical algorithm**: one pass over the
bytes computing a rolling checksum plus JSON framing validation, repeated 200
times. Same input, same work, same output.

Build flags: `rillc build` → `cc -O2`; `rustc -O`; `zig build-exe -O ReleaseFast`.
Times are best-of-3 wall clock for the whole process (`/usr/bin/time -p`).

## v1 — byte-at-a-time scan (arrays/bytes, `for`, `if/else if`, casts)

| Program | run 1 | run 2 | run 3 | checksum |
| --- | --- | --- | --- | --- |
| **Rill** (`bench/json.v1.rill`) | 0.31s | **0.12s** | **0.12s** | 9171776200 |
| Rust (`bench/json.v1.rs`) | 0.32s | 0.13s | 0.13s | 9171776200 |
| Zig (`bench/json.v1.zig`) | 0.63s | 0.13s | 0.13s | 9171776200 |

Every program prints the same byte count and the same checksum, and the harness
asserts it: a first cut compared runs where the Rill range was written `0..reps`
(inclusive in Rill, per the §2.5 table) against Rust's exclusive `0..reps`, so
Rill did one extra pass and the totals differed by exactly one pass
(45,858,881). The programs now use `..<` in Rill to match, and text indexing
goes through `const unsigned char *` explicitly. Timing an unmatched workload is
meaningless, so the numbers above are from the corrected, identical runs.

Run 1 includes cold caches for all three; runs 2–3 are the steady state.
**Rill is at parity with Rust and Zig** on this workload: the C bootstrap backend
lowers to the same straight-line loop clang optimises, and there is no runtime
overhead in the measured path (the only runtime code used is `strlen` for
`size text` and `fopen`/`fread` for input).

Language features exercised by v1: functions, `var`/`set`, byte indexing of
text, `size`, `for` over a range, `else if` chains, boolean and integer locals,
casts (`as`), string concatenation and `io::println`.

Reproduce: `sh bench/run.sh` (regenerates the input, builds all three, times
three runs each).

## v2 — zero-copy JSON query engine (node pools, query, serialisation)

Same design in all three: parse the 55,299-byte document into a pool of
`(kind, span, first child, next sibling, child count)` records pointing into the
input, query the `rows` array for `score > 500` summing `id`, and serialise the
result into a 64-byte buffer and read it back. Pool reused across 200 reps.

| Program | best of 3 | checksum |
| --- | --- | --- |
| Rill (`bench/json_query.v2.rill`) | **0.03s** | 11724000 |
| Rust (`bench/json_query.v2.rs`) | **0.03s** | 11724000 |
| Zig (`bench/json_query.v2.zig`) | **0.02s** | 11724000 |

All three produce the same checksum, so the work is identical, and all three are
within noise of each other.

How it got here, because both gaps were instructive:

1. **Rill had no early-exit loop.** With only `for (k := 0..<n)` plus a done
   flag, every container loop spun the full document length, and the scanner was
   9.07s — ~18x slower than Zig. Adding `while (cond) { … }` and `break`
   (contextual keywords, so existing code is unaffected) and rewriting the member
   loops brought Rill to 0.03s, at parity with Rust and Zig.
2. **Rust was measured with a per-rep pool.** The Rust version allocated and
   zeroed a 786 KB node pool *per repetition*; hoisting it (as the Zig and Rill
   versions already did) accounts for its 0.51s → 0.03s. The remaining parity is
   what the algorithms actually cost on 55 KB of JSON.

The backend itself was never the bottleneck: v1's byte scan and v2's structured
parse both emit C that clang optimises to the same shape as the Rust/Zig code.

Two silent miscompilations were found and fixed while making v2 correct:
a Unit-returning call in a block tail, an `if` branch, or a function body's tail
was computed and then dropped (so scanner recursion vanished), and `.`-selected
indexed members (`p.nodes[i].kind`) were not lowered.

## Planned rounds

* **v2** — real JSON parsing: strings with escapes, numbers, whitespace
  skipping, nesting into a parsed value; exercises `match` on bytes, recursion,
  `inout` mutation and (Rust/Zig) dynamic arrays vs Rill's arena-backed `Array`.
* **v3** — build a typed tree/`Value` structure and query it (field lookup,
  array iteration, string comparisons); exercises `allocs`/arenas, records,
  enums with payloads and `dyn` in each language.
