# Implementation after the v1 checkpoint

Implementation is paused. This is the work queue for an explicitly resumed
cloud session. [PLAN.md](../PLAN.md) defines the full requirements; this file
orders the next work without replacing its acceptance gates.

## 1. Establish the new environment

- [ ] Check out `v1`, read [CLOUD_HANDOFF.md](CLOUD_HANDOFF.md) and `AGENTS.md`.
- [ ] Run formatting, the full tests and strict Clippy using the commands in
  the handoff. Record compiler/target versions and optional integration skips.
- [ ] Run `examples/blocks/main.rill` and `examples/owned/fallible.rill` through
  both interpreter and native C execution. Both currently print `42`.
- [ ] If SDL3 is installed, run `cargo test --test sdl3_direct -- --nocapture`.
  Do not infer cross-platform correctness from the macOS checkpoint evidence.

## 2. Import selected C declarations

This is the next practical FFI task requested by the user. Current `@include`
checks manually selected declarations; it does not generate them.

- [ ] Trace `src/c_import.rs`, native CLI handling in `src/main.rs`,
  `ast::ExternType`, foreign validation in `src/check.rs`, ABI checks in
  `src/backend.rs`, and module export/coalescing in `src/modules/compile.rs`.
- [ ] Compare a Clang JSON subprocess with libclang's stable C API. Choose
  after probing the actual cloud toolchain; no dependency or importer ABI
  was committed at the checkpoint. Preserve the user's selected target,
  sysroot, defines and include paths. Record the decision before implementation.
- [ ] Start with an explicit list of requested functions and constants. A
  proposed `rill import-c` command can emit an ordinary Rill binding module
  containing dependency directives and `export extern` declarations. Its CLI
  shape is a proposal, not a supported command or frozen API.
- [ ] Keep preprocessing and declaration/type extraction inside the C
  frontend. Do not parse headers with regular expressions or infer ABI
  compatibility from size alone. Keep nominal opaque pointee names and
  constness. Obtain scalar widths from the selected target.
- [ ] Reject unsupported aggregates, callbacks, variadics, calling conventions
  and macros with a symbol-specific diagnostic. Function-like macros are not
  foreign function symbols. Define constant-expression handling explicitly.
- [ ] Validate generated bindings through the existing Rill and target C
  checks before publishing output. On failure, leave no partial binding file.
  Use structured argv, bounded output and cleanup for external tool resources.
- [ ] Test typedefs, scalar widths/signedness, opaque pointer constness,
  unknown symbols, macro constants, rejected signatures, preprocessing flags,
  spaces in paths, missing tools and malformed tool output. Run actual calls
  against C fixtures at `-O0` and `-O2`; generate and use selected SDL3 bindings.
- [ ] Update interoperability docs, example generation instructions and the
  FFI blocker. Keep automatic import separate from foreign lifetime/effect
  proofs. Review the implementation and run affected plus full regressions.

Useful primary references are the [libclang tutorial](https://clang.llvm.org/docs/LibClang.html)
and [libclang type-query API](https://clang.llvm.org/doxygen/group__CINDEX__TYPES.html).
Research was paused before choosing an importer design.
The [preliminary investigation](research/selected-c-import.md) records the
libclang recommendation, local feasibility probe and remaining design questions.

## 3. Finish the sequential and ownership foundations

Follow P1/P2 gates before claiming general safety. Consolidate all accepted
programs onto the independently checked semantic representation; the current
general C path and restricted typed owner path are different implementations.
Improve source spans and diagnostic recovery, compact concrete layouts,
initialization and partial-move tracking, owned record fields and control-flow
loan analysis. Keep interpreter/C differential cases and exact cleanup traces.

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
