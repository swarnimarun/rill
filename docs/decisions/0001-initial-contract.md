# Initial implementation contracts

Status: provisional, 2026-11-02. These decisions record the bootstrap subset;
`PLAN.md` remains the proposed design for later phases.

## Arithmetic and evaluation

The first integer type is signed `i32`. Addition, subtraction, multiplication,
division and remainder trap on overflow; division and remainder also trap on
zero. Operands and function arguments evaluate from left to right. Boolean
`&&` and `||` short circuit. The evaluator and C output must agree in debug and
optimized C builds. There is no implicit wrapping or unchecked C signed overflow.

## Ownership and effects

The initial subset contains immutable values and no user-visible allocation,
mutation, references, resource cleanup, or external calls. It does not yet
implement the ownership, loan, and effect contracts in sections 6 and 7 of the
plan. Accepting a program in this subset is not evidence that those later
checks exist. Later phases must introduce these features together with their
static checks and cleanup semantics.

## Migration

The supplied repository contained only `PLAN.md`; no source compiler or tests
were available to compare or migrate. Existing Rill behavior and compatibility
cannot be inferred from the absence of those files. The current syntax is
experimental and does not constitute a compatibility promise. Importing an
earlier implementation requires an inventory and regression suite before
making migration claims.
