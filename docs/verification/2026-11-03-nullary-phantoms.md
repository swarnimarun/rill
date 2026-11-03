# Bounded nullary-phantom verification

Date: 3 November 2026. Tested semantic source:
`24c1a519f9c2b13e34e4c25abc0ef67fb7ade387`.
This combines the nullary-phantom increment with accepted native-configuration
source `a3e005c5ec7cab51e04cb621e1270e8d041a83c6`.

The focused checks, independent acceptance and full default/importer-enabled
aggregate now pass. Final aggregate source:
`67f9bbad683b0d9fd5feb9236cf08d7074f65943`, which changes only documentation
after the semantic source above. This bounded increment is accepted; P1 is
not complete.
See the [representation contract](../typed-nullary-phantoms.md) for the exact
admitted family, logical identities, schema relationship and limits.

## Executed checks

All commands below passed on the tested semantic source:

- `cargo test --locked --lib typed:: --no-fail-fast`: 175 tests passed.
- The twelve focused integration targets below: 148 tests passed by default.
- The same typed library selection with `--features c-import`: 175 passed.
- The same integration selection with `--features c-import`: 160 passed.
- `cargo clippy --locked --all-targets -- -D warnings` and its
  `--features c-import` counterpart.
- `cargo check --locked --target x86_64-pc-windows-gnu --all-targets` and its
  `--features c-import` counterpart. These are compile checks, not Windows runs.
- `cargo fmt --all -- --check` and `git diff --check`.

The selected integration targets were:

```sh
cargo test --locked \
  --test typed_nullary_phantoms --test native_configuration \
  --test typed_foreign --test typed_foreign_raw \
  --test typed_source_origins --test typed_node_origins \
  --test typed_local_values --test typed_specialization \
  --test typed_copy_loans --test typed_field_loans \
  --test owner_adt --test value_restriction --no-fail-fast
```

The new `typed_nullary_phantoms` target passed all 19 groups in both modes.
The focused totals contain no ignored tests. Native configuration contributes
3 default tests and 15 importer-enabled tests. The totals above are not the
repository's full test counts.

A fresh exact-package build preceded the combined checks. Cargo reported both
library and executable artifacts as newly built, rather than reusing another
source checkpoint's artifact. Source-root bytes were unchanged by freshness
refresh. All execution used the existing Rust/C toolchain.

## Representation and behavior evidence

Executed source cases establish ordinary typed lowering and verification for:

- Unresolved direct/returned tags, aliases, constructor shadowing, conditional
  and match results, weak and mutable locals, loops and dynamic tags.
- Generic calls, repeated specialization requests, distinct weak slots and
  independent local classes in different source-function instances.
- Shared/exclusive root-loan forwarding, computed read temporaries, snapshots,
  once-only evaluation, left-to-right effects and the existing runtime traps.
- Nullary enums imported from distinct modules, retaining resolved nominal
  identity rather than treating equal-looking constructors as interchangeable.
- Nested open logical arguments returned through an existing generalized
  record shape. Its private capture stores a proved enum value; the open
  source record is never materialized as a runtime payload product.
- Existing closed nested phantom arguments and accepted `Phantom<Owned>`
  transport, without broadening source owner permissions.

For executable pure cases, the helper compared typed, ordinary and explicit
reference evaluation. Default C was byte-identical to typed C, contained neither
`rl_value` nor `RL_CAP`, and remained deterministic across repeated lowering.
Strict C11 builds at `-O0` and `-O2` agreed on values or exact runtime traps.
Adding an unrelated payload-bearing declaration no longer forces the nullary
family through the old uniform-layout rejection.

Finite recursive forwarding was executed at the existing 32-active-call limit,
including `main`. Separate divergent pure recursion cases only lowered,
verified and emitted C; they were not executed. Their logical instance keys
were explicitly checked to deduplicate rather than acquire fresh binders on
recursive forwarding.

## Independent structural checks

Internal tests corrupt canonical schema identity, arity, complete constructor
sets/order, logical atoms and nominal arguments, binder indexes/scopes/domains,
physical fields, ownership/dependency flags and borrowed-slot instances.
`verify`, `evaluate` and `emit_c` all reject malformed artifacts before use.
Opaque identities copied from a fresh compilation of identical source are
rejected; cloning the original artifact preserves the brand and generated C.

