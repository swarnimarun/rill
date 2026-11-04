# Discarded results: frozen author evidence

Status: **author-verified candidate, not integrated acceptance**. This closes
one bounded representation slice, not P1 or another PLAN phase. Independent
execution, the full default/importer suites and Windows GNU compile checks are
still required. The accepted baseline remains K3b until those gates pass.

## Exact source and environment

- Base: `d78e5d1e6b811f79c7cdfd814251122648920931`.
- Tested production/test source: `ee5763f16686cf5a67decc80dd78979fa8d3ec80`.
- Tested tree: `3f9589dd1e8d62d0765e0332be34c68504ec8ac0`.
- Linux x86-64; Rust/Cargo 1.99.1; GCC 14.2.0; Clang 19.1.7.
- Two Cargo build jobs, debug information disabled, incremental compilation off.
- The final package build explicitly touched only root-source timestamps after
  hashing their bytes, then rechecked unchanged hashes. Cargo JSON records
  `fresh: false` for the actual package library, binary, lib-test executable and
  every selected integration-test executable. The retained artifact ledger has
  their full compile paths, sizes and SHA-256 hashes.
- Work and artifacts stayed in the authorized RAM repository/target. No remote
  writes, cache cleanup, toolchain install, or original-checkout edits occurred.

## Fresh author results

The exact-source GCC command selected the library and six focused integration
suites, with `--locked --no-fail-fast`:

| Cohort | Passed | Failed / ignored / skipped |
|---|---:|---:|
| Library, including 11 new proof/execution tests | 418 | 0 / 0 / 0 |
| New discarded-result public suite | 9 | 0 / 0 / 0 |
| Interrupted eager arguments | 8 | 0 / 0 / 0 |
| Local values | 17 | 0 / 0 / 0 |
| Nonreturning results | 10 | 0 / 0 / 0 |
| Nullary phantoms | 20 | 0 / 0 / 0 |
| Phantom products | 22 | 0 / 0 / 0 |
| **Focused GCC total** | **504** | **0 / 0 / 0** |

The new public suite additionally passed **9/9 with Clang**. Its 31 C fixtures
per compiler each compiled and executed at O0 and O2: **124 actual C executions**
across GCC and Clang, using `-std=c11 -pedantic-errors`. Final new-suite compiler
logs contain no warnings or errors. This count excludes C executions from the
older focused suites. Formatting and default strict all-target Clippy passed.
This is not a claim that the full repository matrix has run.

The public suite explicitly lowers/verifies/evaluates the typed result, compares
reference/default interpreter outcomes, checks default C equals typed C, and
rejects uniform `rl_value`/`RL_CAP` representations. It includes source-main
normal enter/leave traces, exact 32-frame depth boundaries, constructor argument
events and first-versus-later trap order. The interpreter tests retain the exact
mutable source-main slot across the constructor trap.

## Scope and proof evidence

See the [bounded contract](../blockers/discarded-results.md). Enum results of
syntactically unused immutable bindings have full logical identities but no
physical aggregate or local slot. Real calls return normally with no Rill value.
Value/read/inout parameters retain existing represented storage/loan rules;
branches and represented-input matches preserve their source operations.

Demand is part of exact function-instance identity. The frozen relation checks
unused lexical identities, constructor schemas, complete logical results,
source children, argument modes and exact call targets. Independently rebuilt
IR flow does not infer nonreturn from a discard tag. Original-source equality
remains separate from consistency with retained source evidence.

Author mutations cover result/brand/occurrence fraud, wrong constructor
identity/arity, cross-demand callees, missing evidence, excessive logical depth,
inactive owner/native fields in internal-only and source-only-suffix schemas,
hidden-use recount and exact/one-short frozen work reservations. A consistent
IR/witness replacement is rejected by the original-source comparison.

## Development failures and corrections

All development logs and failed fixtures were retained; no failed test was
silently skipped. The first compile/harness iterations contained ordinary Rust
integration errors, a fixture-only private-field access, generic-close/equals
spacing mistakes, and an overbroad C unused-helper warning policy. Strict Clippy
then found an oversized helper signature and a single-element test loop; both
were corrected rather than suppressed.

The broader source regression pass required explicit migration of the newly
supported unused `make()`, dead conditional and conservative recursive-return
fixtures. It also found a real metadata detector gap when no function signature
or physical ADT mentioned a logical-only discard. The final detector includes
proof-bound body/suffix metadata. Original local-shape and observed-main
negatives remain negative.

Independent source review identified a further source-only suffix purity gap.
A paired inactive-schema mutation was demonstrated to pass the first standalone
verifier, then rejected after adding independent full retained-source purity
reconstruction. Its red/green regression is retained. Earlier Clang style
warnings from the new match emitter were eliminated; final evidence uses the
corrected source, not the earlier warning-bearing runs.

## Remaining gates and limits

Independent source review has been active, but its private/public executable
probes are **not counted here**. Full default/importer tests, both strict lint
configurations and both Windows GNU compile checks belong to final integration.
Normally returning unknown values still cannot be observed, stored, passed as
unknown arguments, aliased or used as mutable layoutless locals. Direct
layoutless records, local-shape mixing, unknown nominal interruption results,
and new owner/native combinations remain excluded. No SDL/hardware execution
claim is made by this kernel slice.
