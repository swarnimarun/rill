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
| Rill (`bench/json_query.v2.rill`) | 9.07s | 11724000 |
| Rust (`bench/json_query.v2.rs`) | 0.51s | 11724000 |
| Zig (`bench/json_query.v2.zig`) | 0.02s | 11724000 |

All three produce the same checksum, so the work is identical. **Rill is ~18x
slower here, and the cause is a missing language feature, not the backend:**
Rill has no `while`/`break`, so the object/array member loops are written as
`for (k := 0..<n)` with a `done` flag — they still spin `n` times per container.
Every other part of the scanner is equivalent (recursive early-exit scans, same
node layout, same query code). Zig's tight `while` loops are why it is fastest;
Rust's per-node `Vec` bounds/copy costs sit between. The fix is to add
`while (cond) { … }` with `break` to Rill and rewrite those loops; the backend
itself emits the same C quality (v1's byte scan is at parity).

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
