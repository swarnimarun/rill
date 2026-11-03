# Reconstructed private rank-one solver

This is a bounded P1/C02 increment, not completion of C02, C03 or P1. Following
cloud filesystem loss, the implementation was reconstructed against recovery
commit `e0bc124`. It is new source, not recovered bytes from the earlier solver
commits. Earlier verification results are not evidence for this checkpoint.

## Contract and implementation

The public `check::Type`, `FunctionType`, `CheckedProgram` and expression-address
metadata contracts remain unchanged. Private equality terms are immutable and
interned. Each metavariable has a parent, rank, level, stable term handle and
optional binding. Ranked union uses a deterministic ID tie-break and retains
the minimum level. Representative lookup compresses paths iteratively.

Import, normalization, substitution, free-variable collection, unification,
occurs checking and level lowering use explicit worklists. Free-variable walks
visit shared terms once. Unification memoizes compared term pairs, preserving
left-to-right constraint order without exponentially unfolding shared DAGs.
An occurs check completes before lowering levels or binding its variable.

Local immutable initializers and recursive declaration groups are inferred one
level above the surrounding environment. Binding older variables lowers all
reachable free variables. Generalization independently compares level eligibility
with the existing environment-free-variable rule, resolving environment snapshots
to current roots first. An inconsistency returns an internal diagnostic rather
than silently changing generalization. Mutable bindings and pending raw-pointer
obligations remain monomorphic.

Quantified IDs are sorted, including after rank-induced representative changes.
All three private scheme membership filters use binary search. Completed groups
contribute only their monomorphic free variables to the outer environment, avoiding
a quadratic rescan of earlier generic signatures. Instantiation makes each use
fresh while preserving repeated-occurrence equality within one use.

## Fresh regression coverage

Nine private tests cover:

- 100,000 shallow unions and a deliberately constructed 100,000-parent chain.
- Direct/indirect infinite types and nominal identity/arity errors.
- Fixed buffer lengths and raw-pointer pointee/constness distinctions.
- Transitive level lowering and minimum levels on union.
- Fresh instantiation after a higher-ranked representative replaces an older ID.
- Two 120-layer shared binary DAGs, unified and lowered without materialization.
- 50,000-deep private operations on a 128 KiB thread stack, using an explicit
  iterative disposer for the public unary output tree.
- 2,000 deterministic generated constraint sequences against an independent
  recursive substitution-list oracle, including solved equality, normalization
  idempotence and alpha-equivalent resulting substitutions.

Nine source tests cover aliases connected to outer variables, mutable aliases,
independent local/function instantiations, repeated occurrences, infinite types,
nominal errors, all 120 declaration permutations of a five-function module with
an SCC, 2,000 independent generic functions, 4,000 shallow variable links, 20,000
independent parameters and pending raw-pointer obligations. The wide-parameter
case also exercises a later local let's environment filtering.

## Verification at the reconstructed checkpoint

Fresh checks used Linux x86_64, Rust/Cargo 1.99.1 and GCC 14.2.0 in an isolated
build directory. The restored optional toolchain environment was loaded.

- `cargo test --locked --lib check::tests`: 18 passed.
- `cargo test --locked --lib check::solver`: 9 passed.
- `cargo test --locked --test type_solver --test call_graph --test typed_specialization --test public_entrypoints`:
  9 source solver, 4 SCC, 5 typed-specialization and 4 public-entrypoint tests passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --all-targets --features c-import -- -D warnings`: passed.

- `cargo test --locked --features c-import --lib check::solver`: 9 passed.
- `cargo test --locked --features c-import --test type_solver`: 9 passed.
- `cargo fmt --all -- --check`: passed.

Independent differential review and full combined default/feature aggregates
remain pending. Actual SDL3 execution, Zig, Windows and AArch64 runtime
verification are not claimed by this focused solver checkpoint.

## Remaining scope

Distinct kinds, rigid quantified variables, universes, skolem-escape checks and
higher-rank source syntax are not implemented. Conservative value restriction
and typed local-scheme elaboration remain separate C03 work. This increment does
not claim to repair a demonstrated mutable-polymorphism soundness defect.

The private solver does not bound every compiler resource. Public type trees,
metadata materialization, diagnostic formatting, AST consumers and destructors
retain their own depth/size constraints; a shared DAG can still expand
exponentially when an existing consumer requests a full public tree. A separate
non-materializing cursor bridge for bounded source exhaustiveness is planned
but not included in this checkpoint. Tests establish implementation evidence,
not a principal-types or soundness proof.


The subsequent [source-proof cursor bridge](2026-11-03-solver-pattern-bridge.md)
provides lazy, per-lookup charged inspection of private terms. It does not change
the public-tree materialization limitation described above.

The subsequent [public-materialization budget](2026-11-03-type-materialization.md)
adds a fallible size/depth/cumulative policy for these expansion and checker-copy
paths, including the tiny generic-duplication failure. It does not establish
whole-compiler resource safety.
