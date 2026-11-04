# Automatic headers and native build verification

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](2026-11-04-ernie-integration.md).

`@include` now supplies referenced supported C declarations at build time.
Native `run` and `build` default to `-O3`. The general C fallback uses smaller
tagged payload unions. These changes retain the experimental subset and open
phase gates in [PLAN.md](../../PLAN.md).

## Header import and target agreement

The native driver loads the module graph before ordinary resolution, infers
unresolved native references with lexical scope and module-alias rules, and
asks the supervised C frontend to materialize their declarations. Header
modules export generated functions and constants automatically. Explicit
declarations retain their privacy and annotations; compatible foreign symbols
still coalesce only after individual validation.

Extraction sees the same ordered aggregate header preamble, package flags,
target/sysroot, definitions and optimization configuration as final C output.
This matters when an earlier header changes a later prototype through a macro.
The selected target compiler checks exact scalar/pointer types and constant
types/values. Unknown names remain unresolved; referenced unsupported forms
report their header location and reason. Unused unsupported declarations do
not prevent using an otherwise supported API.

Automatic import handles object macros that alias ordinary functions, including
source, command-line and chained aliases. It follows active preprocessing and
real function references, so obsolete macro definitions do not interfere.
Explicit resource-directory options take precedence over optional discovery.
Discovery probes have two-second and 64 KiB-per-stream bounds, cancellation
propagation and process-group cleanup. Automatic import retains the existing
30-second supervised worker boundary per request.

The default compiler feature includes the runtime-loaded importer. Minimal
`--no-default-features` builds retain pure Rill and explicit-extern use. The
primary [SDL3 example](../../examples/sdl3-direct/README.md) now contains no
handwritten extern declarations. Its header module has only include/library
directives. The game explicitly converts the imported `i32` event constant
when the SDL API consumes `u32`.

## Editor snapshots

Native import captures an opaque input state before target validation. It
contains positive header content and file identity, plus bounded search-root
state, including missing directories, `..` paths, resource directories and
sysroot-relative search paths. The worker transports that original state
through a bounded, versioned response. It rejects input changes during import.
Publication verifies the captured state again and writes the original records;
it cannot pair old declarations with newly fingerprinted headers.

Editor snapshots are published before running native user code. The ordinary
CLI/source-program loaders never reuse them for code generation. Only explicit
editor-library analysis and completion consume valid snapshots, without
pkg-config, libclang or a C compiler. Changing recorded files, header identity
or search-directory state invalidates the interface. Directory traversal has
depth and work limits, and shared/cyclic directories cannot cause exponential
revisits.

Independent reproductions covered header mutation during native execution and
after the worker's target check, optional-header creation, earlier-header
shadowing, parent-relative and sysroot-relative search paths, and byte-identical
hardlink replacement affecting `#pragma once`. Fresh compiler types changed
from `i32` to `u32` in the relevant cases, and the old editor interface was
rejected. These failures have regressions.

A real framed-stdio test builds an include-only module, removes `PATH`, sets
`CC` to a nonexistent executable, then checks native completion, hover, inlay
hints, signature help and invalidation after a header edit. Generated source
boundaries prevent definition navigation or hints beyond real buffer bytes.
New C references and changed target configuration require a fresh check/build
to update the last configured editor interface. Navigation into C headers is
not implemented.

## Native defaults and C output

`run` now compiles and executes native code by default. `--interpret` selects
the checked evaluator and rejects source native dependencies before launching
dependency tools. `--opt-level` accepts `0`, `1`, `2`, `3`, `s` and `z`;
explicit raw C optimization arguments follow that flag and preserve their
override behavior. A real compiler wrapper regression records the default
`-O3` and later `-O0` override, then executes the resulting program.

The fallback value representation shares mutually exclusive string, scalar and
pointer payloads in C11 unions. On the tested AArch64 host, nodes shrink from
64 to 32 bytes and 256-byte-buffer values from 4,216 to 2,112 bytes. Layout,
nested native payloads, negative-zero float bits, byte checksums and arithmetic/
bounds traps are tested at `-O0`, `-O2` and `-O3`. Mixed payloads also passed
AddressSanitizer and UndefinedBehaviorSanitizer at `-O3`.

The [native build note](../native-builds.md) records the actual-emitter benchmark
and its scope. Five timed runs gave a 2.03× median improvement at `-O3` for one
million byte-buffer updates/readbacks. A separate root-agent rerun gave 1.97×,
with checksum `124968056` in every execution. This is one workload on Apple
Clang 17, not a general speed guarantee. The compact typed backend and language
arithmetic/allocation/cleanup contracts retain their existing behavior.

Local benchmark artifacts are under `/tmp/rill-union-benchmark/`, including
`result-actual.json` and `result-root-confirmation.json`.

## Editor and syntax verification

The rebuilt release compiler checks the actual include-only SDL3 entry and
passes installed `hx --health rill`. A fresh real Helix smoke test verifies
SDL3 diagnostics and recovery, highlighting, seven rendered type hints,
automatic signature help advancing parameter 0 to 1, and actual popup emphasis
changes. A separate real workflow test verifies completion acceptance,
navigation, hover, symbol selection, formatting, lambda textobjects and
Tree-sitter indentation. Source and copied configuration bytes remain unchanged;
editor and server shut down cleanly. Existing user settings and compiler
symlinks were not replaced.

Helix uses one popup area for completion and signature help. A completion
response can replace a valid signature popup. The signature-render proof now
disables automatic completion only in that isolated session phase, requires
the actual automatic signature requests and visible parameter changes, and
asserts that no competing completion request occurs. The separate workflow
proof still accepts real completions. This test setting does not modify the
user's configuration file.

Artifacts include `/tmp/rill-native-helix-final/summary.json` and
`/tmp/rill-native-helix-workflows/summary.json`; two additional smoke runs are
under `/tmp/rill-native-helix-smoke-proof2/` and `...-proof3/`.
The real Neovim smoke test also passes. Tree-sitter verification passes 11
corpus cases and 52 repository sources, parser reproducibility, actual queries
and highlighting, incremental edits and rejection/recovery cases. No new
grammar production was required for automatic header import. Installer,
terminal-decoder and workflow-evidence helper tests pass 7, 5 and 2 cases.

The complete minimal-feature inventory passes 720 tests across 105 targets,
with zero failures; doc tests pass. Formatting and strict all-target Clippy
pass with default and minimal features. The release function example prints
`42` through both default native execution and explicit `--interpret`.

## Boundaries

Automatic import covers the existing exact scalar/raw-pointer ABI subset.
Aggregate values, callbacks, variadics and other unsupported C forms still need
diagnostics or an explicit C adapter. Header import proves no ownership,
foreign-resource cleanup or effect contract. Snapshots describe recorded
inputs for the last configured build; they do not establish a general C build
cache or complete preprocessing dependency service. See
[C interoperability](../c-interop.md) and [phase status](../phase-status.md).
