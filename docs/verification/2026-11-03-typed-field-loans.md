# Checked Copy record field loans

This bounded P1/P2 increment moves call-scoped Copy field loans onto the same
independently verified representation as roots, records, enums and concrete
function instances. It follows the separately checked Copy-record checkpoint.
It changes no source syntax and completes no PLAN phase.

## Contract and representation

Call operands explicitly distinguish shared/exclusive roots, shared/exclusive
record fields, value snapshots and computed read temporaries. Parentheses do
not turn an actual field into a temporary. Each projected operand carries a
local identity and one to sixteen nominal record/field steps. The verifier
checks every step, the final parameter type, calling mode, scope, Copy ownership
and mutable authority independently of source-checker metadata.

A root overlaps all its descendants. Two field paths overlap exactly when one
is a prefix of the other. Shared paths can overlap; any overlapping exclusive
access is rejected. Later argument evaluation is checked while earlier loans
are active, including whole-root reads/writes, subrecord replacement, nested
calls and reborrowing. A nested call restores its enclosing loan intervals,
without restoring moved owners. Same-spelled shadows keep distinct identities.
These are call-scoped lexical intervals, not CFG liveness or inferred regions.

The interpreter retains the original internal cell plus a projection path.
Forwarding composes that path rather than copying a field into a detached slot.
No Rust `RefCell` borrow spans expression evaluation or a Rill call. Assignments
evaluate the right-hand side before updating the current original slot. Value
arguments, ordinary reads, local bindings, returns and computed read temporaries
remain snapshots. Each operand retains the source depth limit; composing valid
operands through several calls does not introduce an extra sixteen-step limit.

C passes typed addresses of fields in compact record structs. Ordered pointer
and value temporaries retain left-to-right argument evaluation. Field offsets
are stable even when a sibling enum changes active payload size. No `restrict`,
no-alias or stable foreign-ABI claim is added. Copy fields also work inside the
existing restricted owner programs, without weakening owner moves, cleanup,
allocation failure, call-depth traps or the abort-without-unwinding policy.

After a terminal trap, preceding writes through typed/native direct addresses
remain in their original storage. The legacy AST oracle may omit its copied
field's deferred writeback on that path. An independent internal-store probe
observed this difference. Both executions trap; the language has no source-level
trap recovery and promises no cleanup on that path. Successful values, store
updates, normal cleanup and observable trap outcomes still agree. This is not
a claim that the oracle's inaccessible post-trap internal store is identical.

Owned records, partial moves, uninitialized fields, escaping views, dynamic
index/variant loans and full MIR/CFG initialization/drop analysis remain open.
Generic/recursive owner and borrowed owner-enum restrictions remain unchanged.
Unresolved locally polymorphic record layouts still need separate elaboration.
Vector and foreign-value programs retain explicit checked fallback in the Copy
subset; owner programs never fall back to copying execution.

## Fresh evidence

- Six source groups cover 29 successful/trapping programs: 25 compare the
  checked AST oracle, direct typed evaluation, production interpretation and
  identical typed/production C. Four owner programs use explicit expected
  results because the AST oracle rejects owners. Native checks use strict C11
  with GCC at `-O0` and `-O2`.
- Cases cover disjoint siblings, repeated shared paths, nested forwarding,
  root/shared compatibility, later argument side effects, shadowing, loops,
  match bindings, source-order traps, enum sibling replacement, generic records,
  byte-buffer snapshots, UTF-8/NUL strings, unit/empty fields, subrecord
  assignment, the 32-active-call boundary, sixteen-step field-loan operands and
  composed twenty-step forwarding. Seventeen-step source operands are rejected
  by the parser; the independent IR verifier separately rejects overlong paths.
- Sixteen source rejection cases cover repeated exclusive/overlapping paths,
  root/prefix conflicts, later reads/writes and nested calls, immutable locals
  and shared-parameter authority. The earlier field-loan record/fallback tests
  now require direct typed execution, including the former owner/field case.
  Fallback tests use an actually unsupported vector operation instead.
- Four private malformed-IR groups make thirty mutations and require all three
  typed entrypoints to reject them. They cover scope, local/nominal identities,
  empty/overlong paths, field indices/types, modes, owners, mutability, prefix
  overlap, whole-root conflicts, nested interval restoration and shared
  parameter authority.
- Four further private groups check original-cell/path composition and Copy
  snapshots, handle release after argument/body/depth traps, exact owner cleanup
  and allocation-failure outcomes, and symmetric prefix overlap across all
  forty paths of length zero to three over three field indices.

Fresh affected checks passed 232 distinct tests across the library and 31
integration binaries: an initial 230-test run followed by all eight final
field-loan private groups and six final source groups, including the added
overlap property and single-operand depth groups.
This covers root/field loans, record construction, Copy patterns, specialization,
owner moves/cleanup, allocation, stack and recursion regressions. Strict
all-target Clippy passed in default and `c-import` modes. Formatting and diff
whitespace checks passed. Toolchain: Rust 1.99.1 and GCC 14.2 on x86_64 Linux.

Independent review found no blocking issue in its source and private-IR probes.
Its additional corpus covers 23 supported programs (19 Copy, including traps,
and four owner programs), sixteen source rejections and composed 32-step
forwarding. Strict C11 passes with GCC and Clang at `-O0`/`-O2`, plus GCC UBSan.
An exhaustive private matrix checks 196 mode/path pairs, with 115 legal and 81
rejected, alongside malformed metadata, nested intervals and depth checks.
Exact owner allocation/free/trap traces agree at all nine injected failure
positions. Seven added independent private tests also check live-store states
and handle lifetimes. Native instrumentation confirms argument-trap stores
`[1, 7]` and body/nested-trap stores `[7, 9]`, matching typed execution at both
compiler optimization levels and under GCC UBSan. After every tested call
outcome, retained root handle counts return to one and cells remain writable.
The frozen eight-file implementation manifest was independently rechecked.
Clang UBSan was not run because its compiler-rt runtime archive is unavailable.

The full combined-checkpoint aggregate is
separate integration work. Neither this evidence nor the earlier checkpoint
reports establish full P1/P2 completion. See [ownership](../blockers/ownership.md),
[C layout](../blockers/c-adt-layout.md) and [stack safety](../blockers/stack-safety.md).
