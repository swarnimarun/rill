# Borrowed owner parameters

The restricted hosted `SystemBoxI32` and fixed-length `SystemBytes` can be
passed to an acyclic monomorphic function by value with `move`, by shared
`read`, or by exclusive `inout`. A
borrowed argument must be a live owner local or owner parameter; owned
expressions and allocation temporaries cannot be passed directly as borrowed
arguments. The type is inferred from operations such as `box_read` and
`box_write`. See [the runnable example](../../examples/owned/borrow.rill).

A borrowed parameter aliases the caller's allocation and does not own its drop
obligation. Normal return ends the loan and leaves cleanup with the caller.
Shared parameters permit reads and shared reborrows; exclusive parameters
permit reads, checked writes, and shorter shared or exclusive reborrows.
Neither mode can be moved, returned, stored, passed to FFI, or dropped by the
callee. Owned locals and value parameters may return directly or inside a
finite enum, and consuming enum matches transfer owned payloads into arm
bindings. Borrowed enum parameters and matching, recursive owner call graphs,
generic functions, foreign declarations, vectors and escaping loans remain
unsupported. Borrowed non-owner user-function parameters are also unsupported
in owner programs.

Arguments evaluate left to right. A root loan begins at its argument and ends
when the call returns. Multiple shared loans may overlap; an exclusive loan
conflicts with any overlapping live loan or access. Thus `f(box, box)` may pass
two shared parameters, while `f(box, inout box)` conflicts if the first
parameter is `read`. A later read also conflicts with an earlier exclusive
loan. A scalar copied from a box before a loan is independent. Reborrows end
before control returns to the outer borrower. A trap terminates generated code
and has no Rill cleanup guarantee, as in the existing owner subset.

## Implementation and evidence

The owner-aware typed representation distinguishes value, move, shared-owner
and exclusive-owner call arguments. Its verifier tracks live owners and loans,
rejects moving or cleaning a borrowed parameter, and checks overlapping call
arguments. The interpreter refers to one allocation identity across frames;
the C backend passes an appropriate pointer and does not free borrowed
parameters. Both use the checked typed path for owner programs. This remains a
restricted implementation, not the verified MIR or complete borrow system in
`PLAN.md`.

`tests/owner_borrows.rs` checks repeated shared aliases, exclusive forwarding,
shorter shared reborrows, distinct and shadowed owners, use after a borrow ends,
conflicting later arguments and forbidden moves, returns and writes. It compares
interpreter and C at `-O0` and `-O2` and intercepts allocation/free to check
that borrowers do not create extra cleanup obligations. Typed verifier tests
also reject forged overlapping-loan and cleanup cases.

## Remaining work

Owned record fields, user-defined destruction and borrowed temporaries need
their own move, lifetime, failure and cleanup rules.
Long-lived loans, partial field moves, destructor effects and
control-flow lifetime inference remain P2/P3 work. Allocator authority and
`alloc`/`free` effects remain distinct from borrowing. Recoverable system
allocation is available through `std.alloc`, but the subset does not yet
provide allocator capabilities or an effect system.
