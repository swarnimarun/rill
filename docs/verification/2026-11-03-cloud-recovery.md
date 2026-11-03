# Cloud recovery verification, 3 November 2026

The original cloud workspace became unavailable during continued development.
The durable verified patch was reapplied, without conflicts, to the unchanged
`v1` base `4a1014e0c4b9b28434c1506cf32ce90d47f87ec2` in a distinct workspace.
Recovery commit `e0bc124c83fbcbe5747c240661f9faefb7cc0b8f` restores the source
content previously verified through `fd5b9cab6b225d159acb769daad96746b88c31b3`.
It does not restore the original intermediate commit history or the later
unbacked-up work. Later increments must be reconstructed, reviewed and tested
again before they are counted as delivered.

The original recovery target was not overwritten. The older, unrelated `main`
checkout was not used as the implementation base. Publication still targets
`v1-ernie`; no remote publication is established by this report.

## Fresh environment and checks

The replacement x86_64 Linux environment uses Rust/Cargo 1.99.1, GCC 14.2,
Clang/libclang 19.1.7 and actual SDL3 3.2.10. The Windows GNU Rust standard
library is installed for compile-only checks. Toolchain setup is local to the
workspace; Clang's relocated resource directory is provided explicitly to
importer tests.

These checks ran serially against the recovered source:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast -- --nocapture
cargo clippy --locked --features c-import --all-targets -- -D warnings
cargo check --locked --target x86_64-pc-windows-gnu --all-targets
cargo check --locked --features c-import --target x86_64-pc-windows-gnu --all-targets
```

All seven commands passed. The default suite passed **336 tests** and the
importer-enabled suite passed **370 tests**, with zero failures, ignored tests
or dependency skips. Real direct and generated-binding SDL3 integration ran.
The Windows checks are compilation evidence, not Windows execution, linking
of the Rill examples, or non-Unix importer process-isolation evidence.

## Reproduced native graphics

The committed `scripts/capture-sdl3.sh` ran both original games with GCC and
Clang at `-O2`, using the real SDL3 software renderer and dummy video driver.
Both programs exited normally with result `0`. Their frame-40 BMP files are
byte-identical across the two C compilers. PNG pixel comparisons against the
previously delivered captures report zero differing pixels.

The direct game is 640 by 400 pixels; the collector is 640 by 480. The BMP
SHA-256 values are:

- Direct: `e52aa01ff638c4708cbb08b93e26668c3dd2c4fdb3dedd3a813510b6f6e7f46e`
- Collector: `26d2459a84facd1fec2de50252f75e0ee3e2425f8b3a8a3c6a1855747037c93e`

The harness reads the actual rendered framebuffer and requests an ordinary
SDL quit event. It does not change game drawing or simulate physical input.
Xvfb cannot establish Unix listening sockets in this executor, so no X11,
physical desktop, GPU-rendering or input claim is made.

## Remaining scope

This re-establishes the earlier verified subset in the replacement environment.
It does not close P1–P11 or establish general memory safety, effects, stable
ABI, native debugging, AArch64 execution or Zig execution. New grammar,
source-origin, solver, pattern-budget, typed-loan and typed-record changes need
their own fresh evidence and a combined regression run. Durable source and
work-in-progress backups are kept distinct from verified deliverables.

## Combined reconstruction checkpoint `0cdd345`

The complete seven-command matrix above passed again on source commit
`0cdd345be82bcb1e2f983be2c6ab39258cf747b0`, combining freshly reviewed grammar
and parsed-AST origins, private type solver, bounded source coverage, strict
per-lookup solver cursor, root Copy loans and finite Copy records.

- **418 default tests** and **452 importer-enabled tests** passed.
- Zero failures, ignored tests or dependency skips were reported.
- Formatting and both strict all-target Clippy configurations passed.
- Both Windows GNU all-target compilation checks passed, still not Windows
  execution evidence.
- The source commit and clean worktree remained unchanged throughout the run.
- Real direct and generated-binding SDL3 integration ran. The final integrated
  GCC `-O2` frame-40 captures also reproduce the delivered PNG pixels exactly.

All source increments had fresh independent review. Integration resolved only
status-document conflicts while preserving the combined scope; no unreviewed
source repair was required. Source coverage review found and corrected an
uncharged alias-walk path before integration. The integrated private cursor
charges every public/private head, parent and binding lookup before execution,
without compression, allocation or expanded-tree export. Tests cover failure
without solver-state changes and the exact one-unit catch-all shortcut.

This checkpoint still has the inherited public-type materialization resource
limit documented in the solver report. Conservative local value restriction,
module-rewrite origins, true field-place loans and the public-tree budget are
separate ongoing increments, not claims made by this combined result. No
language phase or general safety theorem is declared complete.

## Combined reconstruction checkpoint `00b0b9d`

The same seven-command matrix passed on source commit
`00b0b9daf9404945967bb73441217b7c21cd9d32`, adding conservative immutable local
value restriction, source origins through module rewriting, and typed Copy
field-place loans to the preceding checkpoint.

- **466 default tests** and **500 importer-enabled tests** passed, with zero
  failures, ignored tests or dependency skips.
- Formatting, both strict all-target Clippy configurations and both Windows
  GNU all-target compile checks passed.
- The exact source commit and clean worktree stayed unchanged throughout.
- Actual direct and generated-binding SDL3 integrations ran. Fresh GCC `-O2`
  captures of both games again match the delivered framebuffer pixels exactly.

Each increment received independent review before integration. The module
review found incomplete validation of private source-symbol metadata; the fix
now checks complete coverage and symbol identities against the exact parsed
slots and recorded transformations. Field-loan review exercised prefix overlap,
composed forwarding, original storage, traps and exact owner failure traces.
The old AST oracle's copied-field writeback differs after a terminal trap:
checked typed/native execution retains writes already performed, while the old
oracle does not finish its deferred writeback. No source-level recovery or
cleanup is promised after a trap; normal-return differentials still agree.

Integration required status-document conflict resolution only. No source
repair or skipped dependency check was needed for this combined result.
Public type-materialization limits, typed vectors and local-polymorphic value
elaboration remain separate increments. The full language phases, stable ABI,
physical input, hardware rendering and general resource-safety claims remain
open.

## Bounded type-materialization checkpoint `90ab9d5`

All seven commands passed on source commit
`90ab9d580bb33159b08412ab449c77af3e7b6f7e`: **481 default tests** and
**515 importer-enabled tests**, zero failures, ignored tests or dependency
skips, both strict Clippy modes, formatting and both Windows GNU compile checks.
The exact source commit and clean worktree remained unchanged throughout.

The independently reviewed materializer preflights expanded type weight, depth,
name bytes and cumulative copies before producing public trees. The historical
160-byte five-layer generic-expansion input now returns `E-TYPE-COMPLEXITY`
under the same capped subprocess conditions where the old code exhausted
memory; the nearby four-layer form remains accepted. Independent review covered
16,000 exact DAG budget boundaries and fresh 30,000-program comparisons against
both the original and value-restriction integration bases. The C03 integration
preserves per-use record instantiation, borrowed identity and weak-variable
lowering. Pattern-cursor accounting remains unchanged.

This policy has a cost: independent single debug-check observations on accepted
probes were about 1.5–2 times the baseline duration. These are not controlled
benchmarks or an execution-time claim about generated programs. The limits bound
specified type materialization and copying, not all compiler allocations or
all public-input resource use. See the [policy and detailed scope](2026-11-03-type-materialization.md).

## Checked vector checkpoint `7f1e961`

All seven commands passed on source commit
`7f1e961c442a60bd347980219d93d0809da79cc2`: **492 default tests** and
**526 importer-enabled tests**, zero failures, ignored tests or dependency
skips, formatting, both strict Clippy modes and both Windows GNU compile checks.
The source and clean worktree remained unchanged throughout.

The eight existing `u8x16` operations now use the checked typed evaluator and C
path, including concrete nominal/generic values, root and field loans and the
narrow Copy-vector subset alongside owners. C values occupy sixteen bytes;
scalar semantics and explicit opt-in NEON behavior are preserved. Independent
review checked byte-plane oracles, original versus typed versus forced uniform
execution, 395 native comparisons, 76 malformed-IR mutations, eight unsupported
NEON-target checks and exact allocation/cleanup/trap observations. GCC and
Clang at `-O0`/`-O2` and GCC UBSan were exercised. This adds no AArch64 execution,
Clang UBSan, automatic vectorization or performance guarantee.

Integration required only a phase-status documentation conflict resolution;
reviewed production code was unchanged. Typed local-polymorphic elaboration and
owned-record initialization/partial cleanup remain separate ongoing work, not
features or completion claims of this checkpoint.

## Initialization/cleanup foundation checkpoint `fedb2d4`

All seven commands passed on source commit
`fedb2d437ef54bfc9443c5c7c1c564d06ccebbdd`: **506 default tests** and
**540 importer-enabled tests**, zero failures, ignored tests or dependency
skips, formatting, both strict Clippy modes and both Windows GNU compile checks.
The source commit and clean worktree remained unchanged throughout.

This replaces typed root-availability bookkeeping with explicit initialization
state and exact cleanup trees. The verifier derives the required cleanup
independently before either consumer executes it. Source and typed-layout guards
for owned records remain closed; this is a behavior-preserving foundation, not
an owned-record or partial-move feature announcement.

Independent review compared 496 source fixtures with their exact baseline
through 1,488 check/run/C-emission outcomes, with no differences. It exercised
101 reachable states, 46,662 authority/loan checks, malformed cleanup at all
three entrypoints and 114 exact native resource traces across GCC/Clang and
available GCC UBSan configurations. A separate author Clang run used a verified
`cc`-to-Clang shim because setting `CC` alone does not control every existing
test. No sanitizer, target or language-phase evidence is inferred beyond these
checks. The combined integration required no conflict resolution.

## Local-value and fixture checkpoint `c42b493`

All seven commands passed on source commit
`c42b49385e26bdcc0bed1d994c508a42ca63d601`: **536 default tests** and
**570 importer-enabled tests**, zero failures, ignored tests or dependency
skips, formatting, both strict all-target Clippy modes and both Windows GNU
all-target compile checks. The source and clean worktree remained unchanged.

Eligible immutable local schemes now use exact per-use reconstruction from
once-evaluated concrete captures. Aliases share the original captures; phantom
parameters receive no fabricated default type. Bounded expansion, original
storage, source ordering and independent IR checks remain in force. See the
[local-value report](2026-11-03-typed-local-values.md) for the implementation,
independent review and deliberately unsupported cases.

The preceding main aggregate failed one unchanged executable-worker fixture
before its intended assertions: the operating system returned `ETXTBSY` when
starting its freshly written script. A controlled inherited writable-descriptor
experiment demonstrated a possible mechanism, but did not establish the cause
of that original failure. The correction serializes executable fixture creation,
launch and cleanup under a test-only mutex shared by the affected test modules.
Production supervision, assertions and timeouts are unchanged; no retries,
sleeps or skipped tests were added. Independent review passed 250 focused tests,
including high-parallelism and serial runs with no leftover fixture directories.
An interrupted author feature run is not counted as a pass. The complete normal
threading aggregate above ran after integration and passed.

Actual direct and generated-binding SDL3 integrations ran in the full suites.
This adds no physical-input, GPU, AArch64, stable-language or whole-phase claim.
Owned-record products/projected moves and native foreign typed lowering remain
separate development candidates, not features of this verified checkpoint.

## Shared native declaration checkpoint `2b480528`

All seven commands passed on source
`2b48052857bf846d795fa7c0b08a7d8329ce99f2`: **541 default tests** and
**575 importer-enabled tests**, with no failures, ignored tests or dependency
skips. Formatting, both strict Clippy modes and both Windows GNU compile checks
passed; source and clean worktree stayed unchanged.

The independently reviewed F0 increment extracts the existing native header
declarations and ABI assertions without changing their output, and exposes a
checked uniform-C reference emitter that rejects owner programs. Differential
tests use that explicit oracle instead of injecting unsupported raw operations.
Production native lowering is unchanged. See the
[native-C oracle report](2026-11-03-native-c-oracle.md) for the exact evidence.

The complete aggregate used a guarded RAM-backed temporary Cargo target, two
build jobs, no Rust debug symbols and no incremental artifacts. Persistent
source/evidence were retained without deleting caches or other work. This
storage choice does not add an optimization, native ABI or full-phase claim.
