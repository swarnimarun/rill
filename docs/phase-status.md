# Delivery status

The gates in `PLAN.md` section 19 define completion. This file records progress,
not a replacement for those gates.

- P0: the supplied baseline and provisional decisions are documented. There
  was no previous compiler or test suite in this repository. Normative examples
  exist; review of the remaining decisions is still needed.
- P1: a sequential expression subset, type checker, reference interpreter and
  C emitter exist. Generic ADTs and exhaustive patterns work in the interpreter.
  Braced `fn` declarations and semicolon-separated blocks are supported;
  legacy expression syntax remains accepted. Programs with concrete finite
  Copy enums, including generic function/type instances and nested or
  literal patterns, now use one independently checked typed path in the
  interpreter and C backend. C emits compact tagged unions and tag-only nullary
  enums; a separate checked AST evaluator remains a differential oracle.
  The private equality solver now uses interned terms, ranked union-find, levels,
  iterative occurs checks and independently cross-checked generalization.
  Immutable local lets now apply a conservative nonexpansive value restriction,
  with weak-variable lowering to prevent alias regeneralization. Kinds, rigid
  escape checks remain open. Eligible immutable local schemes now use bounded
  capture-and-shape elaboration with exact concrete whole-value/field uses;
  unresolved real uses and weak monotypes still have explicit layout limits. See the
  [solver checkpoint](verification/2026-11-03-type-solver.md) and
  [value restriction](verification/2026-11-03-value-restriction.md).
  A bounded deterministic worklist now specializes reachable generic functions
  onto that same checked path, including recursive groups and return-only type
  variables. Copy root/field loans and computed read temporaries also share
  that path, with independent argument-mode and interval checks. Finite Copy records
  use tag-free product layouts, ordered construction, independently checked
  field paths and root/field loans, including prefix-overlap checks and
  original-slot forwarding. Existing `u8x16` operations also share that path,
  with independently checked signatures, compact 16-byte C values and unchanged
  scalar/opt-in NEON behavior. [Local value shapes](decisions/0004-local-value-shapes.md)
  retain one initializer evaluation using private concrete capture records and
  no fabricated phantom types. Broader polymorphic representations,
  typed/backend source mapping, the complete verified MIR boundary and the full
  acceptance suite remain open. Checked native transport now carries all
  declaration/configuration metadata and the existing scalar/static-cstr calls,
  unsigned/float/raw-pointer values, conversions, null operations and same-type
  equality. Closed Copy aggregates, concrete generics, root/field loans and
  once-only native constant captures share this typed C path. It preserves
  exact pointer identities, unused header assertions, snapshots and reached-only
  evaluator rejection. All-nullary enums with unresolved phantom arguments now
  have separate branded logical identities and verified tag-only runtime layouts,
  including dynamic tags, weak/mutable values, generic calls and root loans.
  See the [bounded K1 contract](typed-nullary-phantoms.md)
  and [integrated verification](verification/2026-11-03-nullary-phantoms.md),
  with 715 default and 763 importer-enabled tests passing.
  The accepted [K2 product increment](typed-phantom-products.md) verifies
  complete finite Copy fields with opaque logical arguments, including
  `Option<Flag<opaque>>`, while rejecting actual unknown stored payloads.
  Its full matrix passed 762 default and 810 importer-enabled tests, with
  independent malformed-layout/native/C checks.
  The accepted [K3 nonreturning-result increment](blockers/nonreturning-results.md)
  adds mandatory source/control correspondence and no-storage computation results,
  retaining actual arguments, calls and the depth guard. Its [final matrix](verification/2026-11-04-nonreturning-results.md)
  passed 860 default and 908 importer-enabled tests plus independent event/C and
  malformed-proof checks. The subsequent bounded
  [interrupted-argument increment](blockers/interrupted-arguments.md) has an explicit
  computation for ordinary value calls stopped by a nonreturning argument,
  preserving full source evidence and real evaluation order. Its
  [integrated acceptance](verification/2026-11-04-interrupted-arguments.md)
  passed 892 default and 940 importer-enabled tests plus independent event/C
  and malformed-proof checks. Normally returning discarded enum results now have
  the bounded acceptance below; genuinely unknown storage remains open.
  Other unsupported Copy layouts retain the uniform fallback. This is not P1 or P5 completion; see the
  [F2 evidence and limits](verification/2026-11-03-typed-native-transport.md) and
  [final integrated checks and SDL captures](verification/2026-11-03-typed-native-raw-integration.md).
