# Implementation after the v1 checkpoint

Implementation resumed in the cloud on 3 November 2026. This is the work queue
for continuing the experimental checkpoint on `v1-ernie`.
[PLAN.md](../PLAN.md) defines the full requirements; this file orders the next
work without replacing its acceptance gates. Exact cloud check results and
optional skips are recorded in [verification](verification/2026-11-03-selected-c-import.md).

## 1. Establish the new environment

- [x] Check out `v1`, read [CLOUD_HANDOFF.md](CLOUD_HANDOFF.md) and `AGENTS.md`.
- [x] Run formatting, the full tests and strict Clippy using the commands in
  the handoff. Record compiler/target versions and optional integration skips.
- [x] Run `examples/blocks/main.rill` and `examples/owned/fallible.rill` through
  both interpreter and native C execution. Both currently print `42`.
- [x] Run the direct and generated-binding SDL3 integration with actual SDL3
  development files. Both pass on x86_64 Linux with GCC and Clang; the full
  importer-enabled suite reports no skips. See [native SDL3 evidence](verification/2026-11-03-sdl3-native.md).

## 2. Import selected C declarations

The first bounded increment is implemented behind `--features c-import`.
It uses libclang to generate explicitly selected scalar/raw-pointer declarations
and integer/bool constants, with ordinary Rill module and target-C validation.
See [the current importer contract](c-interop.md#import-selected-declarations).
The original acceptance checklist below remains useful for broader P5 work;
optional-target evidence must not be inferred from fixture success.

- [x] Trace `src/c_import.rs`, native CLI handling in `src/main.rs`,
  `ast::ExternType`, foreign validation in `src/check.rs`, ABI checks in
  `src/backend.rs`, and module export/coalescing in `src/modules/compile.rs`.
- [x] Compare a Clang JSON subprocess with libclang's stable C API. Choose
  after probing the actual cloud toolchain; no dependency or importer ABI
  was committed at the checkpoint. Preserve the user's selected target,
  sysroot, defines and include paths. Record the decision before implementation.
- [x] Start with an explicit list of requested functions and constants. The
  `rill import-c` command emits an ordinary Rill binding module
  containing dependency directives and `export extern` declarations. Its CLI
  is experimental and documented in `docs/c-interop.md`, not a frozen API.
- [x] Keep preprocessing and declaration/type extraction inside the C
  frontend. Do not parse headers with regular expressions or infer ABI
  compatibility from size alone. Keep nominal opaque pointee names and
  constness. Obtain scalar widths from the selected target.
- [x] Reject unsupported aggregates, callbacks, variadics, calling conventions
  and macros with a symbol-specific diagnostic. Function-like macros are not
  foreign function symbols. Define constant-expression handling explicitly.
- [x] Validate generated bindings through the existing Rill and target C
  checks before publishing output. On failure, leave no partial binding file.
  Use structured argv, bounded output and cleanup for external tool resources.
- [x] Test typedefs, scalar widths/signedness, opaque pointer constness,
  unknown symbols, macro constants, rejected signatures, preprocessing flags,
  paths with spaces, missing compilers, malformed headers, validation bypasses,
  output atomicity and frontend/target disagreement. Generated bindings make
  actual C fixture calls at `-O0` and `-O2`. There is no text AST protocol to
  parse or malformed JSON output to accept.
- [x] Run generated selected SDL3 bindings against real SDL3. The installed
  SDL3 3.2.10 integration and identical direct/generated framebuffer checks pass.
  This does not establish physical input, hardware rendering or other targets.
- [x] Update interoperability docs, example generation instructions and the
  FFI blocker. Keep automatic import separate from foreign lifetime/effect
  proofs. Independent review and affected tests pass.
- [x] Isolate the Unix CLI importer in a same-executable worker with bounded
  versioned IPC, cancellable wall deadline, process-group cleanup and atomic
  publication. Exercise hangs, crashes, inherited pipes, malformed frames,
  output limits, interrupted imports and existing-output retention. See the
  [worker verification](verification/2026-11-03-import-worker.md).
- [ ] Extend target/configuration persistence and non-Unix worker supervision.
  Generated modules still require the same target/header configuration on later
  builds. The library API remains in-process; neither path is a memory/security
  sandbox.

Useful primary references are the [libclang tutorial](https://clang.llvm.org/docs/LibClang.html)
and [libclang type-query API](https://clang.llvm.org/doxygen/group__CINDEX__TYPES.html).
The [design investigation](research/selected-c-import.md) records the libclang
choice, feasibility evidence and the bounded implementation contract.

## 3. Finish the sequential and ownership foundations

Follow P1/P2 gates before claiming general safety. Consolidate all accepted
programs onto the independently checked semantic representation; the current
general C path and restricted typed owner path are different implementations.
Improve source spans and diagnostic recovery, compact concrete layouts,
initialization and partial-move tracking, owned record fields and control-flow
loan analysis. Keep interpreter/C differential cases and exact cleanup traces.

The first foundation increment brings concrete finite Copy enums and
primitive/nested Copy patterns onto the existing independently checked typed
representation. Supported production interpretation and C emission now share
that path; the checked AST evaluator remains an explicit differential oracle.
Compact tagged unions and tag-only nullary enums remove program-wide value
capacity from these programs. Copy matches also work inside restricted owner
programs without weakening move joins or cleanup. The subsequent increment adds
a bounded worklist of reachable generic function instances, using full concrete
parameter/result substitutions and
independent comparison with the original layout analyzer. Copy root `read` and
`inout` loans and computed read temporaries now share the typed path, with
independent argument-order loan verification. Finite Copy records now add
tag-free product layouts, ordered construction, checked field projections and
assignment, and whole-record root loans. Copy field loans now share the same
path, with checked prefix-overlap intervals and original-slot forwarding.
Existing `u8x16` operations now also use checked signatures and compact values
on this path. Eligible immutable local schemes now use once-evaluated captures
and exact concrete whole-value/field reconstruction. Unresolved real uses and
weak monotypes still require a representable concrete layout. Owned records,
foreign values, stable resolved IDs for all source constructs and complete MIR
coverage remain open. See the
[Copy-pattern verification](verification/2026-11-03-typed-copy-patterns.md),
[generic-instance verification](verification/2026-11-03-typed-specialization.md),
[root-loan verification](verification/2026-11-03-typed-copy-loans.md),
[Copy-record verification](verification/2026-11-03-typed-copy-records.md) and
[field-loan verification](verification/2026-11-03-typed-field-loans.md), and
[Copy-vector verification](verification/2026-11-03-typed-vectors.md), and
[local-value verification](verification/2026-11-03-typed-local-values.md).

## 4. Complete P3 and P4 contracts

Add closed effects, destructor summaries and allocator capabilities before
generalizing storage. Build growable vectors, owned strings and borrowed slices
with explicit failure behavior and allocator provenance. Extend ordinary
Result-based libraries, then traits, open effect rows, closure call kinds and
stable module/package interfaces according to their dependencies. Current
fixed owners and bundled modules do not establish those gates.

## 5. Advance the remaining plan in dependency order

Complete P5's aggregate ABI adapters, callbacks, exports and audited foreign
contracts. P6 requires a useful native application, debugging, allocation
reports and reusable math/data libraries. P7 through P10 add the advanced
static APIs, structured concurrency, transactions/search and optimization
programs specified in PLAN.md. P11 requires a compatibility policy, audited
standard library, reproducible releases and supported-target evidence.

For each phase, write a bounded implementation plan, build a working increment,
review it, run its acceptance checks, and update phase status. Put a concrete
unresolved blocker in `docs/blockers/` with evidence and the next experiment;
do not substitute a blocker note for feasible implementation work.

## Source-tooling foundation

A shared lossless lexer and conservative stdout/`--check` formatter now cover
exact token/comment retention, byte/scalar spans, parse preservation and
idempotence. See [source tooling](source-format.md) and
[focused verification](verification/2026-11-03-source-format.md). This is a
bounded P1/tooling increment. The subsequent reconstruction adds shared-parser
grammar nodes, structured diagnostics and bounded recovery, followed by a complete
structural parsed-AST origin map. Resolved/IR mapping and finer recovery remain
open. Existing source files were not mass-reformatted.

## Reconstructed source-tree increment

After cloud filesystem loss, the grammar-tree work was reconstructed from the
restored baseline, not recovered from the missing old commit. Fresh checks are
recorded in [grammar-tree verification](verification/2026-11-03-grammar-source-tree.md).
The existing parser now records lossless grammar nodes and bounded top-level
recovery with structured diagnostics. Declaration maps require an error-free
parse. Resolved/IR mapping, finer recovery and P1 acceptance remain open.

The subsequent reconstructed [AST origin pass](ast-source-origins.md) covers all
parsed structural node families, including explicit block/unit/unary/record
provenance. It is optional and cannot alter strict parse success. Resolved/module
origin preservation and typed/generated-code mappings still need separate work.
