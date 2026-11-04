# Integration onto v1-ernie

This is a running verification record for the port from archive snapshot
`69277817ff0408c44dacb723bb1edc20da86159d` onto upstream
`c3f41ef6027033553bd19202bbf457e5d9a3c9d1`.
The [plan](../superpowers/plans/2026-11-04-ernie-integration.md) and
[decision trail](2026-11-04-ernie-decisions.tsv) distinguish completed slices
from remaining work. Earlier reports retain their original checkpoint scope.

## Compiler-action validation

Integrated commit `7e899f8` preserves the upstream configured-native workflow
and working-directory-aware C validation. Ordinary compiler arguments cannot
replace the controlled compilation action, language mode or output path.

Fresh checks on the integration worktree:

```sh
cargo test --locked --test native_validation --test native_cli \
  --test native_configuration --test c_import_direct
```

All 19 tests passed across four targets. The independent slice worktree also
ran these tests and formatting before its commit. Inspection confirmed that
explicit saved configurations retain their separate argument grammar and
directory handling.

## Publication

The plan commit `cf42928` and compiler-action slice `7e899f8` were pushed as
normal fast-forwards to `v1-ernie`. The configured GPG signer timed out waiting
for an interactive prompt. These integration commits use a per-command signing
override; the user's Git configuration remains unchanged.
Portable capture `9a02114`, negation `c68dd13`, reference compaction `67c5fda`
and grammar `0e83182` were also pushed normally. Later slices await the final
integrated gate before publication.

## Portable SDL capture

The capture collector uses a presentation-call macro redirect, retaining a
separate call to the real SDL function. Both the shell utility and the upstream
matrix retain overwrite refusal. The matrix still inspects the exact typed C,
hashes compiler dependencies and compares six real O0/O2 framebuffers.

Fresh checks on macOS AArch64 with SDL3 3.4.18:

- All 18 Python matrix tests passed; shell syntax checking passed.
- A c-import-enabled integration binary passed six captures and five RGBA
  comparisons, including direct versus generated bindings and O0 versus O2.
- The shell utility captured both actual SDL game framebuffers successfully.

The first matrix attempt used a binary without `c-import` and rejected import.
The enabled run then reported a missing `AvailabilityMacros.h`. Supplying the
SDK returned by `xcrun --show-sdk-path` through repeated `--c-arg` options fixed
that configuration. Those exact flags now reach header extraction, C emission
and compilation, and are retained in the manifest. The successful capture
does not establish hardware-renderer or physical-input support.

## Open verification

The functional port is committed and both integrated full suites are running
on its frozen source. The final Helix workflow is still under investigation.
The stopped upstream baseline is described below. No delivery phase is marked
complete by these slices.

## Boolean syntax and SDL lifecycle

The negation/conditional slice is `c68dd13`. The integration rerun passed all
44 tests across `boolean_not`, `boolean_not_source`, `conditional_if`,
`ast_source_map`, `source_format` and `syntax_tree`. The slice owner's wider
focused run passed 59 tests. Source result annotations remain accepted.

The SDL lifecycle port retains upstream typed-emission checks and O0/O2 runs,
and adds Zig coverage, draw-error propagation and exact cleanup traces.
Fresh `sdl3_capture`, `sdl3_direct` and `sdl3_lifecycle` checks passed all five
tests with real installed SDL3 and Zig, plus deterministic failure fixtures.
The manual event constant now has its actual enum type, matching imported
bindings; the game converts it once at the call.

The initial upstream baseline run was stopped and restarted in a dedicated
unmodified checkout when integration examples started changing. Tests read
examples at runtime, so the earlier partial run cannot establish a baseline
for the source being ported.

## CPU voxel reference

The CPU port passed all ten `sdl3_voxel` tests with no failures. These include
interpreter/C O0/O2 simulation comparisons, real SDL dummy-driver execution
and matching captures. The headless CLI returned `204`. The historical stack
overflow observation remains in the blocker record, with its obsolete typed
foreign-transport prerequisite updated to reflect upstream implementation.
No larger storage or frame-time claim follows from this bounded example.

## GPU voxel demo

The GPU adapter, all seven Metal modules and the controller/cursor fixtures
are ported together because their pipeline names and shared layouts depend on
each other. Fresh required-device verification passed all four GPU tests with
no skips, including actual 256-cubed execution, partial-support leaning,
O0/O2 framebuffer agreement and native SDL cursor/focus behavior.

```sh
RILL_REQUIRE_GPU=1 RILL_TEST_GPU_256=1 \
  cargo test --locked --test sdl3_voxel_gpu -- --nocapture --test-threads=1
RILL_VOXEL_GPU_DEBUG=1 examples/sdl3-voxel/run.sh --self-test --world 128
```

The second command passed with Metal API validation enabled, O3/native CPU
tuning/ThinLTO and all 2,097,152 distance entries checked against independent
references. The retained geometry/dirty-work assertions also ran. The default
launcher now selects GPU; `--cpu` retains the bounded reference.
Historical performance distributions are preserved in their dated report;
they were not remeasured during this port. Scripted cursor/focus assertions
are distinct from physical input testing.

## Saved SDL profile source

The integrated saved-profile SDL test initially failed type checking after
supplying the required SDK. Its fixture added `u32` around an enum argument
that the updated game already converted. Removing that second conversion
preserves the actual game source. The fresh saved O0/O2 SDL regression passed;
the other 14 configuration tests had passed unchanged. All four native SDK
and resource-discovery deadline/cleanup probes then passed.

The integrated LSP/cache/overlay rerun passed 89 tests across six targets.
Three fresh launch-helper tests passed, including a post-main infinite loop
that still reports timeout rather than a language trap.

## Include-only SDL

The SDL binding module now contains only its include/link/pkg-config directives.
All five direct, Zig, real-capture and lifecycle tests passed again after that
change, using the default importer-enabled compiler and optimized ordinary
execution path. The explicit manual binding fixture remains for minimal builds.

## Frozen integrated gate

Functional source `f105d7d` is checked in a separate immutable checkout.
Formatting and strict all-target Clippy passed in default and minimal modes.
The full suites use distinct Cargo target directories and two test threads to
limit concurrent native compiler launches. On this macOS host, SDKROOT is set
to the active SDK. Required GPU and 256-cubed checks are enabled in both runs.
The full results remain pending.

The independent reviewer found a joined `-isysroot/path` override bug. The fix
recognizes encoded SDK/resource prefixes, preserves raw path bytes and input
identities, and leaves saved-profile grammar unchanged. Its regression went
red before the fix and green afterward. Ordinary automatic import now passes
the exact requested argument without invoking host SDK discovery.

The original isolated upstream run reproduced native launch timeouts and an
invalid-byte filename filesystem failure. It was stopped after preserving
those failures, rather than running duplicate C workloads beside the final
acceptance runs. It is not reported as a completed or passing baseline.
