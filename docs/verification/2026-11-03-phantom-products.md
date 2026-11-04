# Finite Copy phantom products: integrated verification

## Source and status

The production candidate is `04b88ab8007c035a337a3ea4d531eeee3098b3ed`, based on
accepted K1 source `67f9bbad` and its acceptance record `706a29a`. It includes the
separately reviewed deterministic recursive-declaration diagnostic correction
`cbe8494`; that correction does not change type acceptance or product layout.
The [K2 contract](../typed-phantom-products.md) describes the representation
boundary. Focused, independent and full aggregate acceptance now pass on
`6e337dd8dd2075db282bad4c2c155294a60f97f3`. Its additional changes after the
author candidate are documentation and independently authored test-only cases.
This bounded acceptance is not P1/P2 completion or a stable ABI claim.

No source inference, value restriction, ownership, native ABI or effect rule
was relaxed for product admission. The source checker's concrete `adt_variants`
contract is unchanged. Opaque identities retain their compilation domain and
scope. The new verifier reconstructs every canonical field of every variant,
including complete closed physical descendants, rather than trusting builder
parameter-demand summaries or matching only nominal names.

## Executed author checks

Checks used the existing isolated Rust toolchain and one serial target, with
incremental compilation disabled, two Cargo jobs and debug information disabled.
Every test executable was selected from fresh Cargo JSON whose source path and
test profile matched this checkout. Filesystem free-space and available-memory
guards were checked before compilation and test suites.

On the final production candidate:

- `cargo fmt --all -- --check`: passed.
- Locked compilation of the library and K1, K2 and deterministic-cycle suites:
  passed.
