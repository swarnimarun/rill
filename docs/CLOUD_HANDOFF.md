# Continue the v1 checkpoint

`v1` is an experimental implementation checkpoint, not a stable language
release. The original checkpoint was paused, then resumed at the user's
request on 3 November 2026. The cloud continuation targets `v1-ernie`, leaving
`v1` and the unrelated `main` lineage unchanged. See the
[continuation plan](CONTINUATION_PLAN.md) and
[cloud verification](verification/2026-11-03-selected-c-import.md) for the new
selected-C-import increment and remaining work. The subsequent
[CLI worker increment](verification/2026-11-03-import-worker.md) adds Unix
timeout/cancellation, bounded IPC and process-group cleanup while keeping
the library API in-process.

The subsequent [typed-pattern increment](verification/2026-11-03-typed-copy-patterns.md)
brings concrete Copy enums and primitive/nested patterns onto the shared checked
typed interpreter/C path, including Copy matches inside restricted owner
programs. The [generic-instance increment](verification/2026-11-03-typed-specialization.md)
adds reachable concrete function specialization to that same path. The
[root-loan increment](verification/2026-11-03-typed-copy-loans.md) adds shared
and exclusive Copy root loans and computed read temporaries. The
[Copy-record increment](verification/2026-11-03-typed-copy-records.md) adds
compact finite record products, construction, field reads/assignment and
whole-record root loans. The [field-loan increment](verification/2026-11-03-typed-field-loans.md)
adds independently checked projected operands, prefix-overlap intervals and
original-slot forwarding. The [Copy-vector increment](verification/2026-11-03-typed-vectors.md)
migrates the eight existing `u8x16` operations, retaining scalar/opt-in NEON
behavior and adding no vector semantics. The initialization/drop-tree foundation
is present; accepted owner-record lowering still needs further work.
The [local-value increment](verification/2026-11-03-typed-local-values.md) adds
bounded capture-and-shape elaboration for eligible immutable schemes. Concrete
leaves are evaluated once into private typed capture records; aliases share
captures and each whole/field use has an exact concrete type. Unused phantom
shapes need no default type. This does not broaden generalization or ownership.
The [checked-native increment](verification/2026-11-03-typed-native-integration.md)
adds explicit typed scalar native calls/constants and static C-string arguments,
with complete declaration checks and source-order captures. The subsequent [raw transport integration](verification/2026-11-03-typed-native-raw-integration.md)
adds existing unsigned/float/raw-pointer transport to that checked path. P1 and
P5 remain open.

## Open the checkpoint

```sh
git clone --branch v1-ernie git@github.com:swarnimarun/rill.git
cd rill
cargo build --locked
cargo run -- run examples/blocks/main.rill
```

The local project uses Jujutsu. The `v1` bookmark is published as a Git branch,
so a cloud workspace needs only Git. This checkpoint has a separate lineage
from the repository's existing `main`. Continue current work from `v1-ernie`;
original `v1` remains unchanged. Do not treat it as an incremental patch on
`main` or rewrite the other branch.

Use a recent stable Rust toolchain and a C11 compiler available as `cc`. This
checkpoint was exercised with Rust/Cargo 1.99.1, Apple Clang 17, Zig 0.17.0 and
SDL3 3.4.18 on AArch64 macOS. Other hosts need their own verification. The original checkpoint had no third-party crate dependencies; the optional
`c-import` feature now uses a pinned runtime-loaded libclang binding. SDL3 development files and pkg-config
are optional for ordinary examples, required for the real SDL3 integration.
Zig is optional; writable Zig caches may need configuring in a cloud sandbox.

## Read first

1. [PLAN.md](../PLAN.md), including its implementation amendments and section
   19 phase gates, remains the specification.
2. [Phase status](phase-status.md) records implemented subsets and open gates.
3. [README](../README.md) describes accepted syntax and CLI commands.
4. [C interoperability](c-interop.md), [library APIs](../stdlib/README.md), and
   [blockers](blockers/remaining-phases.md) describe current contracts.
