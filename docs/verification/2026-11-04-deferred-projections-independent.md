# Deferred nominal projections: independent verification

4 November 2026. Independent review and selected execution pass on clean,
committed source `e2ec991cd5dd5c51833b60d131e224ec8752238a`, tree
`bcbc2a33ac59128d593e2f402877971ca7b2e3e2`. This adds tests to the clean author
handoff `4131128cc380b77ac307293c8e4e4d28ece5b321`; no production code changed
during independent review. The following documentation checkpoint changes no
executable or test inputs. The final full aggregate remains pending. This does
not complete P1, P2, C02 or the full language plan.

See the [bounded contract](../deferred-projections.md) and the historical
[author report](2026-11-04-deferred-projections-author.md).

## Review outcome

No confirmed production defect was found in the final author implementation.
The review examined deferred equality discharge, canonical metadata publication,
local and SCC generalization, symbolic loans, one-time read instantiation,
resource admission, and the unchanged independent typed/source-origin verifiers.
The retained tests supplement the author fixtures rather than replacing them.

The accepted scope remains nominal inference within one monomorphic function
component. No label-based structural search is introduced. An unresolved
standalone component rejects before a later caller can constrain it. Ready
projections drain before free-variable and environment snapshots are collected;
the drain does not permanently weaken otherwise eligible local or SCC variables.
Partially resolved paths retain their original type instance and canonical prefix.

Symbolic loan overlap is justified by binding identity and full field segments.
Exclusive roots must have empty schemes and one mutable monotype, fixing the
nominal name/index mapping at every accepted prefix. Quantified roots remain
immutable and unborrowed, permit only shared reads, and use the existing typed
read-temporary path. Root operations overlap every path. The production invariant
comment explicitly requires reconsidering this argument before relaxing those
scheme or authority restrictions.

The new budgets are deterministic policy bounds, not a complete compiler
resource bound. Review checked admission before retained clones, charged schema
and substitution names, bounded retries and symbolic comparisons, and independent
solver limits. Cumulative admission counters and eager field-path charging are
documented, including their possible new complexity rejection.

## Independently retained tests

`tests/independent_deferred_projections.rs` adds three integration tests covering
15 positive fixture instances and seven negative instances:

- All six orders of a three-function generic SCC retain two independent
  quantified parameters and exact canonical result schemes.
- All six orders of a second generic SCC traverse an already-known outer record
  into an unresolved intermediate nominal path, preserving exact final schemes.
- Both orders of a projection alias in `Pair(alias,None)` retain an independently
  generalized second payload without losing normal SCC polymorphism.
- A pattern binding shadows an outer root while its deferred exclusive field
  loan is active. A different binding with the same spelling remains independent.
- Occurs-check failures, a nominal same-label mismatch, an unresolved component
  followed by a constraining caller, and alias-based value-restriction violations
  reject through all six selected public consumers.

Every positive fixture checks analysis, canonical schemes where applicable,
typed lowering and verification, all three evaluators, and production C equality
with direct typed C emission. All return 42. Thirteen fixtures also use uniform
reference C; two retain its exact inherited local-shape rejection.

Six private tests in `src/check/projections.rs` independently check:

- Quantified roots cannot acquire mutable or borrowed authority.
- A read operand creates exactly one scheme instance and one obligation; its
  original quantified template stays unbound and loan state is restored.
- A retained canonical prefix is not traversed again. Resolving the remaining
  step of a two-field path consumes exactly 16 work units and publishes the
  correct indices.
- One hundred unchanged drain checkpoints spend no additional retry work; an
  ordinary unification invalidates readiness and permits discharge.
- Obligation/name admission failure creates no result variable, pending entry
  or occurrence metadata.
- Selected-schema preflight succeeds at the exact 11-unit boundary and fails
  with 10 units.

These are `cfg(test)` additions only. Existing production typed record, loan,
source-origin and representation verifiers were not relaxed.

