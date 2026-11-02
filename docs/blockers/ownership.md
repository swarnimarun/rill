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
interpreter and C backend use checked field metadata and preserve sibling
updates when fields are borrowed. C field writeback must re-find the field in
the current root value because a sibling's variable-sized ADT payload can
change its offset. An exclusive field argument currently uses a copied
temporary and writes it back on normal return; C writeback can copy the entire
containing record. This relies on the current Copy-only record subset and does
not establish correct move or destructor behavior. `tests/field_loans.rs`
exercises one-field cases; `tests/nested_field_loans.rs` covers prefix
conflicts, nested forwarding and variable-size sibling writeback at multiple
levels, with interpreter/C agreement at `-O0` and `-O2`.

The checker still passes an AST and checked metadata to the execution paths;
this is not the verified MIR described in the plan. The current checker aborts
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
