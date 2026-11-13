# Project direction and documentation review

Review baseline: `96d3c98`, the integrated local `v1` checkpoint, on 5 November
2026. The starting worktree was clean. `v1-ernie` is an ancestor of `v1`; the
review did not fetch, push, publish, merge or rewrite history.

This is a source and documentation review, not a comprehensive bug audit,
new benchmark run or full compiler acceptance. The changes alter documentation
and register the active roadmap in the embedded manual. Language semantics,
compiler limits and phase gates are unchanged.

Concurrent ABI/reload development appeared after the initial review, including
changes to Cargo inputs, typed library emission and new renderer/reload files.
Those changes were preserved and are outside this acceptance snapshot. Current
capability claims here refer to the committed baseline, not acceptance of that
in-progress work. A repository-wide format check encountered formatting diffs
in those concurrent files. Validation therefore uses an isolated export of
`96d3c98` with only this review's documentation and manual-registration changes.

## Findings and resulting direction

| Finding | Inspected evidence | Documentation decision |
| --- | --- | --- |
| Production still has typed and Copy fallback paths | `src/backend.rs::emit`, `src/interpreter.rs::run`; both guard fallback with `!checked.has_owned()` | Keep C11 delivery and differential coverage; do not describe full shared MIR as completed |
| Typed verification is explicitly provisional | `src/typed.rs::verify` documents that source ownership checking cannot be bypassed | Retain P1/P2 verification work before general resource claims |
| Ownership restrictions block broader library composition | `src/check/ownership.rs::verify` and `reject_recursive_calls`; `tests/owner_scopes.rs` | Prioritize bounded owned places/cleanup before general containers, affine closures or foreign resources |
| Native generations exist within a narrow interface | `src/abi.rs`, `src/native_reload.rs`; `tests/native_reload.rs` | Distinguish implemented exclusive i32 replacement from future owned-state/callback/GPU migration |
| Frontend reuse does not cache native recipe work | `src/main.rs`, `src/project/recipe.rs`; `tests/project_recipes.rs::frontend_cache_hits_still_compile_current_native_sources` | Correct build guide and separate frontend, C, shader and end-to-end timing |
| Historical performance data is not a regression baseline | `scripts/bench-build.py`, `scripts/bench-native-loop.py`, previous timing reports | PERF-1 retains raw samples/metadata before numeric targets; semantic edits, memory and application distributions remain unmeasured |
| Ordinary compiler PR coverage is absent | `.github/workflows/docs.yml` runs docs on selected PRs; `.github/workflows/release.yml` runs compiler checks on tags/manual dispatch | N1 proposes compiler PR checks and explicit capability accounting; no workflow is added by this review |
| Optional hardware tests can return success after a skip | `tests/engine_3d.rs`, `tests/sdl3_voxel_gpu.rs` | Keep required hardware evidence separate from default test totals |
| Planning docs mix old branch instructions and current capabilities | Previous handoff/continuation directed new work to `v1-ernie`; current source and later handoff already include it | Make one current queue and workspace handoff; preserve dated evidence rather than duplicate chronology |

The README now introduces current usage, phase status summarizes accepted
capabilities, PLAN retains design/gates, CONTINUATION_PLAN owns scheduling and
build-performance owns measurement and optimization criteria. The library
catalogue, feature matrix and language proposals link to those owners.
Documentation maintenance records how to keep these roles synchronized.

The active order starts with regression protection and baselines, then bounded
ownership/cleanup and useful consumers. Performance work proceeds against
measured costs in the C11 pipeline. Interfaces/effects and resource contracts
remain prerequisites for larger compilation/library/interop changes. A new
LLVM/MLIR backend requires measured justification. This is an engineering
priority decision, not evidence of any new feature or performance result.

## Validation scope

An isolated export of `96d3c98` received only the twelve changed documentation
and manual-registration files plus this report. The following commands ran
there successfully on the local macOS host with Rust/Cargo 1.99.1,
Python 3.14.6 and Node 26.8.1:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo test --locked --no-default-features --bin rill docgen` | 24 passed, zero failures; 17 unrelated binary tests filtered out |
| `cargo test --locked --no-default-features --test docs_cli` | 13 passed, zero failures |
| `cargo clippy --locked --no-default-features --bin rill -- -D warnings` | Passed |
| `cargo run --locked --release --no-default-features -- docs --language -o target/site` | Passed; active roadmap included in the offline manual |
| `python3 scripts/check-docs-links.py target/site` | 38 HTML pages, 3,796 links and 285 search addresses passed |
| `node scripts/test-docs-search.cjs` | Search behavior checks passed |

In the shared workspace, the changed-file `git diff --check` passed. A local
path/heading scan resolved all 191 relative Markdown references in the eleven
edited Markdown files. Independent source/documentation review found no
remaining actionable inconsistency after the baseline scope was made explicit.

The first shared-workspace manual build also completed, with four dead-code
warnings in concurrent Copy ABI/reload code. Its link/search checks passed,
but it is not the isolated acceptance run above. The shared-workspace
`cargo fmt --all -- --check` returned 1 for formatting differences in concurrent
reload/ABI files; those files were not reformatted by this task.

No full compiler suite, benchmark, SDK package, GPU run, remote CI job or
deployment was performed for this documentation review. No performance change
or phase completion is claimed.