5. [Continuation plan](CONTINUATION_PLAN.md) gives the next tasks and checks.

## What is available

The Rust compiler has braced functions/blocks, semicolon statements, inference,
mutable locals, records, concrete generic ADTs, exhaustive matching, a reference
interpreter, and C11 output. The restricted owner path supports hosted boxes,
fixed-length owned bytes, finite owner-bearing enums, moves, call-scoped
borrows, owner returns and scoped cleanup. `std.alloc` exposes recoverable
allocation through ordinary `Result` values. Bundled JSON modules parse i32
documents and serialize into caller-provided byte storage. SIMD has scalar
`u8x16` operations and opt-in AArch64 NEON.

Source `@include`, `@link` and `@pkg_config` directives drive native checking,
building and execution. Reusable binding modules export foreign functions and
constants. The target C compiler verifies explicit signatures against headers;
raw pointer lifetime, resource destruction and foreign effects remain the
programmer's responsibility. An optional `c-import` feature now imports explicitly selected declarations
through libclang; see the [current interop workflow](c-interop.md#import-selected-declarations)
and [cloud verification](verification/2026-11-03-selected-c-import.md).

The [SDL3 game](../examples/sdl3-direct/README.md) imports its bindings from
`sdl.rill` and calls the real system library without a C shim:

```sh
cargo run -- check examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill
cargo run -- run examples/sdl3-direct/main.rill --zig-cc
```

## Verification at the checkpoint

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

A full regression run passed 287 tests. A subsequent declaration-validation
fix added one regression; the final affected-component run passed 106 tests:

```sh
cargo test --lib --test native_modules --test ffi --test module_compile --test sdl3_direct
```

Formatting and strict all-target Clippy passed after the fix. Independent
reviews checked native ABI handling, parser scopes, allocation-failure cleanup,
native modules and the final validation fix. The real three-frame SDL3 variant
ran successfully under the dummy video driver through both `cc` and `zig cc`.
Tests explicitly skip that integration when SDL3 development files are absent;
a skip is not evidence of a working SDL3 installation. Physical desktop input
was not simulated.

Two reviewed regressions must remain covered: source `@link` must not duplicate
the library already supplied by pkg-config, which broke Zig-built dylib loading
on macOS; compatible duplicate extern declarations must each be validated
before coalescing, so later duplicate parameter names cannot evade checking.

Do not claim the entire plan, full memory safety, effect system or stable ABI
is complete. Restricted owner programs still reject foreign declarations.
There are no pending source edits needed to repair a known failing check.
Clang can emit a cosmetic `-x none` warning for a library-only link command.

## Suggested skills and agent setup

Use `plan` and `delegate` for phase implementation, `verify` before reporting
success, and `handoff` when moving environments again, if those skills are
available. Preserve the project's Rust/C direction and record genuine blockers under `docs/blockers/`.

## Reconstructed source-tree increment

After cloud filesystem loss, the grammar-tree work was reconstructed from the
restored baseline, not recovered from the missing old commit. Fresh checks are
recorded in [grammar-tree verification](verification/2026-11-03-grammar-source-tree.md).
The existing parser now records lossless grammar nodes and bounded top-level
recovery with structured diagnostics. Declaration maps require an error-free
parse. Resolved/IR mapping, finer recovery and P1 acceptance remain open.

The subsequent reconstructed [AST origin pass](ast-source-origins.md) covers all
parsed structural node families, including explicit block/unit/unary/record
provenance. It is optional and cannot alter strict parse success. Typed/generated-
code mappings still need separate work; module preservation follows below.

The next [module origin increment](module-source-origins.md) adds an opt-in
owned result for the exact flattened program, preserving structural origins
through the existing resolver/rewriter and all compatible native contributors.
Ordinary loading/checking and errors are unchanged. Typed/backend mapping and
source-aware checker diagnostics remain open.

The [function-instance origin index](typed-function-origins.md) adds a separate
opt-in result owning the exact source snapshots and ordinary verified typed
program. It brands query handles to that result and records complete closed
function signatures, all requesting source calls and explicitly uninstantiated
definitions. This is not a complete typed-node/generated-C source map and does
not expand native or owner-language support. Existing compilation APIs remain
unchanged; broader origin coverage needs separate construction-site tracing.
See its [fresh verification record](verification/2026-11-03-typed-function-origins.md)
for the exact author-tested source and remaining combined-native integration gate.

The combined scalar-native and function-origin checkpoint passed its
[integrated matrix](verification/2026-11-03-typed-origin-integration.md), with
584 default and 618 importer-enabled tests. The index remains optional and
does not cover typed nodes or generated C ranges.


The final [raw native integration and capture checkpoint](verification/2026-11-03-typed-native-raw-integration.md)
passed 614 default and 648 importer-enabled tests, both strict Clippy checks and
both Windows GNU compile checks. Twelve actual direct/generated/collector SDL3
software captures agree across GCC/Clang and O0/O2. Use the opt-in
[capture matrix](sdl3-capture-matrix.md) to reproduce the bounded visual gate;
physical input and hardware rendering are not covered.


The separate [Copy-body origin API](typed-copy-node-origins.md) now preserves
validated expression/operand/pattern and capture/shape metadata through the
existing semantic lowerer. Its [integrated checkpoint](verification/2026-11-03-copy-node-origins.md)
passed 661 default and 695 importer-enabled tests, both strict Clippy and
Windows GNU compile configurations, and 39 independent combined probes. It
remains a pure Copy-body mapping API, not native/owner-cleanup or generated-C
byte coverage.

## Explicit native configurations

The bounded Unix selected-C configuration increment passes 678 default and
726 importer-enabled tests, formatting, both strict lint modes and both Windows
compile configurations, plus 41 independent command probes. Actual configured
SDL3 O0/O2 execution is covered. See [the configuration record](verification/2026-11-03-native-configurations.md)
for exact source revisions and limits. Legacy generated source never auto-loads
a configuration. P5, non-Unix supervision, cache identity and broader foreign
contracts remain open.


The bounded [nullary-phantom increment](typed-nullary-phantoms.md) retains frozen,
compilation-branded logical arguments separately from verified tag-only layouts.
Ordinary typed evaluation/C now handles dynamic nullary tags, weak/mutable
values, generic forwarding and existing root loans without choosing a default
type. Canonical full-declaration checks prevent inactive-payload erasure;
optional origin APIs remain closed. The [integrated K1 verification](verification/2026-11-03-nullary-phantoms.md)
passed 715 default and 763 importer-enabled tests, both strict Clippy and Windows
compile modes, plus independent probes and GCC/Clang O0/O2 execution.
The accepted [K2 product follow-on](typed-phantom-products.md) proves complete
finite Copy field-schema reconstruction, including nested symbolic fields and
`Option<Flag<opaque>>`. Its [integrated matrix](verification/2026-11-03-phantom-products.md)
passed 762 default and 810 importer-enabled tests, both strict Clippy and Windows
compile modes, plus independent malformed-layout and native/C checks. Genuinely
unknown stored payloads and bare unresolved/no-return values remain out of scope.

## Nonreturning results

The bounded K3 representation has [integrated acceptance](verification/2026-11-04-nonreturning-results.md)
on source `bdd878fe`: 860 default and 908 importer-enabled tests, both strict
Clippy modes and both Windows GNU compile checks. Independent validation adds
15 retained private regressions and 80 GCC/Clang O0/O2 executions with exact
argument/depth events. Existing K1/K2 negative expectations were explicitly
migrated where K3 adds support; unknown physical payloads remain rejected.

Bare recursive results use no storage or fabricated values. Mandatory source
correspondence and separate source/IR flow proofs precede consumption. See the
[bounded contract](blockers/nonreturning-results.md) for current exclusions;
interrupted eager arguments and bounded discarded returning enum results are
accepted below. The full language plan remains open.


## Interrupted eager arguments

The accepted bounded implementation is documented in
[interrupted arguments](blockers/interrupted-arguments.md). It retains an explicit
prefix/barrier computation plus full source call evidence instead of creating an
unknown argument slot or a never-entered executable instance. Source checking,
independent source/IR flow, main-inclusive depth and original-source correspondence
remain mandatory. The [integrated matrix](verification/2026-11-04-interrupted-arguments.md)
passed 892 default and 940 importer-enabled tests, both strict Clippy modes and
both Windows GNU compile checks. Independent verification adds 12 private tests
and 76 GCC/Clang executions; two fixtures retain documented Clang style warnings.
Bounded normally returning discarded enum results are accepted below; full PLAN
gates remain open.

## Normally returning discarded results

The accepted bounded [discard-result implementation](blockers/discarded-results.md)
adds demand-aware instances, real normal void returns, enum operand effects and
mandatory full logical/source correspondence. Direct record discard has further
acceptance below. Local-shape mixing, observed/used/mutable unknown values and
new owner/native combinations remain excluded. The [integrated record](verification/2026-11-04-discarded-results.md)
passes 939 default and 987 importer-enabled tests, both strict Clippy modes and
both Windows GNU compile checks, with no dependency skips.

The [frozen author record](verification/2026-11-04-discarded-results-author.md)
identifies source `ee5763f`: 504 focused GCC tests, 9 additional Clang tests,
124 new-suite C executions, formatting and default strict all-target Clippy.
The [independent record](verification/2026-11-04-discarded-results-independent.md)
adds 25 private proof tests and 212 final C executions; an explicitly migrated
K3b expectation has 80 further C executions. Known legacy Clang match warnings
are retained. The accepted direct-record follow-on is described below.


## Direct discarded records

A bounded structural follow-on now represents direct record construction under
discard demand, retaining written field order, canonical schema indices and all
logical children without an aggregate or local slot. Independent reconstruction
checks complete field coverage/substitution and source occurrences, while actual
flow and effects/traps remain preserved. See the
[bounded contract](blockers/discarded-results.md#direct-record-follow-on).
[Integrated acceptance](verification/2026-11-04-discarded-records.md) passes
996 default and 1,044 importer-enabled tests, strict lint and Windows compile
checks, with no dependency skips. Unknown
physical storage, used/mutable/projected layoutless values, local-shape mixing,
unknown nominal interruption and new owner/native forms remain excluded.

The [frozen author record](verification/2026-11-04-discarded-records-author.md)
identifies source `93046a1`: 640 focused tests, 17 additional Clang tests, 188
new-suite C executions and 216 older-harness regression executions. Formatting
and default strict Clippy pass. The [independent record](verification/2026-11-04-discarded-records-independent.md)
adds 24 private tests, 24 typed cases and 156 C executions. The subsequent closed
function-result annotation increment is accepted below.


## Closed function-result annotations

Optional `-> TypeExpr` on braced `fn` and `export fn` has
[integrated acceptance](verification/2026-11-04-result-annotations.md) on source
`93b1188`. The fresh matrix passes 1,034 default and 1,082 importer-enabled tests,
formatting, both strict lint modes and both Windows GNU all-target compile checks.
There are no failures, ignored tests or SKIP notices. Windows binaries were not
executed. Fresh independent and author annotation harnesses separately execute
68 and 280 GCC/Clang C11 O0/O2 cases.

At that result-only checkpoint, closed equality constraints were installed across
each recursive group before its bodies; parameters retained their then-existing
per-function timing. The recursive-signature increment below changes that timing.
Complete source/origin roles, module privacy, exact logical result identity and
borrowed graph-wide input/resolved-name budgets are checked. The
[independent record](verification/2026-11-04-result-annotations-independent.md)
distinguishes two environment losses from the new evidence. The author-stage
reports remain historical records, not current pending gates.

The accepted setup uses Rust/Cargo 1.99.1, GCC 14.2, Clang/libclang 19.1.7 and
SDL3 3.2.10 on x86_64 Linux. The configured libclang tests explicitly propagate
`RILL_TEST_CLANG_RESOURCE_DIR` through `--clang-resource-dir`; raw default header
search is not assumed. No new Zig, AArch64, GPU or physical-input evidence is
claimed. Kinds/skolems, genuinely unknown stored layouts, broader owner/native
support and broader optional provenance remain open. This does not complete P1
or C02.


## Closed recursive signatures

The bounded [recursive-signature increment](recursive-signatures.md) has
[integrated acceptance](verification/2026-11-04-recursive-signatures.md) on source
`31cac927`: 1,065 default and 1,113 importer-enabled tests, formatting, both strict
lint modes and both Windows GNU all-target compile checks pass. Neither test run
reports failures, ignored tests, SKIP notices or warnings. Windows checks are
compile-only. Real direct SDL3 runs in both test modes; generated SDL3 bindings
run in importer mode. No new framebuffer capture or physical-input claim is added.

The checker installs all closed results, then all closed parameters, before any
body in each recursive group; body order and ordinary generalization remain.
Parameter/result input trees share the bounded allowance, with a separate exact
resolved-name pass before module cloning. Combined totals can newly reject, and
the diagnostic is now `E-TYPE-COMPLEXITY: signature annotation input budget exceeded`.
Source roles, modes, loans, snapshots, nominal identity and privacy remain intact.

The [author record](verification/2026-11-04-recursive-signatures-author.md) retains
708 selected tests and 176 GCC/Clang C11 O0/O2 executions; the separate
[independent record](verification/2026-11-04-recursive-signatures-independent.md)
retains 715 selected tests and 112 C executions. Those overlapping selected suites
and repeated runs are not added to the aggregate totals. Unannotated callee-result
projection and unconstrained intermediate parameters were limited at that
checkpoint; the bounded follow-on below addresses them. Owner-record support is unchanged. This does not close P1, C02 or broader annotated interfaces.


## Deferred nominal projections

The bounded [projection increment](deferred-projections.md) has
[integrated acceptance](verification/2026-11-04-deferred-projections.md) on exact
source `81c8c151`: **1,103 default and 1,151 importer-enabled tests**, formatting,
both strict all-target lint modes and both Windows GNU compile-only modes pass.
Each test mode reports zero failures, ignored tests, SKIP notices or warnings.
Real direct SDL3 runs in both modes; generated bindings and configured O0/O2 SDL
run in importer mode. No new capture, Windows execution, GPU or physical-input
claim follows.

Ordinary equalities within one monomorphic component now resolve nominal field
reads, assignment and read/inout paths before generalization. Exact per-use roots,
canonical metadata, value restriction, symbolic loan overlap and independent
typed verification remain enforced. Admission/work budgets are explicit; eager
paths can also exhaust them. Unresolved standalone components still reject, and
no structural label search or universal order-independence guarantee is added.

The [independent record](verification/2026-11-04-deferred-projections-independent.md)
retains 694 selected tests, 112 independent C executions and the clean committed
340-run replacement of the author matrix. Its eight Clang style warnings and the
five author/two independent inherited reference-C local-shape rejections remain
explicit. The [author record](verification/2026-11-04-deferred-projections-author.md)
keeps the original dirty-worktree provenance and retained preliminary failures.
Repeated runs are not added to historical or aggregate totals.

This bounded projection task is complete. No layout, owner/native form or
completed P1/P2/C02 phase gate is claimed. Owned records remain unsupported.
Broader inference, kinds/skolems, unknown physical storage and optional provenance
remain open.
