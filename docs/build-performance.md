# Build scaling and library boundaries

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

## Changes in this checkpoint

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
watch scans skip `build`. The project driver does not manage this C dependency
graph; source native arguments retain their ordinary order and validation.

Runtime performance is a separate measurement. The retained
`scripts/bench-native-loop.py` compares one checked scalar loop with equivalent
C and Rust, excluding startup and printing. Local medians were about 23.2 ms
for all three over 5,000,000 iterations, with identical expected checksums.
See the [method, ranges and limitations](verification/2026-11-04-project-cli-reload.md#native-scalar-loop-observation).
This does not measure large-library build latency or whole-program performance.

## Current limits and next build work

The compiler currently flattens a checked import graph and emits one C
translation unit. Every build rechecks imported modules and invokes the C
compiler. This checkpoint adds neither object-file caching nor independent
package compilation. The module loader currently bounds an entry graph to 256
modules, 64 levels and 1 MiB per source file. Other checker/specialization budgets
can reject larger inputs explicitly. No support for an arbitrarily massive
library follows from the lookup improvement.

The next separate-compilation work needs these contracts, in this order:

1. Canonical checked interfaces recording nominal identities, type schemes,
   conventions, effects/resource bounds when implemented, and layout/drop
   requirements. An implementation-only edit must not silently change an
   imported contract.
2. An explicit dependency graph and input identity for each checked module and
   concrete generic specialization. Cache source content, compiler version,
   target, options, transitive native headers/configuration, sysroot/toolchain
   and actual linked inputs. A timestamp or source-name hash is insufficient.
3. Verified serialization/deserialization of those interfaces and checked
   bodies. Reject stale, truncated and incompatible entries; publish cache
   entries atomically and handle concurrent writers without partial reads.
4. Per-module or grouped C/object output with stable internal linkage contracts
   and one ownership-aware entry/runtime. Account for cross-module generic
   instances and closures before moving them across an object boundary.
5. Regression and benchmark workloads for clean builds, no-op builds, leaf edits,
   public-interface edits, generic helper edits and transitive C-header changes.
   Verify identical results/traps/cleanup before reporting latency or code size.

Generic functions should continue using static specialization on hot paths.
Any future dictionary or dynamic dispatch option must make its call/storage
cost explicit. Compilation partitioning must not introduce hidden allocation,
relax traps or change cleanup to improve timings.
