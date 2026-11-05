# Scalar C export implementation plan

> **For agentic workers:** Implement the bounded export API after the docs checkpoint, then review its verifier and C consumer independently.

**Goal:** Emit a checked C11 library interface for explicit closed integer functions as a prerequisite for native function reload.

**Architecture:** Analyze the entire source library, validate explicit exports, seed verified typed instances from real source signatures, and emit wrappers using their actual verified function IDs. Library emission omits an executable main. A C header and versioned JSON manifest describe only the admitted external scalar interface.

**Tech Stack:** Rust compiler, existing typed IR/verifiers, C11, existing serde_json.

**Spec:** User request to advance ABI/function-level reload; PLAN.md sections 14.5, 15 and 19; docs/blockers/hot-reload.md.

## Global constraints

- Preserve ordinary executable lowering, independent verification and C output.
- Do not fabricate a synthetic main, argument values or foreign signatures.
- Analyze every source declaration before exporting a subset.
- Reject owners, native declarations/dependencies, raw values, callables and unsupported symbolic/control proofs.
- The initial external contract is closed monomorphic value-mode i32 parameters/results, at most 64 exports and 32 parameters per export.
- Require explicit distinct C names under the rill_ prefix; validate identifiers and collisions.
- This builds an export boundary, not a dynamic loader or live-state migration guarantee.

## Task 1: Rooted checked library lowering

Files: src/typed/instances.rs, src/typed.rs and a focused private helper if needed.

- [x] Seed reachable instances from exact requested function signatures instead of an executable main.
- [x] Share body lowering and entry-independent verification while preserving all executable main checks.
- [x] Keep unsupported logical/control-extension evidence rejected; do not weaken a verifier to admit the new mode.
- [x] Run existing typed/instance/control tests and new malformed-export regressions.

## Task 2: C exports and interface artifact

Files: src/abi.rs, src/lib.rs, src/typed/c.rs, tests/abi_exports.rs.

```rust
pub struct Export { pub source: String, pub c_name: String }
pub struct Artifact { pub c: String, pub header: String, pub manifest: String }
pub fn emit_i32_library(
    program: &rill::ast::Program,
    exports: &[Export],
) -> Result<Artifact, String>;
```

- [x] Validate closed integer source signatures and explicit export names before typed lowering.
- [x] Emit library C without main, calling actual verified function IDs from public wrappers.
- [x] Emit a C11 header and deterministic ABI-version-1 JSON manifest with names, parameter types and results, omitting private IDs and host paths.
- [x] Compile C callers at O0/O3, compare exact results, and cover recursion/traps and rejection of incompatible modes/types/native/resources.
- [x] Verify deterministic artifacts and rejection of invalid unused source declarations.

## Task 3: Evidence and checkpoint

- [x] Independently review root seeding, entry verification and generated C wrapper identity.
- [x] Document supported signatures, trap/call-depth behavior, target C ABI assumptions, host call/thread boundaries and remaining loader/quiescence/state work.
- [x] Run appropriate formatter, strict lint, library/backend regression and C caller tests.
- [x] Commit the verified ABI increment separately; do not push or mark a full PLAN phase complete.

The feature checkpoint records completed targeted evidence. The broader
aggregate native matrix is running and will receive a separate verification
update when it finishes. No acceptance phase is marked complete.

## Review decisions

- Library depth is thread-local because independent pure host calls have no
  shared Rill state. Ordinary executable C stays byte-identical, including its
  existing depth declaration. Native callbacks and signal reentrancy remain
  unsupported.
- Independent review found dormant raw signatures could bypass an expression-
  only query. The shared private `check/raw_types.rs` admission scan now covers
  signatures, expression/application metadata and each declaration schema, with
  a 4,000,000-node budget and no nominal expansion. ABI and interpreter reload
  use it. The existing expression-only query and general checker/provenance
  coverage policies remain unchanged.
- Regression cases cover source reorder, bundled reload hooks, raw/CStr/owner
  signatures, raw nested callables and declaration fields, and valid generic
  parameter shadowing. Rejected interpreter replacements retain the accepted
  generation and usable old calls.
