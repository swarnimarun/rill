# Performance and build scaling

This is the performance work plan for the experimental C11 implementation,
reviewed against committed source `96d3c98` on 5 November 2026. It separates implemented reuse,
historical measurements and work that still needs acceptance. The
[continuation plan](CONTINUATION_PLAN.md) sets project priorities;
[PLAN.md section 17](../PLAN.md#17-optimization-program) defines
the longer-term performance contract. No new timings were collected for this
documentation review.

Measure four costs separately: building the Rust compiler, compiling a Rill
program, iterating through watch/reload, and executing the resulting program.
C11 remains the first target. A new backend is a decision to justify with
profiles and a bounded experiment after measuring the existing pipeline.

## Library boundaries

Split a library by domain and import the modules an executable actually needs:

```text
src/
  main.rill
  math/integer.rill
  assets/decoder.rill
  assets/tools.rill
```

```rill
import math.integer as integer;
fn main() { integer.twice(21) }
```

Put `export fn twice(value) { value + value }` in `src/math/integer.rill`.
The module root is the entry file's directory. A directory full of source files
is not implicitly compiled; unimported `assets` modules in this example do no
checking or code generation work. An imported module is checked completely,
including unused functions and declarations. Keep optional subsystems in
separate modules rather than importing every subsystem through one umbrella
module. Splitting imports must preserve module privacy and nominal identity.

## Implemented baseline

Callable detection now builds a name-to-arity index once and uses it for every
call. Previously each call searched every source function, creating quadratic
work for large function sets. Callable conversion also keeps a borrowed function
index instead of repeatedly searching declarations. Indexes are lookup-only;
source traversal, specialization order and generated output order remain intact.
Duplicate declarations and inconsistent arities retain rejection behavior.

Run the repeatable detector benchmark with:

```sh
cargo run --locked --release --example function_lookup_bench
```

It compares the archived full-scan detector and current detector on the same
synthetic AST, checks that both report no callable conversion, warms each path,
and prints median milliseconds. It measures one compiler operation, excluding
source parsing, checking, specialization, C compilation and linking. Do not
interpret its ratio as an end-to-end build or program execution speedup.
Host-specific observations appear in the checkpoint verification record.

Source watch retains its transitive input set. After successful discovery,
unchanged polls compare file
contents and lexical-to-canonical path identities without repeating import
lexing, parsing, resolution or checking. Unused library files do not trigger a
rebuild. A changed reachable input re-discovers imports and rebuilds normally;
retargeted source symlinks invalidate the set. During a broken import, watch
observes the local tree and retries discovery so adding or repairing a module
can recover. Explicit directory watch paths still incur their requested scan.
See [watch limits and native inputs](hot-reload.md).

The existing typed backend already specializes reachable functions and emits
compact records/enums. Unused pure functions need not occupy generated native
code. All imported source still passes checking, and unused native declarations,
header assertions and linker configuration retain their validation obligations.
This optimization does not prune those obligations.

For large existing native dependencies, use source `@pkg_config`/`@link`
attachments to link installed libraries. A project can also compile its own
unchanged C support code once and pass the object through the existing linker
interface:

```sh
mkdir -p build
cc -std=c11 -O3 -c native/helper.c -o build/helper.o
rill project build --link-arg build/helper.o
rill project watch --link-arg build/helper.o --watch build/helper.o
```

Run these commands from the project root and substitute the real C support
file. Rebuild that object when its source, headers, target or flags change.
Watching the explicit object detects a replacement, even though recursive
watch scans skip `build`. This manually supplied object has no managed dependency
graph; source native arguments retain their ordinary order and validation.

Version-2 [project recipes](projects.md#native-and-shader-recipes) also accept
`native.sources`, include directories, packages, libraries and a shader manifest.
The driver syntax-checks those C files for `project check` and compiles fresh
temporary objects for builds. Watch observes the declared sources, their parent
directories, include directories and shader inputs. This is declarative build
orchestration, with no persistent native-object or shader compilation cache.
It does not yet discover every external transitive C header as a precise watch
dependency. Shader builds retain their own validation/publication contract in
the [shader guide](shaders.md#rebuilds-dependencies-and-publication).

Runtime performance is a separate measurement. The retained
`scripts/bench-native-loop.py` compares one checked scalar loop with equivalent
C and Rust, excluding startup and printing. Local medians were about 23.2 ms
for all three over 5,000,000 iterations, with identical expected checksums.
See the [method, ranges and limitations](verification/2026-11-04-project-cli-reload.md#native-scalar-loop-observation).
This does not measure large-library build latency or whole-program performance.

## Historical compile and build latency

The 4 November [compile/build measurements](verification/2026-11-04-build-speed.md)
separate checking, C emission and native compilation. Reproduce them with:

```sh
cargo build --locked --release --bin rill
python3 scripts/bench-build.py --rill target/release/rill --samples 5
```

Add `--debug-rill target/debug/rill` only after building that binary separately
if a debug-versus-release compiler comparison is needed.

Warm local medians on arm64 macOS with the release compiler and Apple Clang 17
are below. Each phase has one warmup and five samples. Times include process
startup; program execution is checked separately, outside the timed interval.
These are full repeated builds with warm filesystem caches, not incremental
cache hits or cold-machine measurements.

| Workload | Check | Emit C, including checking | Full build O0 | Full build O3 |
| --- | ---: | ---: | ---: | ---: |
| Factorial | 4.2 ms | 3.8 ms | 74.6 ms | 77.4 ms |
| Standard-library utilities | 12.5 ms | 18.3 ms | 100.1 ms | 118.7 ms |
| 5,000 unused declarations, two emitted functions | 77.4 ms | 115.6 ms | 189.5 ms | 188.9 ms |
| 900 leaves, 963 emitted functions including grouping helpers | 31.4 ms | 50.6 ms | 214.7 ms | 293.5 ms |

Checking/emission and full builds are separately timed commands; medians should
not be added or subtracted as exact stage accounting. The synthetic functions
have small homogeneous i32 bodies, not representative large application code.
Native-header imports, GPU compilation, compiler memory and cross-target builds
are outside this measurement. The full record includes ranges, smaller sizes,
the debug-compiler comparison and C-only compile/link timings.

## Incremental checked-C reuse

Project build/run/watch enable incremental frontend reuse by default:

```sh
rill project build my-tool --cache-status
rill project watch my-tool --cache-status
rill project build my-tool --no-incremental
```

Direct source commands opt in. `--cache-dir` also enables reuse:

```sh
rill build main.rill -o build/tool --incremental --cache-status
rill emit-c main.rill --cache-dir build/rill-cache --cache-status
```

The default directory is `build/.rill-cache` under the entry directory for raw
source commands, or under the project root for project commands. Explicit raw
paths resolve from the working directory. Project overrides must stay inside
the project and use ordinary directories. Cache and executable paths cannot
contain each other. An implicit project default falls back to disabled reuse
when it conflicts with an existing path or an unusual executable output path;
an explicit conflicting cache choice rejects. `--cache-dir` and
`--no-incremental` cannot be combined.

A hit reuses C emitted after a previous successful whole-program check. The
loader still reads the entire reachable graph and validates its import and
filesystem identities. The exact key includes logical module names, path bytes,
source bytes, imports and an embedded compiler-build identity. That identity
covers compiler sources, bundled standard-library files, Cargo inputs, compiler
version, features and build settings. A changed imported declaration, including
an unused one, cannot reuse a previous checked graph. Changing any source byte
invalidates the entry, even when emitted C would be identical.

Only Rill source graphs without native dependency directives, extern functions
or native constants are stored. Saved native configurations and interpretation bypass
reuse. `check` always checks source and retains its ordinary backend boundary.
Native source graphs continue their complete header/dependency checks. Pure
misses also avoid a redundant header-use parse and graph clone.

A version-2 project can still hit this frontend cache when its Rill graph is
eligible and its recipe has separate C sources. The recipe compiles those sources
again on a hit. Shader preparation also runs before frontend lookup. A hit is
not a certificate for native recipes, their headers or shader inputs; see
`frontend_cache_hits_still_compile_current_native_sources` in
[`tests/project_recipes.rs`](../tests/project_recipes.rs).

Every `build` and native `run`, including hits, validates current C arguments,
invokes the current C compiler, and links current inputs. C flags, injected
headers, optimization/SIMD choices and linker arguments therefore take effect
without depending on a cached toolchain/SDK certificate. No executable or object
file is reused. Native C compilation can still dominate small builds.

`--cache-status` writes `rill cache: hit`, `miss`, `disabled`, or a bypass reason
to stderr. It does not change program/emit-C stdout. A native graph can report a
miss but never writes an eligible entry. A hit reports frontend reuse, not a
skipped native compilation.

Storage is currently Unix-only and best-effort. Missing, malformed, truncated,
incompatible or checksum-damaged entries fall back to compilation. Entries are
published atomically through private no-follow directory handles. Symlinked
cache ancestry, group/world-writable cache locations and unsupported hosts
disable storage without failing a valid build. Canonical project/source roots
work normally; an explicit `/tmp` alias on macOS may need its canonical path.
The local cache trusts the current user, not arbitrary downloaded build data.
An entry is bounded to a 64 MiB key and 32 MiB of C. Total storage has no eviction
policy yet; removing the cache directory forces regeneration.

The 5 November [incremental measurements](verification/2026-11-05-incremental-speed.md)
observe 5,000-unused-declaration emission at 72.6 ms normally and 6.8 ms on a
hit; full O3 builds take 117.8 ms and 52.6 ms respectively. Small factorial
builds gain little, while changed-source misses include cache publication
overhead. These are whole-graph reuse measurements, not separate compilation.
That run overlapped the test suite, and its raw JSON was retained only locally.
Use it to identify candidates for investigation, not as a regression threshold
or a comparison with the earlier host run.

## Current limits

The compiler currently flattens a checked import graph and emits one C
translation unit. Cache misses recheck imported modules; every native build
invokes the C compiler. Checked-C reuse adds neither object-file caching nor
independent package compilation. The module loader bounds an entry graph to 256
modules, 64 levels and 1 MiB per source file. Other checker/specialization budgets
can reject larger inputs explicitly. No support for an arbitrarily massive
library follows from the lookup improvement.
The typed backend currently caps a program at 1,024 reachable function
instances, including concrete specializations and generated helper functions.
Unused source declarations still require checking but do not consume that
reachable-instance budget. Exceeding it reports `E-TYPED-INSTANCES` rather than
silently switching to a representation with different limits.

Generic functions should continue using static specialization on hot paths.
Any future dictionary or dynamic dispatch option must make its call/storage
cost explicit. Compilation partitioning must not introduce hidden allocation,
relax traps or change cleanup to improve timings.

## Measurement protocol

Use a release Rill binary and save the raw output for each comparison:

```sh
cargo build --locked --release --bin rill
mkdir -p target/performance
python3 scripts/bench-build.py --rill target/release/rill \
  --samples 5 --unused 100 1000 5000 --reached 100 900 --incremental \
  > target/performance/build.json
python3 scripts/bench-native-loop.py --rill target/release/rill \
  > target/performance/native-loop.json
cargo run --locked --release --example function_lookup_bench \
  > target/performance/function-lookup.tsv
```

The build runner checks expected O0/O3 output, exact cached C and a reported
cache hit. Its changed-source case appends comments to temporary fixtures;
it measures invalidation overhead, not a changed public API or program body.
The native-loop runner checks independent expected checksums and rotates
language execution order. The lookup benchmark checks the detector result for
its restricted synthetic AST. None of these checks proves all compiler semantics.

Keep raw output out of source control. Attach it to a review or CI artifact and
record its location and checksum in a dated `docs/verification/` report. Record
the source revision and dirty state, compiler binary hash, host CPU/OS/memory,
Rust/C toolchains, target, flags, feature set, input sizes and expected outputs.
The scripts supply some of these fields; add the rest to the report. Run baseline
and candidate on the same idle host, alternate their order, and retain all samples.
Do not run the test suite or another benchmark during timed runs.

| Cost | Existing coverage | Measurements still needed |
| --- | --- | --- |
| Compiler tool build | Historical warm Cargo observations | Clean and no-op Cargo builds with explicit dependency/cache state |
| Rill compilation | Check, emit C, C-only compile/link, complete O0/O3 builds, exact-graph hits and comment-edit misses | Peak compiler memory, native imports, real application graphs, semantic leaf/interface/generic edits |
| Iteration | Watch and reload correctness tests | No-change poll cost, edit-to-ready latency, native generation compile/load/swap cost, shader rebuild time |
| Runtime | One i32 loop against C/Rust; retained voxel frame observations | Supported data/owner/closure workloads, allocations/bytes, code size, peak memory and representative application distributions |

Report warm full builds, frontend hits, misses and cold builds as distinct cases.
A fresh temporary cache is not a cold filesystem or SDK cache. Use medians and
ranges for the existing five-sample build runner; collect enough samples before
reporting tail percentiles. Record omitted metrics and unavailable environments.
Native-only programs need a C caller or independent model as their oracle because
the interpreter does not execute native dependencies.

## Prioritized performance work

These are planned increments, not accepted features. Each needs its own bounded
implementation plan, negative cases and before/after report. PERF-1 includes
runtime baselines immediately. PERF-2 and PERF-3 can proceed independently after
measurement identifies the relevant cost; runtime coverage in PERF-5 grows in
parallel as its language prerequisites become available.

| ID | Work and prerequisite | Acceptance evidence |
| --- | --- | --- |
| PERF-1 | Retain reproducible build and runtime baselines using the commands above. Extend `scripts/bench-build.py` with real graphs, semantic edits, memory/code-size observations and explicit metadata before setting thresholds. | Re-runnable baseline/candidate reports with raw artifacts, correct outputs and cache state; measured variance and a named host profile. Missing GPU/native metrics stay explicit. |
| PERF-2 | Profile source loading, checking, callable lowering, specialization, emission and watch polling. Optimize the dominant measured cost within the existing C11 pipeline. Source areas include `src/modules.rs`, `src/check/`, `src/functions.rs`, `src/typed/` and `src/watch.rs`. | A representative workload improves beyond observed noise; tiny builds and changed-source misses have reported tradeoffs; diagnostics, source identity, traversal order, traps and cleanup remain covered. Do not simply raise work/depth limits. |
| PERF-3 | Measure version-2 C recipes and shaders, then design reuse for the expensive repeated stage. Start with `src/project/recipe.rs`, `src/shader.rs` and `src/main.rs`. Reuse requires complete input identity, including transitive headers/includes, tool versions, target, flags and SDK configuration. | Source/header/flag/tool changes invalidate the right artifact; stale or corrupt entries rebuild; concurrent publication cannot expose partial outputs; failed candidates preserve the prior usable build. Report clean, no-op and edit latency separately. |
| PERF-4 | Implement checked interface identity and separate compilation after the relevant P1–P4 contracts are explicit. This is C14 work, not an extension of the current exact-source cache key. Begin at `src/modules.rs`, typed verification and backend boundaries. | Canonical interfaces survive irrelevant internal ID changes, preserve nominal/type/layout/cleanup identity and record every implemented convention/effect bound. Versioned decoding rejects invalid entries. Cross-module generic instances/closures, linker identities and a single ownership-aware runtime agree with whole-program output. |
| PERF-5 | Expand runtime and graphics workloads as supported types/resources permit. Use `scripts/bench-native-loop.py`, `examples/engine-3d`, `examples/sdl3-voxel` and owner regression cases. General containers, effects and scoped jobs require their own semantic increments first. | Equal work and inputs, independent checksums/state/cleanup oracles, O0/O3 agreement and explicit allocation observations. Report code size, memory and frame distributions; distinguish Rill execution from C adapters and external shaders. |

PERF-4 needs a dependency graph for checked modules and concrete generic
specializations, verified interface/body serialization, and a linkage contract
before splitting C output. Native artifact keys must cover actual linked inputs,
ordered linker arguments and sysroot/toolchain identity as well as source and
compiler identity. Include implementation-only, public-interface,
generic-helper and transitive C-header edits in its invalidation matrix. Unknown
effects must not become empty effects, and interface reuse must not bypass checks
on unused declarations. Per-module output is an experiment until it demonstrates
useful edit latency without changing results, traps or cleanup.

For PERF-5, retain the [voxel measurement method](verification/2026-11-04-voxel-performance.md)
and distinguish CPU submission from completed GPU work. Record resolution, world
size, camera/workload, shader/driver settings, warmup and completion policy.
Required hardware runs use `RILL_REQUIRE_GPU=1`; optional early-return passes
do not establish GPU performance. The [voxel scale blocker](blockers/voxel-native-scale.md)
also remains relevant to the CPU reference's stack use.

The acceptance floor is correct behavior and no unexplained regressions. Numeric
latency, memory and code-size budgets become reviewed host/workload-specific goals
after PERF-1 establishes variance. There is no current whole-language Rust/C speed
claim, fixed frame-rate guarantee or accepted allocation-effect proof. Preserve
C11 as the default while testing any LLVM/MLIR proposal against the same corpus;
a backend switch requires evidence that it addresses a measured limitation.
