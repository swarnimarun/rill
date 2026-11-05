# Standard library foundations implementation plan

> **For agentic workers:** Implement the independent source modules below, then review their contracts and differential tests before each local checkpoint.

**Goal:** Expand the six-module bundled library into a useful default collection for the implemented sequential language.

**Architecture:** Add ordinary checked Rill source, explicitly imported through the existing bundled catalogue. Keep canonical Result/allocation types unchanged. Separate optional generic Copy helpers from type-only definitions so monomorphic owner programs remain usable.

**Tech Stack:** Rill source, existing Rust module loader, interpreter and C11 backend; no new Rust dependency or native binding.

**Spec:** User request for sane default standard libraries; PLAN.md sections 11, 14, 15 and 19; stdlib/README.md; docs/blockers/owned-allocation.md.

## Global constraints

- Preserve existing std.result/std.alloc source and canonical identities.
- Use braced functions and semicolon statements for new source.
- No implicit imports, allocations, fabricated strings, unchecked foreign signatures or silently copied owners.
- All buffer failures must occur before writes and preserve every byte.
- Generic option/result operations are opt-in Copy helpers, not owner polymorphism.
- Explicit errors cover overflow, invalid ranges, invalid byte values and division by zero; unsupported OS/resource facilities stay documented as open work.
- Register modules only in src/modules/stdlib.rs; keep privacy, reserved namespace and loader budgets unchanged.

## Task 1: Numeric and range utilities

Create stdlib/int.rill, stdlib/range.rill, tests/stdlib_int.rs and tests/stdlib_range.rs.

- [x] Export i32 limits, min/max/signum, checked add/sub/mul/div/rem/neg/abs, saturating add/sub/mul and checked clamp. `Error = Overflow | DivisionByZero | InvalidBounds` travels through canonical result.Result.
- [x] Guard every arithmetic operation before it can trap, including MIN/-1 for division/remainder and MIN negation. Validate against a wider Rust oracle and explicit boundary examples.
- [x] Export half-open `Range = { start: i32; end: i32 }`, new/is_empty/contains, next/next_back returning option.Option<i32>, and len_checked returning result.Result<i32,int.Error>. Reversed ranges are empty; preserve written endpoints.
- [x] Run interpreter/C O0/O3 cases, extreme range endpoints and alternating front/back iteration without integer wrap.

## Task 2: ASCII and static text

Create stdlib/ascii.rill, stdlib/text.rill, tests/stdlib_ascii.rs and tests/stdlib_text.rs.

- [x] Export ASCII byte classification/case conversion, digit_value and hex_value. Classifiers reject out-of-domain integers; case conversion preserves nonletter values. Decoders return option.Option<i32>.
- [x] Export static str equal/compare/starts_with/ends_with/contains, find/rfind returning Option<i32>, ASCII trim returning `Span = { start: i32; end: i32 }`, valid_span and span_equal. Empty needle matches at 0 for find and byte_len(text) for rfind.
- [x] Compare bytes, including embedded NUL and UTF-8; do not manufacture substrings or imply Unicode case folding. Invalid caller spans compare false without indexing.
- [x] Exhaust all byte classifiers with independent expectations and compare text behavior against independent Rust byte operations at interpreter/C O0/O3.

## Task 3: Owned buffers and binary integers

Create stdlib/bytes.rill, stdlib/binary.rill, tests/stdlib_bytes.rs and tests/stdlib_binary.rs.

- [x] Export `Error = InvalidRange | InvalidByte`; valid_range(read buffer,offset,count), fill(inout buffer,offset,count,value), copy(inout destination,dst_offset,read source,src_offset,count), copy_within(inout buffer,src_offset,dst_offset,count), write_str(inout buffer,offset,text), equal(read left,read right), and find(read buffer,value). Writes return Result<i32,Error>; find returns Result<Option<i32>,Error>.
- [x] Validate negative/extreme ranges through subtraction after bounds checks. Validate byte values before writing. copy_within uses memmove semantics in both overlap directions; distinct-buffer copy keeps ordinary source loan exclusions.
- [x] Export read_i32_le/read_i32_be and write_i32_le/write_i32_be with offset arguments and bytes.Error. Encode signed two's complement in exactly four bytes, no alignment requirement. Preflight complete ranges before read/write; readers return Result<i32,Error>, writers Result<i32,Error> with count 4.
- [x] Test complete independent expected bytes, poisoned destinations and unchanged errors, overlap/same-range/empty operations, nonempty subslices, integer boundaries, endian order, repeated calls and owner cleanup through interpreter/C O0/O3.

## Task 4: Core types, optional generic helpers and integration

Create stdlib/option.rill, stdlib/option_ops.rill, stdlib/result_ops.rill, tests/stdlib_ops.rs and examples/stdlib/utilities.rill. Modify src/modules/stdlib.rs, stdlib/README.md and relevant user guides/status records.

```rill
// std.option is type-only, like std.result.
export type Option<a> = None | Some(a)
```

- [x] Add optional Option helpers is_some/is_none/unwrap_or/map/and_then/or_else and Result helpers is_ok/is_err/unwrap_or/map/map_err/and_then/or_else/ok/err. Use ordinary inference for callback types; retain lazy callback branches and eager ordinary argument evaluation.
- [x] Verify Copy generic callbacks and static closures at multiple concrete types; verify type-only Option can carry a live owner and reject generic helper imports in owner programs with the existing explicit diagnostic.
- [x] Register all nine new modules without changing existing modules. Test imports from a fresh directory, local shadow resistance, private helper rejection and deterministic bundled snapshots.
- [x] Add a runnable utilities example and document contracts, result/error handling, allocation behavior, byte/text differences and exact current limitations. Record runtime strings/containers and OS/resource blockers with prerequisites.
- [x] Run focused default/minimal suites, formatter, strict all-target lint and broader relevant existing library/module/ownership/backend regressions after source freeze. Audit generated HTML links after documentation changes.
- [x] Review each independent module set and commit verified changes locally in focused checkpoints. Do not push or mark language phases complete.

## Review decisions

- The repository is already an isolated JJ working copy on the v1 lineage. Preserve its ancestors and the user's signing workflow.
- Source-only library additions need no new unchecked C runtime or compiler builtin. Wide arithmetic or OS facilities that require missing language contracts are not approximated.
- Keep range and buffer operations monomorphic, allowing them to coexist with the current owner path. Import optional generic operations only in Copy programs.

## Checkpoint evidence

The feature checkpoint has 62 passing selected default tests and all 24 new
tests in minimal mode, both strict lint modes, formatting and an independent
review. See [the exact verification](../../verification/2026-11-04-stdlib-foundations.md).
The frozen-source aggregate run finishes with 1,489 reported passes and zero
failures. Four optional GPU hardware branches skip, as detailed in the separate
evidence checkpoint. No completed language phase is inferred.
