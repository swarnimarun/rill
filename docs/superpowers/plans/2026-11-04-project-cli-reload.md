# Project CLI and reload implementation plan

> **For agentic workers:** Implement the independent project, library and reload tasks in parallel, then review and verify their integration.

**Goal:** Supply runnable project workflows, an allocation-free CLI token library, source watching and checked interpreter function replacement.

**Architecture:** Project commands delegate to the existing compiler using structured arguments. Watching builds a fresh executable before replacing the child. Embedded function replacement installs a complete checked generation between synchronous calls, with host-owned integer state.

**Tech stack:** Rust, existing serde_json dependency, Rill source modules, C11.

**Spec:** User request; PLAN.md sections 14.5, 18 and 19; docs/phase-status.md.

## Global constraints

- Preserve the existing checked language, ownership and native ABI boundaries.
- Never evaluate a shell command assembled from project data.
- Reject incompatible reload hooks before publishing a generation.
- No Rust/Zig parity, stable ABI, benchmark speedup or phase-completion claim.

## Task 1: Project commands

Files: src/project.rs, tests/project_cli.rs, docs/projects.md, examples/project; integration in src/main.rs.

Interfaces:
```rust
pub fn create(args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String>;
pub fn run(args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String>;
```

- [x] Create strict version-1 rill.json loading and no-clobber `new` creation.
- [x] Delegate check/build/run in the project directory; preserve individual compiler arguments.
- [x] Run `cargo test --locked --test project_cli`, covering actual native output and malformed manifests.

## Task 2: CLI source library

Files: stdlib/cli.rill, src/modules/stdlib.rs, tests/stdlib_cli.rs, examples/cli, stdlib/README.md.

Interfaces: `classify(text, options)` returns typed token spans; `parse_i32(text, span)` returns std.result.Result with byte-offset errors. Input remains an explicitly supplied static string.

- [x] Implement options, inline values, terminator, positional arguments and bounded integer parsing.
- [x] Compare interpreter and C11 O0/O2 results with expected boundary/error values using `cargo test --locked --test stdlib_cli`.

## Task 3: Watch and restart

Files: src/watch.rs, tests/watch_cli.rs, src/main.rs, docs/hot-reload.md.

Interface:
```rust
pub fn run(args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String>;
```

- [x] Track bounded source/native-input content snapshots and extra explicit watch paths.
- [x] Build a new candidate before stopping the previous child; retain the running child on compile errors.
- [x] Expose `--once` for a single build/run, interval validation and project forwarding.
- [x] Run snapshot unit tests and CLI compile/run/recovery tests.

## Task 4: Embedded function replacement and source hooks

Files: src/reload.rs, tests/reload.rs, stdlib/reload.rill, src/lib.rs, src/modules/stdlib.rs, examples/reload, docs/hot-reload.md.

Interfaces:
```rust
pub struct Session { /* checked program, integer hook contracts, generation */ }
impl Session {
    pub fn new(program: rill::ast::Program, hooks: &[&str]) -> Result<Self, String>;
    pub fn replace(&mut self, program: rill::ast::Program) -> Result<i32, String>;
    pub fn generation(&self) -> i32;
    pub fn call_i32(&self, name: &str, args: &[i32]) -> Result<i32, String>;
}
```

- [x] Check complete source and each fixed i32 hook before publication; reject owners/native code/main/generic or borrowing interfaces.
- [x] Install candidates only between synchronous calls; reject mismatches without changing the current generation.
- [x] Provide source-level generation/context decisions with explicit host inputs.
- [x] Run `cargo test --locked --test reload` and source-hook interpreter/C comparisons.

## Integration and review

- [x] Review task interfaces: project delegates watch; main registers both; standard catalogue registers cli and reload independently; reload uses existing compiler APIs.
- [x] Update README, phase status and handoff with the bounded contracts and remaining native reload blocker.
- [x] Finish the full default `cargo test --locked --no-fail-fast` run: 1,404 passed and one SDK setup failure; that same test passes with SDKROOT configured. Formatting, both strict Clippy configurations, minimal all-target compilation and final affected default/minimal tests pass. Preserve the failed aggregate in the verification record.
- [x] Inspect git diff and untracked files before reporting exact outcomes and skips.


## Task 5: Large-library build work from user steering

Files: src/functions.rs, tests/function_lookup.rs,
examples/function_lookup_bench.rs, docs/build-performance.md.

- [x] Replace repeated full-list callable detection/conversion lookup with borrowed name/arity indexes, preserving invalid duplicate behavior and source emission order.
- [x] Retain the old restricted detector in a runnable release benchmark and compare 1,000/5,000/10,000-function ASTs after warmup with median-of-seven timings.
- [x] Check a 4,096-function library, including an invalid unused function; run existing interpreter/reference/C function suites.
- [x] Make normal watcher polls reuse known reachable inputs and check source path identity before reads; preserve recovery for broken imports and source symlink changes.
- [x] Document actual module boundaries, current admission limits and the required interface/toolchain/header identities for future separate compilation and caching.


## Task 6: Runtime evidence and commit checkpoints

- [x] Measure a fixed bounded i32 kernel against equivalent C and Rust with startup/printing excluded; verify every checksum independently and retain the reproducible script.
- [x] Record medians, observed variation, exact compiler flags and the narrow scope of both performance measurements.
- [x] Create focused unsigned local implementation commits after explicit user authorization; preserve global Git signing configuration and do not push.
- [x] Provide the installed JJ signing command for the new commit range, leaving signing to the user.
