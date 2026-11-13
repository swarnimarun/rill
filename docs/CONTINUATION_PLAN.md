# Active development roadmap

Reviewed against committed source `96d3c98` on 5 November 2026. This is the single ordered work
queue for the current integrated checkout. [PLAN.md](../PLAN.md) defines the
language contract and section 19 acceptance gates; [phase status](phase-status.md)
records implemented capabilities. Completed increments belong in
[verification records](verification/), not in this queue as new work.

## Direction

Make ordinary native programs useful with explicit allocation and reliable
resource handling. Keep the Rust compiler and C11 output as the delivery path.
Use the existing renderer and a small CLI/library program to expose missing
contracts. A new backend, more bundled graphics libraries or more syntax does
not by itself resolve ownership, effects or interface limitations.

The immediate priorities are repeatable regression protection, reproducible
performance measurements and bounded P1/P2 work. Small library and diagnostic
improvements can proceed independently when they fit the existing subset.
Do not wait for every P1 goal before maintaining useful P5/P6 experiments, and
do not count those experiments as completion of their prerequisite phases.

## Planning and evidence rules

- `NEXT` means the next work to plan, not work already started or accepted.
  `DEPENDENT` means its listed contracts must exist first. `DEFERRED` means it
  is outside the active queue. None is a phase-completion label.
- Before implementing a row, write a bounded plan under `docs/superpowers/plans/`
  naming the source changes, accepted/rejected programs and verification commands.
  Identify an owner when work starts; do not assign fictitious owners or dates.
- Record the tested revision, toolchains, inputs, failures, skips and exact
  evidence under `docs/verification/`. Preserve earlier records. Update current
  contracts and phase status after acceptance, without copying test totals into
  every guide.
- [Performance planning](build-performance.md) owns PERF-1 through PERF-5,
  workload definitions, measurements and optimization gates. The rows below
  reference that plan rather than defining another benchmark policy.

## Ordered work queue

| Order | State | Work and dependencies | Acceptance needed |
| --- | --- | --- | --- |
| N1 | NEXT | Routine compiler regression CI and capability accounting | PRs run formatting, strict lint and default/minimal compiler tests; dependency and hardware jobs distinguish executed, skipped and compile-only checks |
| N2 | NEXT, parallel with N1 | PERF-1 baseline for compiler iteration and supported runtime kernels | Reproducible inputs, raw samples, correct outputs/events and recorded host/toolchain/cache state; no new speed claim without these |
| N3 | NEXT, one bounded change at a time | P1/P2 typed representation, owned places and cleanup | Interpreter/C agreement, independent malformed-IR rejection and exact move/cleanup traces for each new accepted form |
| N4 | NEXT within current contracts; otherwise DEPENDENT on N3 | Useful library and source ergonomics | A runnable CLI/library or renderer use case, explicit failure behavior and tests preserving existing inference and ownership rules |
| N5 | DEPENDENT on N2 measurements | PERF-2/3 iteration improvements | A measured bottleneck reduced with unchanged diagnostics/semantics, complete invalidation and failure/concurrency tests |
| N6 | DEPENDENT on the relevant N3 contracts | P3 effects/storage and P4 interfaces; then PERF-4 separate compilation | Allocator/drop/effect summaries, canonical interfaces and dependency keys checked before consumers reuse results |
| N7 | DEPENDENT on N3/N6 contracts | Broader P5 foreign resources and P6 native library/application acceptance | Target ABI round trips, foreign retention/cleanup contracts, source debugging and allocation reports |

PERF-5 runtime experiments accompany supported library/renderer changes. They
need PERF-1 baselines and the semantics of their workload, not completion of
all compile-time optimizations. A performance regression discovered during N3
must be recorded and assessed before accepting that increment.

## N1: protect the current implementation

The existing documentation workflow builds and checks documentation on selected
PRs. Compiler format/lint/full-test commands currently live in the tag/manual
release workflow. Add ordinary compiler PR coverage before treating historical
acceptance reports as continuing regression protection.

Likely files are `.github/workflows/`, the existing tests and AGENTS.md's
verification instructions. The baseline commands are:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
cargo test --locked --no-default-features --no-fail-fast
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

Default features already include `c-import`; `--features c-import` is not a
second distinct mode today. Older reports used different defaults. Give each
claimed target a recorded compiler/SDK setup. Require actual SDL/libclang/GPU
execution only in jobs configured to supply it. `RILL_REQUIRE_GPU=1` makes the
engine hardware test reject missing capabilities; other optional suites need
their own skip accounting. A green Rust test return is not hardware evidence.

