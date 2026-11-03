# Typed Copy enums and patterns: cloud checkpoint

This is a bounded P1/P2 foundation increment, not completion of either phase or
of the verified MIR required by PLAN.md. Work continues on the separate
`v1-ernie` continuation without rewriting the original `v1` checkpoint.

## Implemented contract

- Monomorphic functions over concrete finite Copy enums now lower to the same
  independently verified typed representation used by the interpreter and C.
  Generic enum types may have multiple concrete instances; generic function
  specialization is still a separate remaining task.
- Copy patterns include literals, unit, bindings, wildcards and nested nominal
  constructors. A match takes one scrutinee snapshot, checks arms in source
  order and selects the first match, including redundant arms. Constructor
  fields and side effects retain left-to-right evaluation.
- The verifier independently checks pattern types, nominal identity, tag/arity,
  binding identity and scope, arm result types and exhaustiveness. Its iterative
  pattern matrix has a 100,000-unit work budget and 128-level pattern limit.
  Resource exhaustion produces a diagnostic. Source parser limits still apply.
- C uses per-type tagged unions, with tag-only layouts for nullary enums and no
  empty C aggregates. Nested payload reads follow short-circuit tag tests.
  The ten-layer Pair example formerly rejected by uniform capacity inflation
  now runs at both optimization levels; eleven layers still exceed the unchanged
  conservative 1 MiB estimated frame budget. This is not a native stack proof.
- Primitive and nested Copy matches also work in restricted owner programs.
  Enclosing-owner states must agree across arms; borrowed owners cannot be moved,
  and affine enum destructuring retains its existing restrictions. Tests check
  exact cleanup order and no cleanup promise after aborting traps.
- `interpreter::run_reference` retains the independently source-checked AST
  evaluator as a Copy differential oracle and refuses owner programs.

## Verification

Host: Linux x86-64, Rust/Cargo 1.99.1, GCC 14.2.0 (`cc`), Clang 19.1.7,
SDL3 3.2.10. Tests use the existing toolchain environment and real SDL3.

A full default regression run exercised every target and found one new
migration regression: the nested JSON interpreter exhausted the 2 MiB Rust
thread stack before its 32-call trap. All other targets passed, including the
real SDL3 integration. The fix separates heavyweight builtin temporaries from
recursive typed evaluator frames. It does not raise the test stack or change
Rill's 32-active-call limit, argument evaluation order or trap semantics.

After that fix, this final affected-component command passed 132 tests:

```sh
cargo test --locked --lib \
  --test typed_copy_patterns --test typed_c \
  --test owner_adt --test owner_scopes \
  --test recursion_guard --test stack_budget \
  --test json_validator --test json_module --test json_result \
  --test fallible_allocation --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

The aggregate default/feature suites on the integrated continuation are a
separate final check; this record does not claim that the pre-fix full run was
clean. The new regression tests cover malformed IR, all 512 subsets of nine
boolean-product patterns against a four-value oracle, nested and redundant
patterns, shadowing, string NUL/UTF-8 bytes, zero-length buffers, snapshots,
constructor/scrutinee order, recursion boundaries, traps and compact layouts.
New C differential cases use strict C11 at `-O0` and `-O2`.

Independent review rebuilt the final evaluator and passed 43 source probes:
32 Copy cases compared with the AST oracle and 11 owner cases, each checked
against production interpretation and generated C at GCC `-O0`, `-O2` and
with undefined-behavior instrumentation. Fourteen private malformed-IR tests
also passed, including forged allocation metadata beneath CopyMatch and a
4096-unit pattern product rejected by the analysis budget. Eleven owner cases
checked exact allocation/free traces and failure injection with GCC at both
optimization levels plus UBSan and Clang at `-O2`. Clang UBSan was unavailable
because its compiler-rt sanitizer archive is absent; GCC UBSan actually ran.
An additional eight-local/31-call probe now succeeds on a 2 MiB thread stack,
while attempting the 33rd active Rill call still traps as specified.

## Remaining scope

Generic function instances, records and non-owner loans, foreign values and
vectors still have fallback paths. General initialization, partial moves,
drop flags, CFG loan inference, owned records, effects and allocator capabilities
remain open. No claim of phase completion, stable language/ABI, general stack
safety or completed ownership proof follows from this increment.
