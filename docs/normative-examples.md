# Sequential kernel examples

These examples document the provisional behavior in
[decision 0001](decisions/0001-initial-contract.md). They are exercised from
source by `tests/normative_examples.rs` in the evaluator and generated C at
`-O0` and `-O2`.

- [arithmetic.rill](../examples/contracts/arithmetic.rill) evaluates to `42`.
  Signed division truncates toward zero, and the remainder has the dividend's
  sign.
- [short_circuit.rill](../examples/contracts/short_circuit.rill) evaluates to
  `42`; the divisions by zero are never evaluated.
- [boundaries.rill](../examples/contracts/boundaries.rill) evaluates to `42`
  using both `i32` endpoints.
- [overflow.rill](../examples/contracts/overflow.rill) traps because the sum
  is outside the `i32` range.

These examples cover only the current sequential kernel. They do not specify
ownership, effects, or the behavior of an earlier Rill implementation.
