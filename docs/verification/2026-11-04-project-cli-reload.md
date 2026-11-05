# Project, CLI, reload and build-scaling checkpoint

This is a bounded tooling/compiler-performance increment. It does not complete
PLAN.md phase gates, provide Rust/Zig parity, expose runtime argv/general I/O,
or replace native functions with arbitrary live state.

## Changes and acceptance

- Strict local project manifests and no-clobber scaffolding reuse ordinary
  source-file compiler commands, including native build/run and interpretation.
- Native source watch builds candidates before replacing the program, preserves
  a running program on failed compilation, recovers from broken imports and
  supervises Unix cancellation/process groups. Project watch inherits the watcher
  PID on Unix so a parent can signal it directly.
- Watch retains reachable inputs, avoids import discovery on unchanged valid
  graphs, observes lexical/canonical source identities and ignores unused
  libraries. Explicit native/asset inputs remain caller-selected.
- `std.cli` classifies supplied string tokens and checks full-range signed decimal
  input with typed error offsets. `std.reload` exposes pure explicit generation
  contexts. Both have interpreter/C11 O0/O2 comparisons.
- `reload::Session` checks complete pure libraries and fixed closed integer hooks,
  atomically replaces accepted generations between calls, and retains the old
  program after failed validation or generation exhaustion. Host state crosses
  the boundary as integer arguments/results. Calls clone and recheck the program.
- Callable detection and conversion index declaration names/arity instead of
  repeated full-list scans. A 4,096-function regression retains full checking,
  including an invalid unused declaration. Duplicate arities retain detection
  and ordinary duplicate-name rejection.

Independent review found source symlink retarget invalidation, including the
regular-file-to-symlink variant. The retained lexical/canonical identity checks
now run before source reads. Regression coverage includes entry and imported
files. A second review identified Unix project-wrapper signal forwarding;
`exec` now gives the watcher the project process's PID.

## Local performance observation

Host: arm64 macOS, Rust/Cargo 1.99.1, Apple Clang 17.0.0.
Run `cargo run --locked --release --example function_lookup_bench` to reproduce.
The benchmark generates the AST first, runs one warmup, checks identical detector
results and takes the median of seven calls to each detector. The archived path
matches the old linear search on this restricted Call/Int fixture.

| Library functions | Archived linear detector | Indexed detector |
| ---: | ---: | ---: |
| 1,000 | 2.345 ms | 0.038 ms |
| 5,000 | 59.746 ms | 0.182 ms |
| 10,000 | 237.686 ms | 0.361 ms |

These are one local rerun under concurrent test load. An earlier same-process
run ranged from 4.415/0.077 ms at 1,000 functions to 647.819/0.371 ms at 10,000.
The timings show the removed quadratic search, without establishing an
end-to-end build speedup, runtime performance or a controlled variance estimate.
The retained benchmark, rather than a hard-coded timing threshold, supports
future measurements. [Build scaling](../build-performance.md) records the
remaining separate-compilation/interface/cache work.

## Native scalar loop observation

The retained `scripts/bench-native-loop.py` compares the current generated C
kernel with an equivalent C kernel and Rust kernel. All compute 5,000,000
iterations of `sum = (sum + i % 97) % 1000003`. An independently computed
checksum, `998162`, matched all 24 executions. For this workload the accumulator
and loop bounds keep every addition below i32::MAX; both divisors are positive.
The C baseline therefore has defined arithmetic matching these source inputs.

The script emits the actual checked Rill C, finds the one generated i32-to-i32
kernel and times it directly inside a temporary wrapper. It depends on the
current private emitter shape and fails explicitly if that shape changes; it
is not an exported native ABI. C/Rill use C11 `-O3`; Rust uses `opt-level=3`
with overflow checks enabled. Inner monotonic timers exclude process startup
and checksum printing. One warmup precedes seven samples per implementation,
with process order rotated between trials.

Final local rerun on the same arm64 macOS/Rust/Clang host:

| Implementation | Median | Observed range |
| --- | ---: | ---: |
| Rill | 23.246 ms | 23.197–23.339 ms |
| C | 23.227 ms | 23.159–24.661 ms |
| Rust | 23.242 ms | 23.199–23.286 ms |

