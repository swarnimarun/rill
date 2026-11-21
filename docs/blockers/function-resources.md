# Resource-capturing functions

Copy closures, higher-order calls and currying are implemented, but affine
closure captures remain outside the current checked owner subset.

## Evidence

`tests/function_inference.rs` verifies rejection of mutable captures, borrowed
parameters, implicit owner capture, an owner stored by partial application and
borrow-mode function values. `E-FUNCTION-CAPTURE`, `E-FUNCTION-MODE` and
`E-FUNCTION-OWNED` make those boundaries explicit. First-class functions cannot
select the copying interpreter fallback when a program contains owners.

The existing affine verifier models named functions, root/field loans and
structured cleanup. It does not yet carry a closure's call capability, retained
loans, owned captures and destruction obligations through indirect invocation.
Converting a callable into a record before proving those source obligations
would hide moves or drop unvalidated declarations.

## Next required work

Add explicit capture modes and source-level affine closure state. Verify each
lambda body independently, including unused declarations. Track captured loans
until invocation or destruction; distinguish repeated read-only invocation from
consuming invocation. Add cleanup transfer and failure traces for closure
construction, partial application, return, consuming call and scope exit.
Only then extend typed lowering and its independent verifier to owner-bearing
environments and remove the current diagnostics. Header checks and raw C
pointer compatibility do not prove these obligations.
