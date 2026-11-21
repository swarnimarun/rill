# Progress and preservation for closed conditional and Option expressions

The [Lean model](../proofs/OptionCore.lean) defines raw terms, a typing relation, values and one-step evaluation independently. A separate type-indexed tree erases to those raw terms. The fragment has arbitrary finite nesting, signed 32-bit literals represented by their bit patterns, booleans, conditional expressions and one nominal Option constructor family. The only binder occurs in the identity arm of the exhaustive match shown below. There are no free variables.

| Core form | Rill surface expression | Typing premise and result |
| --- | --- | --- |
| `int n`, `bool b` | An i32 or boolean literal | `i32` or `bool`, respectively |
| `none` | `None` | `Option<A>` for any ground type A |
| `some e` | `Some(e)` | `Option<A>` when e has type A |
| `branch c y n` | `if c { y } else { n }` | Both arms have one type A, and c has type bool. Result A. |
| `getD s d` | `match s { Some(x) => x \| None => d }` | s has type `Option<A>` and d has type A. Result A. |

The Option declaration is `type Option<a> = None | Some(a)`. Ground types comprise `i32`, `bool` and finite nests of Option over them. Erasure maps each `Checked` constructor to its corresponding raw `Tm` constructor recursively, dropping the indices. In particular, every checked `none` erases to the same raw `none`. Erasure introduces no type-driven runtime branch.

Evaluation reduces a conditional's condition before selecting its arm. Constructing `some` evaluates its argument to a value. Option elimination reduces its scrutinee, returns the payload of `some value`, or selects the fallback on `none`. Literals, `none`, and `some` of a value are terminal.

## Theorem scope

* `erasureTyped` proves that erasing a checked tree produces a term of its indexed type.
* `valueNormal` proves that values cannot step.
* `progress` proves that any well-typed raw term is a value or has a successor.
* `preservation` proves that **every** single step from a well-typed term has the same type.
* `reachableTyping` and `reachableProgress` extend these results to arbitrary finite reduction traces.

Each theorem quantifies over all terms in this fragment. The model omits arithmetic, variables and general substitution, let generalization, arbitrary pattern bodies, recursion, stores, loans, owners, FFI, allocation and traps. The Lean kernel checks properties of the specified calculus. Connecting Rust inference and typed IR to those judgments for every accepted Rill program remains a separate refinement obligation.

## Reproduce

```sh
lean proofs/OptionCore.lean
cargo test --locked --test option_core_proof
```

Use Lean 3.51.0. On Nix, run `nix-shell -p lean --run 'lean proofs/OptionCore.lean'`. The proof uses the bundled core. Lean 3.51.0, commit `9fc1dee97a72`, reported `no axioms` for all five audited theorems and exited successfully with zero diagnostics. The [Rust regressions](../tests/option_core_proof.rs) instantiate both parametric Lean traces at payload 42 and fallback 7. Both parse and type-check through the public compiler, give 42 and 7 respectively in the reference and checked interpreters, and run as generated C11 at O0 and O3. Four counterexamples test the typing premises through check, interpretation and C emission.
