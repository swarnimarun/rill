# Native builds and optimization

`rill run` compiles and runs a native executable by default. `rill build` emits
an executable through the same C11 backend and selected C compiler. Both use
`-O3`; checking imported headers uses the same target/preprocessing and
optimization configuration.

```sh
cargo run -- run examples/functions/main.rill
cargo run -- build examples/functions/main.rill -o /tmp/rill-functions
cargo run -- run examples/functions/main.rill --opt-level 0
cargo run -- run examples/functions/main.rill --interpret
```

`--opt-level` accepts `0`, `1`, `2`, `3`, `s` and `z`. The last two request the
C compiler's size-optimization levels and require compiler support. Use an
unoptimized level for debugging or comparison. Explicit `--c-arg` options follow
the selected optimization flag, so an existing `--c-arg -O2` keeps its meaning.
`--interpret` uses the checked evaluator and cannot combine with native
dependencies or explicit compiler/linker options. `--native` remains accepted.

`CC`, `--cc` and `--zig-cc` select the C compiler as before. Generated C from
`emit-c` still needs optimization flags when compiled outside Rill. Arithmetic
overflow, division/bounds traps, allocation and cleanup retain their existing
contracts at optimized levels. The default uses portable compiler optimization
without fast-math or an assumption that the execution CPU matches the build
host.

## Compact reference values

The compact typed backend remains the first path for supported programs. The explicit `backend::emit_reference` oracle uses a uniform tagged
representation for supported Copy programs, including supported native transport.
The normal native backend retains upstream checked typed lowering. Its string, scalar and pointer payloads now share a C11
union. Flattening and extracting nested values copy the payload selected by
the tag.

In the archived AArch64 reference-emitter measurements, an inline node shrinks from 64 to 32 bytes. A
256-byte-buffer value shrinks from 4,216 to 2,112 bytes. This reduces copying
without removing checks or changing the logical representation.

A historical reference-emitter benchmark performs one million updates/readbacks on a 256-byte buffer
with an independent expected checksum of `124968056`. Actual emitted C was
compiled using Apple Clang 17. Two warmups and five timed runs per case, in
alternating order, produced these medians:

| C optimization | Previous representation | Union representation | Speedup |
| --- | ---: | ---: | ---: |
| `-O2` | 1.2701 s | 0.6710 s | 1.89× |
| `-O3` | 0.7605 s | 0.3751 s | 2.03× |

These historical numbers describe the old uniform-emitter workload on one host
at snapshot `69277817`; they were not remeasured during this port. They are not a
general performance guarantee. Local source, binaries, timings and checksums
are under `/tmp/rill-union-benchmark/`; `result-actual.json` records the measured
actual-emitter comparison. Regression tests compare layout, nested native
payloads, independent byte checksums and traps at `-O0`, `-O2` and `-O3`.

## Explicit native configurations

Saved `--native-config` builds use a separate exact-replay path. Ordinary O3
defaults and automatic SDK/resource discovery do not modify their recorded
compiler arguments. Their current supported optimization profiles remain O0
and O2. See [configuration contracts](c-interop.md) before creating or replaying
a saved configuration.
