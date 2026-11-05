# String equality implementation plan

> **For agentic workers:** Use the delegate skill for compiler implementation,
> independent tests and search documentation, then review their combined result.

**Goal:** Make `str == str` and `str != str` compare byte contents and document
precise search-loop behavior without adding unverified abrupt exits.

**Architecture:** Extend the existing concrete equality obligation to static
strings. Keep ordinary type unification and the independent typed verifier;
emit a length-aware byte comparison for both C representations. Existing
interpreters already compare string values by contents after checking.

**Tech stack:** Rust compiler/interpreters, C11 byte loops, no new crate/header.

**Spec:** User request to improve the string-equality workaround and early-exit
advice; current [migration guide](../../rust-to-rill.md).

## Constraints

- Compare exact UTF-8 bytes, including embedded NUL; no pointer/strlen equality.
- Evaluate both operands exactly once in source order, without implicit allocation.
- Keep string ordering, unresolved generic equality, owner/buffer/function and
  aggregate equality rejected. Mixed operand types still reject normally.
- Preserve reference/typed interpreter and general/typed C agreement.
- Independently reject malformed typed mixed-string equality and string ordering.
- Keep `std.text.equal` available as an ordinary source-library alternative.
- Do not assume the user's missing Day 1 code or add break/return semantics.

## Task 1: Compiler support

Files: `src/check.rs`, typed verification/C emission and general C runtime;
compiler agent owns these files and related in-file verifier tests.

- [x] Admit only resolved `Type::StaticStr` in the equality obligation.
- [x] Verify matching string operands independently, retaining other restrictions.
- [x] Compare lengths and then bytes in typed and general C. Zero length must
  avoid dereferencing an absent buffer. Preserve operand snapshots/order.
- [x] Check current interpreter string behavior and reject mixed/ordered cases.
- [x] Improve rejected aggregate-equality guidance without suggesting a typeclass
  or deriving feature that does not exist.

## Task 2: Independent acceptance

File: `tests/string_equality.rs`, owned by a separate test agent.

- [x] Cover empty/equal/different strings, UTF-8, embedded NUL, prefixes and
  unequal lengths, variables, returned strings and aggregate-held string fields.
- [x] Check operand effects once in source order through mutable counters.
- [x] Exercise typed and reference evaluation and C at O0/O3.
- [x] Confirm mixed operands, aggregate equality and string ordering still reject.

## Task 3: Search documentation

Files: `docs/search-and-equality.md`, `examples/search/`,
`tests/search_example.rs`, main guide/catalogue links.

- [x] Provide a complete bounded first-match scan and independent edge cases.
- [x] Show that setting found changes the next loop condition, not control flow
  within the current body. Guard the suffix that must not run after a match.
- [x] Keep empty/missing/first/last/duplicate semantics explicit. State current
  static-string and supplied-input boundaries.
- [x] Record finite Copy aggregate equality as a separate proposal with leaf,
  recursion, ownership, generic-obligation and verifier acceptance decisions.

## Verification and commit

- [ ] Run focused equality/search tests, malformed verifier tests and existing
  boolean/primitive/native equality cases, then the full locked suite.
- [ ] Run formatting, strict Clippy, generated-doc links/search and inspect diffs.
- [ ] Commit the feature and document actual coverage/skips. No phase completion.