## N3: close semantic gaps before generalizing owners

Both production execution paths prefer typed lowering and permit a checked
Copy fallback. Owner programs must always use the typed path. Start from
`src/check/ownership.rs`, `src/typed.rs`, `src/typed/`, `src/backend.rs` and
`src/interpreter.rs`, with `tests/public_entrypoints.rs`,
`tests/owner_scopes.rs`, `tests/owner_optimization.rs` and representation suites.
The current verifier is provisional; PLAN section 16's complete verified MIR
is still a goal.

First bound an owned-record increment using the existing initialization/drop
tree foundation. Specify construction failure, field moves, replacement,
branch joins and exactly-once cleanup before allowing a new source form.
Keep unrelated generic, recursive and foreign owner combinations rejected.
Follow with separately reviewed loan/control-flow and owner-generic contracts,
using actual container/library needs to choose the next restriction to lift.

Preserve the Copy reference path as a differential oracle while representation
gaps close. Keep unknown storage rejected rather than inserting values or
erasing observable effects. Track stack/resource budgets separately; raising
limits is not a substitute for bounded algorithms or stack-safe traversal.
See [ownership](blockers/ownership.md), [owned allocation](blockers/owned-allocation.md),
[layout](blockers/c-adt-layout.md) and [stack safety](blockers/stack-safety.md).

## N4: choose small useful consumers

The [language proposals](proposals/next-language-increments.md) rank source
convenience changes within this track. Closed local annotations and monomorphic
CLI parsing over caller-owned bytes are candidates that can be scoped without
a general container system. Their source roles, inference rules, invalid spans
and no-mutation-on-failure contracts need explicit acceptance.

Design loop exits and early return together around checked exit targets and
cleanup, then implement them separately. Existing nonreturning-result proofs
do not establish arbitrary jumps. Hosted argv follows an agreed byte/encoding,
capacity and interpreter-input contract. Do not treat raw C strings as owned
Rill input. Float literal/operator ergonomics likewise need an explicit numeric
policy before altering current i32 inference.

Use `stdlib/`, `examples/cli/`, `examples/rust-migration/` and
`examples/engine-3d/` to validate usefulness. Keep generic Copy helpers separate
from owner-compatible utilities until their restrictions actually change.

## N6 and N7: larger contracts

Closed effects, destructor summaries and allocator provenance precede general
vectors, owned strings, borrowed slices and resource-safe host APIs. Canonical
module interfaces must describe all information that consumers rely on before
per-module compilation can be sound. Use C09–C14 in PLAN section 20 to split
these contracts into reviewable work; the C identifiers are requirements, not
an unchecked list of features still wholly absent.

P5 work should expand scalar interop through target-tested aggregate adapters
and reviewed foreign resource contracts. Callbacks need retention and quiescence
rules. GPU retirement needs completion evidence beyond CPU scope exit. Keep
external shader tools and narrow C adapters while these contracts are missing.
A small reusable native library and renderer remain the P6 acceptance consumers.

Separate project process restart, interpreter hooks, scalar native generations,
Copy-wire native calls and versioned host-owned Copy-state migration. The newer
Copy APIs remain bounded and experimental; they do not migrate owners, foreign
resources or GPU resources. See [C interop](c-interop.md),
[native generations](native-hot-reload.md), the
[stateful reload renderer](../examples/reload-renderer/README.md) and the
[graphics audit](graphics-feature-audit.md).

## Deferred work and decisions

P7 advanced static APIs, P8 concurrency, P9 transactions/search and the broader
P10 optimization/shader-language program retain PLAN's dependencies. Add no
LLVM/MLIR backend until profiling and code/layout reports identify a C11 limit
worth the cost of another backend. Formal core work can proceed alongside
bounded semantics, but testing is not a soundness proof.

P11 stabilization needs an explicit compatibility policy, audited libraries and
reproducible supported-target evidence. Alpha archives and a working renderer
do not close that gate. Publishing, pushing and changing repository settings
remain separate authorized actions.

## Resuming work

Read [AGENTS.md](../AGENTS.md) for workspace and validation steps.
The earlier `v1-ernie` work is integrated into the newer local `v1` lineage;
do not switch to an older branch based on historical instructions. Preserve
unrelated changes. Record a real blocker with evidence and a next experiment
under `docs/blockers/` when it prevents a bounded task from proceeding.
