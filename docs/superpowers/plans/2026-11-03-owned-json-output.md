# Owned JSON output implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:delegate (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Write JSON strings into a caller-owned runtime-sized byte buffer with a recoverable capacity result and no partial writes.

**Architecture:** A Rill local module exports `write_string(inout output, text, offset)`. A read-only preflight counts escaped bytes against remaining capacity before taking the first write. Success returns the written byte count, and invalid offsets or insufficient capacity return `-1`.

**Tech Stack:** Existing Rill owner builtins, Rust integration tests, C11 differential execution.

**Spec:** `docs/blockers/standard-library.md`, remaining work item 2.

## Global constraints

- Owner functions remain acyclic and monomorphic.
- Use the explicit system allocator only in the caller.
- Input is a static UTF-8 string. Output is JSON bytes with no NUL terminator.
- Preserve existing workspace changes and do not push.

---

### Task 1: Implement and verify an owned JSON writer

**Files:**
- Create: `examples/owned/json_output.rill`, the reusable serializer module.
- Create: `examples/owned/json.rill`, a runnable caller.
- Create: `tests/owned_json_output.rs`, fixture and interpreter/C tests.
- Modify: `examples/owned/README.md`, `docs/blockers/standard-library.md`.

**Interfaces:**
- Consumes: `owned_bytes_len`, `owned_bytes_set`, `byte_len`, `byte_at`.
- Produces: `write_string(inout output, text, offset) -> i32`, returning bytes written or `-1` before writing anything.

- [x] Add fixture tests for empty/escaped/UTF-8 strings, all control bytes, exact capacity, invalid offsets, insufficient capacity, prefix/suffix preservation and repeated writes.
- [x] Run `cargo test --test owned_json_output` before implementation and confirm failure due to missing module.
- [x] Implement capacity preflight with `needed > remaining` before each subtraction. Reserve two quote bytes, then emit checked bytes into the owned destination.
- [x] Run fixture tests through the interpreter and C at `-O0` and `-O2`.
- [x] Add a runnable module caller and document its return convention, atomic capacity failure and static input restriction.
- [x] Run formatting, focused tests and strict Clippy. Review new files and report the remaining P3 limitations.

## Verification record

The initial focused test run failed because the module did not exist. After
implementation all three tests passed, comparing 18 fixture programs through
the interpreter and generated C at `-O0` and `-O2`, plus the CLI example.
`cargo fmt --all -- --check` and
`cargo clippy --test owned_json_output -- -D warnings` passed. Full-project
verification is handled by the coordinating agent. No compiler source changed.