- Complete library test binary: 298 passed, zero failed or ignored.
- `typed_phantom_products`: 20 passed, zero failed or ignored.
- `typed_nullary_phantoms`: 19 passed, zero failed or ignored.
- `type_cycle_diagnostics`: 6 passed, zero failed or ignored.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo build --locked --features c-import`: passed; this prepares the
  importer-enabled CLI for the independent configured-native seam.
- `cargo clippy --locked --features c-import --all-targets -- -D warnings`:
  passed.

The final focused total is 343 tests. Earlier candidate `28a5c27` also passed
443 affected tests: 295 library tests and 148 integration tests covering K1/K2,
specialization, local values, records, root/field loans, Copy patterns, scalar
and raw native transport, and both optional origin APIs. Later production
changes add conservative work reservations and the separately reviewed cycle
correction; the final aggregate below repeats all suites on the combined source.

The K2 suite explicitly invoked GCC 14.2.0 and Clang 19.1.7 at both `-O0` and
`-O2`, with `-std=c11 -pedantic-errors`. Both compilers were available; there
were no missing-compiler skips. Pure fixtures compare ordinary typed,
reference and default evaluation, exact values/traps, repeated deterministic C,
and byte-identical ordinary/typed C output without the uniform representation.
Native fixtures assert exact header contracts, pointer-slot constness, trace
ordering, unchanged reached evaluator errors and unused-signature rejection.
They do not claim interpreter FFI execution. The uniform C oracle's narrower
open-layout boundary is unchanged.

Internal tests cover closed-descendant field forgery, full record/variant
identity, nested and closed phantom argument swaps, inactive optional payloads,
physical owners, forged native dependency bits/pointers, binder escape and
consistent schema rewriting rejected at the checked-source boundary. Work tests
cover exact/one-short catalog, substitution, verifier and registry reservations,
closed-descendant failure before parent publication, exhausted cached requests,
composed physical depth, copied pointer atoms and separate name copy/hash costs.

## Retained failed attempts and repairs

The initial candidate `14f12c6` compiled and passed 27 logical tests and all 19
K1 integration tests. Its first formatting check failed on two local formatting
hunks; the hunks were formatted and subsequent checks passed. Its K2 suite had
18 passes and two failures:

1. A foreign-constant evaluator-order fixture declared `WORD: u32` without the
   mandatory `@include`. Source analysis correctly rejected the fixture. The
   repaired fixture provides its actual header; the intended error-order test
   now passes. No native checking rule changed.
2. The ordinary mutual recursion fixture `A<a> = A(B<a>)`, `B<a> = B(A<a>)`
   produced valid but different A/B cycle witnesses across separate analyses.
   The failure was an existing diagnostic-stability observation, not a product
   proof defect. The bounded K2 assertion initially accepted either witness.
   The separate [cycle correction](2026-11-03-type-cycle-diagnostics.md) now
   selects the first cyclic declaration in declaration order, and its six public
   regression tests plus deterministic private test pass in the final candidate.

An affected-suite compilation command also named the nonexistent target
`typed_copy_node_origins`; Cargo rejected the command before compiling. It was
corrected to the existing `typed_node_origins` target and the affected pass above
succeeded. Original failed logs and subsequent successful results were retained.

Independent source review identified precharge gaps in schema validation, name
indexes, closed descendants and copied pointer atoms. These were repaired with
separate bounded reservations and regression tests before the final candidate.
No reported gap established a representation identity or field-erasure bypass.

## Independent execution

The final clean candidate `6e337dd8dd2075db282bad4c2c155294a60f97f3` passed:

- 24 independent semantic/source-boundary cases.
- 68 strict C11 builds and executions across GCC 14.2.0, Clang 19.1.7 and O0/O2.
- Four independently authored crate-private groups covering 29 malformed
  product layouts, with all three consumers rejecting each mutation: 87
  expected rejections. These tests are retained in
  `src/typed/independent_phantom_product_tests.rs`.
- 32 existing private logical/registry tests replayed separately.
- Four configured-native compiler/optimization combinations: 40 CLI commands
  and four direct generated-program runs, including eight expected header-drift
  or binding-snapshot rejections with unchanged prior outputs.

The public cases include exact nominal/closed-descendant identities, nested
physical products, snapshots and loans, runtime traps, raw/native dependencies,
origin boundaries and deterministic first-cyclic-declaration errors across all
seven source entry points. Fresh default/importer build records identify the
exact final manifest and source; both were newly compiled. The final importer
CLI SHA256 was
`ace06e897c651a680fb7f2f016734d880f99208da233a894823726383f312704`.

Initial independent harness failures were retained and corrected without
weakening the compiler: external mutation of private ADT fields failed Rust
compilation, so the malformed cases became proper test-only child-module tests;
the explicit reference evaluator correctly rejected a closed owner control
while typed/default execution returned 42; and Clang forwarded its existing
extraneous-parentheses diagnostics before correct CLI runtime output. The
harness recognizes only those exact known warnings, does not suppress them,
and still requires exact direct-executable traces. All final expectations pass.

## Full integrated aggregate

The clean final source above passed all seven stages from 00:09 to 00:12 UTC
on 4 November 2026:

- `cargo fmt --all -- --check`.
- `cargo test --locked --no-fail-fast -- --nocapture`: 762 passed, zero failed
  or ignored, no dependency skip notices.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo test --locked --features c-import --no-fail-fast -- --nocapture`:
  810 passed, zero failed or ignored, no dependency skip notices.
- `cargo clippy --locked --features c-import --all-targets -- -D warnings`.
- Both default and importer-enabled `cargo check --locked --target
  x86_64-pc-windows-gnu --all-targets` configurations. These are compile-only.

Source-root timestamps were refreshed without byte changes to force fresh
builds from the final main checkout. Source stayed clean at the same commit
throughout. Existing real configured/direct/generated SDL3 execution is part
of these Linux suites. Screenshots were not recreated, and no new physical
input, GPU, Windows-runtime, Zig or AArch64 execution evidence is claimed.
The subsequent checkpoint changes acceptance/status documentation only.

Genuinely unknown stored payloads, bare unresolved/no-return values, broader
origin mapping and owner products remain outside this bounded slice.
