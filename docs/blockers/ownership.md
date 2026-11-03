# Ownership and checked call loans

The compiler supports initialized mutable locals, nominal records with named
fields, and call-scoped `read` and `inout` loans of a local root or a record
field path up to 16 segments. Ordinary records and buffers use the Copy
subset. A restricted hosted integer box and fixed-length owned byte buffer
support explicit moves, finite owner-bearing enums and scoped cleanup; see [owned allocation](owned-allocation.md). General non-Copy values,
escaping references and control-flow loan analysis are not implemented.
`PLAN.md` sections 7 and 19 remain the contract and phase gates; this subset does not complete P2.

## Current calls and places

A `(read x)` parameter borrows a local, parameter or record field path for
shared access through the call. A literal, nullary constructor or computed
expression is evaluated into a temporary at its argument position; that
storage remains alive until the call returns. Parenthesizing a local or field
does not turn it into a temporary. Its reads and writes are checked against
loans established by earlier arguments. A read parameter cannot be assigned
or forwarded as `inout`, but it can be copied or forwarded as `read`.

An `(inout x)` argument uses explicit `inout local` or `inout local.field`
and requires initialized mutable storage. An existing `inout` parameter can be
reborrowed. A `read` parameter cannot be borrowed exclusively. Nominal
parameter annotations such as `(inout p: Point)` constrain record field access;
the checker rejects an unresolved field owner instead of guessing from labels.
Record construction evaluates field values in source order, even if fields are
written in a different order from their declaration.

Arguments evaluate left to right and each loan lasts until its call returns.
A root overlaps every descendant, and a field overlaps its descendants. Two
field paths are disjoint when neither is a prefix of the other. Shared loans can overlap other shared loans, but
an exclusive loan or write conflicts with an overlapping active loan. Thus
`f(inout p.x, inout p.y)` may be valid, while `f(inout p.x, inout p)` and
`f(p.x, inout p.x)` conflict when the first parameter is `read`. A value
argument evaluated before a loan is an independent copy. Other later argument
expressions are checked against the live loans, including nested calls,
assignment and shadowing.

On a trap the process terminates, so no borrow is resumed or cleanup promised.
Existing scalar C externs cannot receive an address of a Rill local. If that
changes, their effects and retention contracts must be checked before these
rules are extended to such calls.

## Current implementation and evidence

The checker assigns a distinct identity to each parameter, local and pattern
binding, and tracks root and field-index paths during call argument analysis.
Same-spelled shadows have distinct identities. Two paths with the same root
conflict when either is a prefix of the other and at least one is exclusive. The
checked AST oracle and fallback backend preserve sibling updates by copying
projected temporaries and writing them back on normal return. That legacy C
writeback re-finds the field because variable-sized sibling payloads can change
its uniform-layout offset. It does not establish owner move/drop behavior.
`tests/field_loans.rs` and `tests/nested_field_loans.rs` retain source-level
prefix, forwarding and sibling-update cases at `-O0` and `-O2`.

Supported scalar, buffer, vector, finite-enum and Copy-record instances now
share an independently checked typed path, including primitive/nested Copy
matches and root loans inside owner programs. Typed call operands distinguish
shared/exclusive roots, shared/exclusive fields and computed read temporaries.
The verifier checks loans through later arguments and nested calls, allowing repeated shared
reads and rejecting overlapping exclusive access independently of source
checking. Nested calls restore the enclosing loan state. Interpreter cells
are shared only for explicit loans; ordinary values and returns are snapshots.
C uses const/non-const typed pointers without `restrict` claims. Record
construction evaluates fields in written order; projections use independently
checked nominal paths of at most 16 steps. Field assignment resolves the path
in the original root after its right-hand side, preserving replaced siblings
and prefixes. Rust cell borrows never span evaluation or Rill calls. Direct
field call operands retain actual-place identity rather than becoming computed
temporaries. Their checked intervals use prefix overlap; disjoint siblings and
repeated shared paths remain legal. Projected interpreter handles retain the
original cell and compose paths when forwarded. Each operand retains the
16-step limit; composing valid operands adds no new source limit. C passes the
actual compact struct field address. This also works inside restricted owner
programs. Owned records, partial moves and escaping views remain open.
See [root-loan evidence](../verification/2026-11-03-typed-copy-loans.md),
[Copy-record evidence](../verification/2026-11-03-typed-copy-records.md), and
[field-loan evidence](../verification/2026-11-03-typed-field-loans.md).
This is not the complete verified MIR described
in the plan. The current checker aborts
analysis on its first error. Its temporary loan snapshot is not restored on
every error exit before returning. If error recovery is added, restore the
snapshot before continuing so one error cannot create false conflicts later.

## Remaining work

- Introduce a resolved-place representation shared by checking and lowering,
  with stable field identities and explicit types rather than requiring each
  consumer to recover them from syntax. Keep place identity internal.
- Add projections into variants, indices, slices and dereferences only with
  their own validity and overlap rules. A dynamic index
  is not automatically disjoint from another index. Existing record-field path
  checks do not establish those rules.
- Extend equal-state ownership joins to path-dependent initialization and
  cleanup using drop flags. Add partial moves and destructor effects. Current
  locals are initialized at declaration and cannot be partially moved.
- Add provenance, CFG liveness and escape checking before allowing loans to
  persist beyond calls. Test branches, loops, returns, failure and future
  cancellation paths; do not infer safety from source spelling.
- Extend tests for all these paths, including different roots and shadows,
  loan conflicts under later argument side effects, variable-sized field
  replacement, and diagnostics that identify the original and conflicting
  places. Continue requiring checker rejection before code generation and
  comparing both execution paths.
