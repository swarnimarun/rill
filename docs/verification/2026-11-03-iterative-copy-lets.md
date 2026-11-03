# Iterative Copy-let scopes

Adversarial review of reachable generic specialization exposed an existing
host-stack limit. On a 2 MiB Rust thread, 32 active calls with 14 consecutive
Copy lets per function worked, but 15 overflowed before the source call-depth
guard. The independent AST oracle already overflowed at ten, so specialization
did not introduce that failure. The earlier eight-let regression was too small
to expose this remaining per-binding cost.

The typed evaluator now walks consecutive `Let` continuations iteratively.
Initializers still execute once, in source order, before their binding enters
scope. A scope stack removes introduced local IDs in reverse order on both
successful return and initializer/body failure. The shared helper works for
Copy-valued and owner-valued continuations. Actual `OwnerLet` evaluation, owner
transfer, runtime trap policy and the 32-active-call limit are unchanged.

## Evidence

The final affected run passed 149 tests: 103 library tests plus recursion guards,
generic specialization, owner optimization/borrows/bytes/enums/returns, JSON
validation and standard-library JSON. Strict all-target Clippy in default and
`c-import` modes, formatting and diff whitespace checks passed. A full combined-checkpoint suite
is required after integration; the preceding generic aggregate did not contain
this follow-on.

- An isolated subprocess starts an explicit 2 MiB Rust thread and checks 16/24
  Copy lets per function at the 32-call success and 33rd-call trap boundaries.
  Both recursive generic Copy functions and acyclic owner-returning chains
  exercise direct typed and production evaluation. A stack overflow cannot
  terminate the parent aggregate test harness.
- A private evaluator test verifies no leaked local bindings, retained outer
  scope, initializer order, shadowing and later/nested/body traps.
- A separate owner-chain unit test checks one allocation/one cleanup on normal
  return and one allocation/no cleanup on the call-depth abort path.
- Independent review exercised 11 ad-hoc AST/typed/production/C differentials for mutation,
  repeated-name shadowing, loop re-entry, branch/match scopes, byte snapshots,
  unit lets and traps. C ran at `-O0` and `-O2`.
- Independent 2 MiB probes passed scalar success/trap boundaries at 16, 24, 32
  and 48 lets and owner-return chains at 8, 16 and 32 lets. Source with 64 lets
  in the owner probe was rejected by the existing parser depth bound.
- Independent C owner traces placed 20 Copy lets before and after each function's
  scratch owner. A 31-function success chain returned `42`, freeing scratch
  values 130 through 100 in reverse followed by the original 42. A 32-function
  depth trap and an initializer division trap at function 15 freed no owners,
  preserving the abort policy at `-O0` and `-O2`. Typed/production execution on
  2 MiB matched return/trap outcomes; the separate unit above checks typed
  allocation/free counts.

The AST oracle is deliberately unchanged. Its nine/ten-let boundary and other
recursive compiler/evaluator paths remain documented in the
[stack-safety blocker](../blockers/stack-safety.md). This increment removes one
ordinary-block stack multiplier; it is not proof of stack safety for arbitrary
accepted programs and does not complete a PLAN phase.
