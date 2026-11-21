# Integration onto v1-ernie

The implementation port and its full acceptance checks are complete. This
record covers archive snapshot
`69277817ff0408c44dacb723bb1edc20da86159d` onto upstream
`c3f41ef6027033553bd19202bbf457e5d9a3c9d1`.
The [plan](../superpowers/plans/2026-11-04-ernie-integration.md) and
[decision trail](2026-11-04-ernie-decisions.tsv) record the retained and adapted
slices. Earlier reports retain their original checkpoint scope.

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
and grammar `0e83182` were also pushed normally. The subsequent reviewed slices
were published in four further normal fast-forward batches ending at `af302f5`,
`647157b`, `f105d7d` and `d2e3554`. The full integrated gate was still running
when those development checkpoints were published.

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

## Acceptance scope

Both complete feature modes passed with real GPU requirements enabled. Final
counts and commands are below. These results cover this implementation port;
no language delivery phase or stable release is declared complete.

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

Functional source `f105d7d` was frozen in a separate checkout; final default
fixture source is `b758366`. Compiler/library source is identical.
Formatting and strict all-target Clippy passed in default and minimal modes.
The full suites use distinct Cargo target directories and two test threads to
limit concurrent native compiler launches. On this macOS host, SDKROOT is set
to the active SDK. Required GPU and 256-cubed checks are enabled in both runs.
Both complete results are recorded below.

The independent reviewer found a joined `-isysroot/path` override bug. The fix
recognizes encoded SDK/resource prefixes, preserves raw path bytes and input
identities, and leaves saved-profile grammar unchanged. Its regression went
red before the fix and green afterward. Ordinary automatic import now passes
the exact requested argument without invoking host SDK discovery.

The original isolated upstream run reproduced native launch timeouts and an
invalid-byte filename filesystem failure. It was stopped after preserving
those failures, rather than running duplicate C workloads beside the final
acceptance runs. It is not reported as a completed or passing baseline.

## Final editor workflow

Fresh compiler source `d2e3554` passed the include-only SDL check, real Helix
smoke and full workflow, and real Neovim with LSP. The final workflow inspected
28 decoded screens and checked four definition destinations, three accepted
completions, hover, symbols, unsaved formatting, function/lambda textobjects
and indentation. Inlay hints and x-to-y signature emphasis were inspected in
the smoke. Disk sources and copied configuration stayed unchanged; both ends
completed clean shutdown. Grammar regeneration/query checks passed twelve
corpus cases and all 56 integrated repository sources.

Three preliminary workflow failures are retained. The scope failure reproduced
with built-in Rust under Helix's basic-color fallback, which omits the operator
style. The isolated verification session now explicitly selects true color and
a complete bundled theme. The first scope-probe predicate could also match a
directory name; no query change was made from that false positive. The hardened
gate requires the decoded popup at its cursor-relative cells, with a regression
rejecting status/source text and partial names. All fifteen Helix helper tests
passed after this change. Startup and edit deadlines remain unchanged.

Preflight inspection found that installed Helix paths were correct, but the
then-existing release compiler and parser rejected source result annotations. They were refreshed after the checkout advanced to the published port.
The release build, original-path SDL interface check, Helix health check and
installed parser acceptance of the real result-annotation example all passed.
Editor/language configuration and four queries stayed byte-identical; dated
backups retain the prior parser and installation files. Existing editor sessions need restarting to load the new
compiler and parser.

## Filesystem capability limits

An explicit nocapture run confirmed four invalid-byte filename setup checks
return their documented runtime skip on this filesystem with EPERM: formatting
source paths, module source paths, selected-import include paths, and saved
configuration include/compiler paths. Synthetic raw-byte wire/input tests still
execute. These are capability skips, not evidence of those filesystem paths
working here. Required SDL/GPU and final editor checks have no skips.

## Native display access for the full gate

The first complete-suite attempts were started inside the execution sandbox.
Required GPU checks failed with status 77 and native video initialization
reported no displays, with denied macOS workspace service connections. These
are failed runs, not accepted GPU skips. The earlier standalone required-device
run outside the sandbox passed all four tests. The superseded sandbox runs
were stopped, and both full modes were restarted with native display access,
the same frozen source/targets, required GPU checks and 256-cubed execution.
No test, production deadline or device assertion was weakened.

## Synchronization and temporary heads

