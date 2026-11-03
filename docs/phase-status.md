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
  acceptance suite remain open. The first checked native-call increment carries
  all declaration/configuration metadata and lowers existing `i32`/`bool` calls
  and constant reads, `unit` results and direct static `cstr` arguments to typed
  C. It preserves unused header assertions, argument snapshots and reached-only
  evaluator rejection. Raw foreign scalar/pointer transport still uses the
  uniform Copy fallback. This is not P1 or P5 completion; see the
  [F1 evidence and limits](verification/2026-11-03-typed-native-calls.md).
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
  P5. Optional target-configured libclang import generates explicitly selected
  scalar/raw-pointer functions and integer/bool constants as checked binding
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
does not map typed expressions, storage, captures, cleanup or generated C.
Complete typed/backend mapping, finer recovery and editor integration remain
open. See [module origins](module-source-origins.md),
[source tooling](source-format.md) and its
[fresh grammar verification](verification/2026-11-03-grammar-source-tree.md).

Run `cargo test` for the currently supported subset. The C differential tests
require `cc` and compare interpreter and emitted C results at `-O0` and `-O2`.