- P2: initialized mutable locals, nominal records, and call-scoped shared `read`
  and exclusive `inout` loans of roots or record field paths up to 16 segments
  are supported.
  A hosted `i32` box and a fixed-length owned byte buffer have restricted moves,
  direct owner returns with caller cleanup transfer,
  call-scoped shared/exclusive borrows and scoped cleanup in acyclic
  monomorphic owner programs. Finite concrete enums can contain nested owners;
  consuming matches transfer their fields and clean up the active variant.
  Structured branches can move enclosing owners when all arms agree on the
  resulting ownership state. Primitive and nested Copy matches can coexist
  with owners while preserving the same branch ownership joins and cleanup.
  `std.alloc` exposes recoverable system box/byte allocation through ordinary
  affine `Result` and canonical `AllocError` values, with checked sizes and
  fault-injection coverage. General move checking, loan inference, allocator
  capabilities and cleanup are not implemented.
  P3–P11 remain open; their dependencies and evidence remain as stated in the
  plan. Explicit native dependency directives, exact scalar/raw-pointer
  declarations,
  target-header compatibility checks and a direct SDL3 game exercise part of
  P5. Default-enabled target-configured libclang import generates referenced or
  explicitly selected scalar/raw-pointer functions and integer/bool constants as checked binding
  modules. The Unix import CLI now supervises a separate worker with bounded
  IPC, timeout/cancellation and process-group cleanup; the library API remains
  in-process, and neither is a memory/security sandbox. Aggregate ABI adapters,
  broader declaration import and checked
  foreign lifetime/effect/resource contracts remain open. See
  [C interoperability](c-interop.md). Static UTF-8 literals and
  fixed-capacity byte values support JSON validation and bounded decimal and
  string serializers, but do not establish the P3 owned-library gate. File-based
  imports, exports, nested local paths and explicitly bundled `std` modules
  exercise part of P4. Reusable native binding modules support exported
  functions/constants, private wrappers, transitive dependencies and checked
  duplicate declarations. Package and stable interface support remain open.
  `std.json_input.parse_i32` returns an ordinary `Result` with typed byte-offset
  errors. Explicit `u8x16` operations and opt-in AArch64 NEON exercise a
  small part of P10, without completing its optimization or conformance gate.

Source tooling retains exact tokens/trivia and byte/scalar spans through a shared
lexer. The conservative formatter preserves parsing and is corpus-idempotent.
A shared-parser grammar tree now adds production nodes, declaration source
mapping, structured diagnostics and bounded top-level recovery. Any error
suppresses its AST/map; lexical failures preserve unlexed bytes without recovery.
The optional AST origin map now covers all parsed structural node families with
explicit desugaring provenance. An opt-in module loader now retains owned
snapshots and validated structural origins through the existing resolution,
renaming and native coalescing pass. An immutable, snapshot-branded
[function-instance origin index](typed-function-origins.md) now preserves resolved
definitions, exact closed signatures and all specialization request sites. It
does not itself map typed expressions, storage, captures, cleanup or generated C.
A separate [Copy-body index](typed-copy-node-origins.md) now maps pure reached
bodies, operands, patterns and capture/shape materializations with independent
correspondence checks. Its [combined verification](verification/2026-11-03-copy-node-origins.md)
passed 661 default and 695 importer-enabled tests. Native/owner-cleanup coverage,
complete storage/backend mapping, finer recovery and editor integration remain
open. See [module origins](module-source-origins.md),
[source tooling](source-format.md) and its
[fresh grammar verification](verification/2026-11-03-grammar-source-tree.md).

Run `cargo test` for the currently supported subset. The C differential tests
require `cc` and compare interpreter and emitted C results at `-O0` and `-O2`.

An explicit selected-C configuration persistence increment passes 678 default
and 726 importer-enabled tests plus independent command probes. It keeps legacy
commands unchanged and requires fresh supervised validation for opted-in
capture/materialization and consumers. This bounded acceptance closes no full
phase gate. See the [configuration record](verification/2026-11-03-native-configurations.md).

The accepted bounded [discard-result increment](blockers/discarded-results.md) covers
normally returning unused immutable enum results through a separate logical-only
computation, with real call/return effects. This narrows one P1 representation
gap. Its [integrated checks](verification/2026-11-04-discarded-results.md) passed
939 default and 987 importer-enabled tests, strict lint and Windows compile modes
plus independent proof/event/C cases. The direct-record follow-on is accepted
below; unknown physical storage, observed unknown results and full phase gates
remain open.