An earlier run measured medians of 23.181/23.180/23.195 ms respectively. These
results overlap within observed variation for this scalar kernel. Concurrent
regression tests were running; this is not an isolated hardware study, a
whole-program result, a Zig comparison or a language-wide performance claim.
The kernel source performs no allocation operations; no allocation counter was
instrumented. Copies, closures, owners, FFI, parsing, graphics and large-library
build latency need their own workloads.

Reproduce after building the compiler:

```sh
python3 scripts/bench-native-loop.py --rill target/debug/rill
```

## Verification

Completed on the final source:

- `cargo fmt --all -- --check` and `git diff --check` pass.
- Strict all-target Clippy passes with default and `--no-default-features` configurations.
- `cargo check --locked --no-default-features --all-targets` passes.
- The final default affected run passes 45 tests across the binary, function lookup,
  projects, reload, std.cli, std.reload and watch suites.
- The minimal affected run passes 27 tests. A subsequent focused watcher rerun
  passes all four unit tests, including both source symlink regression variants.
  Those overlapping results are not added together.
- The existing function/reference/module/upstream suites pass 44 selected tests;
  the two new lookup tests also pass in the final default affected run.
- Runnable examples produce `generation 1: state 43` for the Rust reload host
  and `13` for the Rill generation-context example. CLI/project examples also
  execute in their integration tests.
- Relative documentation targets resolve, and the final file review finds no
  tracked build outputs, credentials or machine-specific source paths.

The full `cargo test --locked --no-fail-fast` command finished with exit 101:
its test harnesses reported **1,404 passed, one failed, zero ignored** across
139 suite summaries. The sole failing target was the configured SDL SDK case
detailed below; its environment-configured retest passes. This is not an all-pass
aggregate. The run began before the final watcher symlink regression and new
lookup integration target were added; the separate final affected run covers
those final files. Historical checkpoints and overlapping narrow reruns are not
added to the aggregate count. No Windows execution or arbitrary native reload evidence
is claimed. The optional 256-cubed GPU capacity case requires an opt-in and is
not enabled by this run.


## Configured SDL environment failure and retry

The initial full default run encountered the existing
`native::configured_sdl_game_runs_with_saved_o0_and_o2_profiles` failure. The
configured libclang frontend could not find `AvailabilityMacros.h` included by
SDL's platform header. `SDKROOT` was unset, while Apple Clang's resource
headers and a separately installed Homebrew libclang were in use. The SDK
header exists in the installed Command Line Tools SDK.

The narrow test reproduced the failure with the original environment. This
same test then passed, exercising its saved O0 and O2 profiles, with only this
environment change:

```sh
SDKROOT="$(xcrun --show-sdk-path)" cargo test --locked --test native_configuration native::configured_sdl_game_runs_with_saved_o0_and_o2_profiles -- --exact
```

No importer or native-configuration test source changed. The default feature
already enabled the same pinned libc before this checkpoint; its unconditional
Unix use adds signal support to the minimal-feature watcher. The earlier full
run is still recorded as a run with an SDK environment failure, rather than
being relabeled as an entirely passing aggregate.


## Local commit checkpoints

The implementation was initially verified without incremental commits. After
the user requested checkpoints, the work was split into these focused commits:

- `6702294` indexes callable declaration lookups with tests and a benchmark.
- `19fd243` adds project commands and source watching with lifecycle tests.
- `bd41d95` adds the CLI source library and checked reload hooks/contracts.
- `a36c30e` adds the native comparison script and build-scaling guide.

The user authorized unsigned local commits after the configured GPG signer
timed out. Signing was disabled per commit only; the global Git configuration
was not changed. No push was performed. With the configured JJ signing backend,
re-sign all new commits from this work after the final documentation checkpoint:

```sh
jj sign -r 'f3b1205..v1'
```

The range excludes the pre-existing checkpoint. The installed JJ 0.36.0 help
confirms `sign -r` accepts a revset and colocated repositories import underlying
Git updates automatically. This command is provided for the user; signing and
history rewriting were not performed by the agent.
