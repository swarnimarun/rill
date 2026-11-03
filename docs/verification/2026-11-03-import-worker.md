# Verification of the selected-import worker boundary

This increment continues local checkpoint `1ee46f19e4cae5a5a085eefa8efb1b758db4ce5b`.
It isolates the existing selected-C-import CLI without expanding the supported
C ABI or claiming completion of P5. The [interop contract](../c-interop.md#cli-worker-isolation-and-limits)
and [design decision](../research/selected-c-import.md#cli-isolation-continuation-3-november-2026)
state the supported boundaries and limitations.

## Environment

- x86_64 Linux, Debian 13; Rust/Cargo 1.99.1.
- Clang/libclang 19.1.7; GCC 14.2.0 as final C validator.
- Actual SDL3 3.2.10 development files and pkg-config available.
- Official Rust `x86_64-pc-windows-gnu` standard-library target installed for
  compile-only cfg checking. Windows execution was not performed.
- No Zig or AArch64 execution evidence was obtained in this increment.

## Verified behavior

The CLI now uses a same-executable worker for package resolution, libclang,
Rill checking and final C checking. The parent uses bounded nonblocking pipes,
a cancellable wall deadline and a dedicated process group. It waits without
reaping the leader, kills the owned group, then reaps; inherited ignored
SIGCHLD cannot invalidate that ownership invariant. The parent validates the
private protocol and restricted generated-module shape before atomic output.

Ten supervisor/protocol tests cover:

- Missing workers, worker crashes and failed exits after complete output.
- A blocked input pipe, a hanging worker and closed output before worker exit.
- Infinite stdout/stderr floods and exact retained-output limits.
- Normal worker exit with descendants retaining pipes; remaining descendants
  are stopped rather than left running.
- Private-workspace cleanup, malformed-success output retention, protocol
  version, truncation, trailing bytes, oversized lengths/lists and non-UTF-8
  output. Unix path bytes round-trip without lossy conversion.
- Deep expressions/types and a 5,000-declaration dependency chain are rejected
  outside the generated subset before the parent's general checker can run.

The 23 selected-import integration tests include the previous 16 exact-ABI,
preprocessor, malformed-header, compiler-bypass, target-disagreement and atomic
output cases, plus package/compiler deadlines, SIGINT/SIGTERM cleanup, inherited
ignored SIGCHLD, relative/non-UTF-8 paths, timeout-option validation and parity
with the explicitly in-process library API. Generated fixture bindings still
make actual C calls at `-O0` and `-O2`.

Both direct and generated SDL3 bindings ran against the actual SDL3 library,
including native build/run under the dummy video driver. These were not
optional-dependency skips. No physical input or desktop behavior is inferred.

Independent review identified and verified fixes for inherited SIGCHLD
reaping, arbitrary parent-side Rill dependency checking, and non-Unix test cfg
coverage. Review found no remaining production-code defect in this increment.
An initial integration stopwatch included cold CLI startup under heavy parallel
scheduling and was flaky. The redundant whole-launch assertion was removed;
exact timeout errors, confirmed tool startup, cleanup and sentinel retention
remain tested, alongside the supervisor's 200 ms timeout / 3 s return checks.

## Commands and results

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo check --locked --all-targets --target x86_64-pc-windows-gnu --features c-import
git diff --check
```

- Importer-enabled full suite: **324 passed, zero failed, zero ignored**, with
  no SDL3 dependency skips.
- Default full suite: **290 passed, zero failed, zero ignored**, with no SDL3
  dependency skip. The default-build availability smoke confirms missing
  feature support does not load libclang or modify an existing destination.
- Formatting and strict all-target Clippy passed in both feature modes.
- Windows-target checking passed, including the explicit non-Unix rejection
  test's compilation. This is not evidence of Windows execution or a Windows
  worker implementation.
- Whitespace/diff inspection passed; no build artifacts, downloaded tools,
  local installation paths or credentials are included in the commit.

The default CLI worker deadline is 30 seconds, configurable from 1 through
600,000 ms. This is process isolation and bounded IPC, not a memory/CPU/disk or
security sandbox, and not a realtime OS guarantee. Deliberately escaped process
groups, parent SIGKILL and host failure are outside cleanup guarantees. The
library `generate` API remains in-process. Non-Unix CLI supervision, persistent
target/configuration identities, broader C ABI support and foreign safety
contracts remain open.