Native and owner dependency reconstruction includes nested logical arguments,
unused fields in their referenced schemas, raw pointee spelling and constness.
Known owner dependence cannot disappear under an erased argument. Existing
native, owner, local-value, loan and origin suites passed in the focused matrix.

The checked-source boundary separately compares canonical declarations and
scope origins with the actual checked source. A test deliberately changes an
unused schema consistently: structural verification can describe that different
program, while the source-relationship check rejects the alteration. Structural
verification is not presented as a proof of arbitrary lowering transformations.

Exact tests cover the 100,000-unit accounting boundary, exhausted-budget state,
128-level logical/declaration dependency bounds, cyclic malformed schemas,
instance deduplication work, and reservation before registry growth.
Review identified an omitted constructor-table cost: the final implementation
charges each complete retained variant table before cloning and charges every
argument/variant/field edge during logical verification. A tiny precharged
fixture proves rejection leaves registry identities and layouts unchanged.

## Boundaries that remain rejected

Source-accepted open `None | Some(a)`, fixed-payload `Mark<a> = Mark(i32)`, open
records, physical payload wrappers around symbolic enums, and bare unresolved
nonreturning results still return an explicit typed representation boundary.
Their source acceptance is not confused with C acceptance or typed success.
Weak/mutable polymorphism, incompatible nominal/raw-pointer uses, nonexhaustive
matches, aggregate equality and conflicting root loans remain source errors.

Both optional origin APIs reject opaque logical arguments explicitly after
ordinary success, including body-only and nested-shape cases. Semantic failures
retain precedence. Closed origin controls and hidden-native exclusions passed.

No new syntax, generic-owner permission, payload-erasure rule, ABI, allocation,
effect or owner-cleanup behavior is introduced. Fixed-layout phantom products
and their transitive field-substitution proof remain a subsequent increment.

The first combined strict Clippy run reported an explicit loop-counter style
lint; it was corrected before the final successful matrix. No behavioral test
failure occurred in these author runs. Broader phase acceptance still requires
its own full conformance evidence.

## Final independent and aggregate acceptance

Independent execution on clean `67f9bbad683b0d9fd5feb9236cf08d7074f65943`
passed 18 Rust/API/source-boundary probes and 40 strict C11 executions: ten
cases across GCC/Clang and O0/O2. Pure cases compared typed, ordinary and
reference evaluation; native cases preserved reached-only evaluator rejection
and used real C execution for outcomes. Generated typed/default C agreed and
remained deterministic. Runtime traps, nested symbolic forwarding, value/loan
ordering, hidden native dependencies and mapping boundaries were exercised.

The combined configured-native fixture passed ten CLI steps plus direct
program execution. Two native counter calls produced Off then On exactly once
and returned 42. Saved-profile replay emitted typed C; selected-constant drift
and changed binding bytes both rejected while preserving existing output, and
restoring the inputs restored success. A real 400-constructor/300-weak-instance
source hit the intended logical complexity diagnostic after source checking.

Initial independent fixture errors were retained: generic `>=` was correctly
tokenized when the fixture omitted the space before `=`, and the first budget
fixture exceeded the existing parser-depth limit. Correcting only those inputs
reached the intended checks. No compiler changes or ignored failures were used
to make the independent matrix pass.

The final matrix ran from 23:36 to 23:40 UTC on 3 November 2026:

- Formatting passed.
- Full default tests: 715 passed, zero failed or ignored.
- Full importer-enabled tests: 763 passed, zero failed or ignored.
- Both strict all-target Clippy configurations passed.
- Both Windows GNU all-target compile configurations passed.

Both complete test suites had no dependency skip notices. They used the same
locked commands as the native-configuration full matrix, including
`--no-fail-fast -- --nocapture`. Actual configured/direct/generated SDL3
execution is included, not new screenshots or physical-input/GPU validation.
Windows results are compile-only; Zig and AArch64 execution were not added.

A source-root timestamp refresh preserved exact bytes and forced fresh builds
from the final main checkout in each configuration. Source remained clean at
the same commit throughout. The next checkpoint changes only acceptance
and continuation documentation.