The accepted direct-record follow-on extends the discard mechanism
with schema-indexed fields in written evaluation order and no physical record.
Complete field coverage/substitution, source roles, normal-flow reconstruction
and preflight bounds remain mandatory. Its [integrated checks](verification/2026-11-04-discarded-records.md)
passed 996 default and 1,044 importer-enabled tests, both strict lint modes and
both Windows compile modes. This does not close P1 or broaden unknown storage,
projections or owner support.
The [author record](verification/2026-11-04-discarded-records-author.md) reports
640 focused tests and 188 new-suite GCC/Clang C executions. The separate
[independent record](verification/2026-11-04-discarded-records-independent.md)
adds 24 private tests and 156 C executions.


Closed result annotations on braced `fn` and `export fn` now have bounded
[integrated acceptance](verification/2026-11-04-result-annotations.md) on source
`93b1188`: 1,034 default and 1,082 importer-enabled tests, formatting, both strict
lint modes and both Windows GNU compile-only checks pass, with no test failures,
ignored tests or SKIP notices. Independent checks add 68 GCC/Clang C11 O0/O2
executions; the freshly repeated author matrix contributes 280 separately counted
executions. At that result-only checkpoint, SCC-wide result constraints preserved
the then-existing per-function parameter timing, module privacy, full logical
identity and exact source/origin roles. Graph-wide source/resolved annotation
budgets precede cloning. This adds no unknown storage,
kinds, skolems, owner/native combinations or broader optional provenance. P1 and
C02 remain open; Windows execution and other untested targets are not claimed.


The bounded [recursive-signature increment](recursive-signatures.md) has
[integrated acceptance](verification/2026-11-04-recursive-signatures.md) on source
`31cac927`: 1,065 default and 1,113 importer-enabled tests, formatting, both strict
lint modes and both Windows GNU compile-only checks pass. The test runs report
zero failures, ignored tests, SKIP notices or warnings. Results constrain
monomorphic placeholders first, then all closed parameters, then source-order
bodies and ordinary generalization. Borrowed annotation budgets now cover
parameters and results together before recursive consumers and module cloning.
This intentionally changes parameter timing and combined admission totals.
The author record retains 708 selected tests and 176 C executions; the separate
independent record retains 715 selected tests and 112 C executions. Overlapping
and repeated runs are not added. Broader projection inference remains limited,
owner-record support is unchanged, and P1/C02 remain open. Windows execution is
not claimed.


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

## Local compiler and editor integration

The local compiler/editor work is ported onto `v1-ernie` in small verified
slices. Full default/minimal suites, required real GPU checks and final editor
workflows passed; see the exact results and capability skips below.
The upstream typed-native, configuration, logical-layout, signature and
projection checks remain the foundation. The port adds Copy callables,
boolean negation, unit-valued omitted `else`, automatic referenced-header
imports, native optimization defaults, guarded editor interfaces, the LSP
and editor packages, plus SDL lifecycle and voxel demonstrations.

The [integration plan](superpowers/plans/2026-11-04-ernie-integration.md),
[decision trail](verification/2026-11-04-ernie-decisions.tsv) and
[fresh verification](verification/2026-11-04-ernie-integration.md) record each
slice. Historical counts above describe their own checkpoints. They are not
a fresh aggregate result for this port. No P1, P4, P5 or later phase is marked
complete. General effects, affine closures, foreign ownership and broader
layout/provenance coverage retain their documented limits.


## Project tooling, CLI tokens and reload hooks

The local project driver adds `new` and `project check/build/run/watch` through
strict version-1 `rill.json` manifests and structured compiler arguments.
[Project documentation](projects.md) describes path rules and the executable
example. Native watch builds candidates before replacing the running process;
a rejected candidate retains it. Default watching follows imported sources,
with explicit extra native/asset inputs. Unchanged valid input graphs need no
repeat import discovery. Callable detection/conversion now use indexed source
declarations; the retained benchmark measures that compiler operation alone.

`std.cli` provides allocation-free classification of supplied static-string
tokens, spans and checked i32 parsing with typed errors. Runtime argv and general
I/O remain open. `std.reload` provides explicit generation-context decisions.
The optional `reload::Session` embeds synchronous, closed integer hooks in the
checked interpreter; exclusive replacement checks the complete candidate and
keeps the accepted generation after failure. It does not patch native code.
See [reload contracts](hot-reload.md), [build scaling](build-performance.md) and
the [feature matrix](language-features.md). Native per-function ABI/state
migration remains a [documented blocker](blockers/hot-reload.md).

No phase gate or Rust/Zig feature parity follows from this tooling increment.
Fresh [verification](verification/2026-11-04-project-cli-reload.md) records this
work separately from historical counts, including the configured SDL SDK setup
failure and its passing environment-only retest.