## Clean committed-source execution

All counted stages started with the exact clean commit above. Its full tracked
source manifest was captured before execution and verified unchanged afterward.
The selected stages passed between 14:21:57 and 14:32:56 UTC:

- The full library and focused author suites passed **549 tests**: 516 library,
  8 deferred semantics, 7 deferred origins, 7 recursive signatures, 10 result
  annotations, and 1 author C harness.
- The new independent integration suite passed **3 tests**.
- The same twenty affected suites listed in the author report passed **142 tests**.
- Default `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and `git diff --check` passed.

The selected total is **694 unique Rust tests**. The preliminary six private
tests are already included in the library count, and the two Clang harness
reruns are not added. There were zero failures, ignored tests or SKIP notices.
This is not a full-suite or full-platform aggregate count.

The independent 15-fixture matrix completed **112 C11 executions**, 56 GCC and
56 Clang, at O0/O2: 60 production/typed and 52 uniform-reference executions.
It produced no compiler warnings.

The complete author 45-fixture matrix was replayed on the same clean source:
**340 C11 executions**, 170 GCC and 170 Clang, at O0/O2. These replace the
earlier author matrix for clean-source evidence; they are not added to its
historical 340. All 42 return fixtures and three trap fixtures were preserved,
including actual recursive back-edges returning at 31/32 active calls and
trapping at 33. Eight existing Clang `-Wparentheses-equality` warnings remain
visible in the author-matrix replay. They were not suppressed. Other selected
stages report no warnings.

The author originally ran two final test edits before committing them as
`02d5aa5bcefe8bbf0eeeabafbf7c4fe3e9eb0594`. Later input hashes match that
subsequently committed content, but a complete dirty-tree fingerprint was not
captured before those early runs. The clean committed-source replay above
removes that evidence limitation for the complete author C matrix and selected
Rust suites. It does not relabel the historical at-run HEAD.

## Reference-C boundary

The independent selective-Pair cases and the author's five local-shape cases
assert the exact diagnostic:

`E-C-ADT: unresolved polymorphic value requires local specialization`

All these fixtures still pass typed verification, all three evaluators and real
production C execution with both compilers and optimization levels. None is
silently omitted. Static review of the immutable accepted `81b329ff` layout
analyzer and typed-local-value contract agrees with the author's retained fresh
baseline reproduction: uniform reference C does not specialize local schemes.
That baseline probe checked C emission, not C execution. No baseline source was
built into the current target generation during independent review.

## Guards, evidence and remaining gate

Separate control-flow probes exercised the new independent harness with injected
guard rejection. First-admission denial exited 86 after one guard and zero
compiler-spy or executable calls. Pre-execution denial exited 86 after two guards,
one compiler-spy call and zero executable calls. Neither reached a later shell
marker. The unchanged real resource guard ran during both probes. These are not
real C compilations/executions and are excluded from the matrix counts.

All execution used the frozen Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7
toolchain, normal Rust flags, two Cargo jobs, zero dev/test debug information,
disabled incremental compilation, serial tests, and the current-checkout-only
target generation 2. Fixed 2 GiB disk and 3 GiB MemAvailable floors passed.
New C compilation and execution each have a guard and a subprocess deadline.
No actual resource floor failed. All artifacts and earlier evidence were retained.

Evidence directory `deferred-projections-independent-drafts-20261104T140645Z`
retains the original unexecuted drafts separately from executed stage logs
01–09, `selected-summary.json`, source hashes, the committed test patch,
timestamped C artifacts and fail-fast probe records. The directory name records
when drafting began; the original draft disclaimer is not an execution claim.

At this selected-review checkpoint, the final default/importer/lint/Windows/SDL
aggregate and publication remained separate acceptance and release steps. See
the [subsequent integrated acceptance](2026-11-04-deferred-projections.md).
No broader layout, owner/native, GPU, physical-input, Windows execution or
completed-phase claim follows from this selected independent pass.
