# Conservative immutable-let restriction

This is a bounded C03 increment, not completion of C03 or P1. The exact accepted
forms, snapshot distinction and migration examples are in
[ADR 0003](../decisions/0003-local-value-restriction.md).

## Implementation

This reconstruction starts at `2e7eaef2615f32853255c7aed4343d5047291292`.
It changes private checker logic and preserves the public `Type`, `FunctionType`
and `CheckedProgram` representations. Constructor classification uses validated
declaration identities; local names take precedence for value references.
Record construction uses checked nominal metadata.

The existing level/environment candidate comparison runs independently before
the value restriction. Restricted initializers receive no quantified variables,
and all free result variables are lowered to the ambient let level. A later
immutable alias cannot regeneralize those variables. Top-level function and SCC
polymorphism remain unchanged. Mutable storage, direct borrowed references,
active loans, raw-pointer obligations, moves and known owner dependencies retain
their conservative restrictions.

Owner dependency checking visits each reachable nominal declaration once and
scans its original field syntax, including inactive variants and phantom
arguments. It does not recursively instantiate those generic declarations into
exponentially large field trees. This does not solve the separate inherited
public-type materialization/expansion resource limit.

The adjacent private record projection helper now instantiates a generalized
immutable binding before selecting its field. Otherwise, restricting a projected
local could lower or bind the original record scheme's quantified variable.
Mutable and borrowed bindings have empty schemes and preserve their root type.
This does not add typed local record specialization or alter evaluation order.

## Fresh focused evidence

All evidence below was obtained from the reconstructed files. Results from the
unavailable earlier workspace are not counted. The environment was Linux
x86_64 with Rust/Cargo 1.99.1 and GCC 14.2.0, using the restored optional native
toolchain environment and an isolated Cargo target directory.

The 17 `tests/value_restriction.rs` tests cover:

- Independent nullary constructor and immutable-alias instances, nested eligible
  constructor values and generic record literals.
- One-type call/conditional/match results, nested sequencing/loops/mutation,
  binary child expressions, alias chains and constructor-wrapped weak variables.
- Outer variables, mutable slots, read/inout parameters, field reads and active
  shared loans. Shadowed locals are distinguished from active loan roots.
- Immutable copies of borrowed/mutable values: old variables stay monomorphic,
  while independent fresh phantom parameters may generalize later.
- Independent whole-record/field instances, mutable and borrowed record roots,
  nominal identity and source field evaluation order.
- Actual constructor resolution under local shadowing, capitalized user
  functions, qualified imports and distinct imported nominal identities.
- Top-level identity/result schemes and mutually recursive generic groups.
- Raw pointers, allocation/move restrictions, inactive owner variants and owners
  present only through nested phantom arguments.
- An 80-layer generic declaration graph whose ownership scan must not expand
  substituted field trees.
- Concrete migration programs compared through checked AST interpretation,
  typed evaluation where supported, production execution and C11 at `-O0` and
  `-O2`. Constructor arguments produce `12` and reordered record fields produce
  `21`, preserving source order. Accepted polymorphic locals retain explicit
  typed/C specialization diagnostics.

Commands and results:

- `cargo test --locked --test value_restriction --test type_solver --test call_graph --test typed_specialization --no-fail-fast`: 35 passed.
- `cargo test --locked --lib check::`: 36 passed, 83 unrelated tests filtered.
- `cargo test --locked --test records --test record_audit --test field_loans --test nested_field_loans --test loan_scopes --test read_audit --test read_temporaries --test inout_audit --test owner_scopes --test owner_borrows --test owner_adt --test returned_owner --test specialization --test ffi --test owner_borrow_checker --test owned_bytes_checker --no-fail-fast`: 56 passed.
- The first 35-test command repeated with `--features c-import`: 35 passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --all-targets --features c-import -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

These are 127 default-feature focused tests plus 35 repeated with `c-import`,
not a full aggregate. Independent runtime/differential review and the combined
integration aggregate remain pending at this author checkpoint. SDL3 native
execution, Windows, AArch64 NEON and Zig were not exercised by this increment.

## Remaining gates

Typed local specialization, kinds, rigid-variable escape checks, closure capture
and effect conditions, complete checked MIR coverage and the full P1 gate remain
open. The conservative rule is intentional experimental-language policy, not a
claim that a mutable-polymorphism soundness exploit was demonstrated or fixed.