A fresh remote fetch found no incoming cloud commits ahead of the published
head. The fixture correction `b758366` was pushed normally, and both canonical
local/remote refs matched it. The original Jujutsu checkout was advanced without
dropping its archived source snapshots; its rebuilt release compiler prints
`42` for the function example in both native and interpreted modes. The exact
original snapshot is protected by local `archive/source-snapshot-69277817`.
Jujutsu's original working-change archive also retains the editor-log snapshot.

At the user's request, the five temporary `port/*` bookmarks were removed and
25 duplicate side-only commits were abandoned. A separate review found 24
identical patch IDs and one native-default patch whose only adapted difference
retains the integrated `rill lsp` usage entry. No descendants outside that set
were present, including through JJ keep refs. Canonical ancestry, archives and
all other refs were excluded. The three clean completed agent worktrees were
removed; active verification checkouts remain until their jobs finish.

## Filename fixture variants and minimal acceptance

Outside the sandbox, APFS returns EILSEQ92 for unsupported raw-byte directory
names, rather than the sandbox EPERM1. The two new importer fixture guards
initially matched only ErrorKind values that covered EPERM. A repository-wide
search found exactly those two guards; the formatter and module fixtures
already handled raw EPERM1/EILSEQ92 precisely. The importer guards now use
the same macOS-only policy, retaining failures for other errors/platforms.
This is a test portability correction, not a compiler behavior change.
The two affected complete targets passed 15 and 22 tests in the corrected
default run. A focused nocapture command exposes capability reasons:

```sh
cargo test --locked --test format_cli --test module_origins \
  --test native_configuration --test selected_c_import utf8 -- --nocapture
```

The unrestricted minimal-feature full run completed successfully on `f105d7d`:
1,301 reported passing tests, zero failures/ignored/filtered tests, and zero
Rust warnings. Required real GPU and 256-cubed checks passed all four tests.
The two filesystem filename cases report capability skips; they are included
in Cargo's passing-test count. The later guard changes are inside the disabled
importer-only test modules, and compiler/library source is unchanged. Both
strict Clippy modes and formatting passed again on final fixture source
`b758366`. The corrected complete default run also finished successfully, as below.

## Complete integrated acceptance

Both commands returned exit status zero. Runs used the active macOS SDK,
distinct Cargo target directories, `RILL_REQUIRE_GPU=1` and
`RILL_TEST_GPU_256=1`. All required SDL/GPU checks executed successfully.

| Gate | Result |
| --- | --- |
| `cargo test --locked --no-fail-fast -- --test-threads=2` on `b758366` | 1,378 reported passing results; zero failures, ignored or filtered tests |
| `cargo test --locked --no-default-features --no-fail-fast -- --test-threads=2` on `f105d7d` | 1,301 reported passing results; zero failures, ignored or filtered tests |
| `cargo fmt --all -- --check` on `b758366` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` on `b758366` | Passed |
| `cargo clippy --locked --no-default-features --all-targets -- -D warnings` on `b758366` | Passed |

Both full modes report zero Rust warnings and include their zero-test doctest
target. Passing-result counts include runtime capability returns: four distinct
macOS raw-filename checks in default mode and two in minimal mode. These
filesystem behaviors remain unverified here. No required GPU, SDL or final
editor check was skipped. The two later importer guard changes are disabled
in minimal mode; all active compiler/library code is identical across the
recorded source checkpoints.

Final full logs and machine-readable summaries are retained locally as
`/tmp/rill-ernie-full-default-final.log`,
`/tmp/rill-ernie-full-default-result.json`,
`/tmp/rill-ernie-full-minimal-host.log` and
`/tmp/rill-ernie-full-minimal-result.json`. The final editor command/artifact
manifest is `/tmp/rill-ernie-editor-verification-final.json`. Failed preliminary
logs remain separate; their results are not counted as successful acceptance.

The accepted implementation still has the documented experimental boundaries,
including Copy-only callables, unsupported mixed callable/logical-layout
conversion cases, bounded optional typed-origin coverage and unchecked foreign
resource lifetimes. Historical performance figures were not remeasured.
Existing Helix sessions must restart to load the refreshed compiler/parser.

## Independent final review

`gpt-6-astra` independently checked the 134 result summaries in each full log,
zero-failure exits, required GPU results, source equivalence across checkpoints,
final Clippy/format checks, editor artifacts and temporary-head cleanup. No
unresolved findings remained. The reviewer used supplied task messages, Git
and retained artifacts; the original conversation transcript was unavailable.
