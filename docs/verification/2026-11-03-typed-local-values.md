# Typed local value shapes: 3 November 2026

This increment starts from the reviewed Copy field-loan checkpoint `00b0b9d`.
It preserves the conservative local value restriction and extends typed lowering
only for its immutable nonexpansive Copy schemes. It does not complete P1, C03,
MIR, ownership, effect inference or source mapping.

## Accepted slice

- Nullary/positional constructors, nested enums/records, immutable aliases and
  independent concrete whole-value uses of one local scheme.
- Independent whole-record and field uses, including a field with a closed type
  while unrelated phantom parameters remain unresolved.
- Concrete captures tied to function parameters, mutable-storage snapshots,
  read/inout snapshots, fixed buffers and static strings.
- Unused shapes and aliases without fabricated phantom types. Concrete leaves
  are still snapshotted once at the original binding in written order.
- Shared read arguments as exact concrete temporaries, with source loan checks
  unchanged; imported nominal identities, local/constructor shadowing and
  repeated generic-function instances.

The [representation decision](../decisions/0004-local-value-shapes.md) explains
compiler-generated capture records, explicit origin metadata, shape sharing,
concrete use-site reconstruction and resource policies.

## Evidence

`tests/typed_local_values.rs` compares the independently checked AST interpreter,
typed interpreter, default interpreter, typed C emitter and default C emitter.
Concrete differential cases compile as strict C11 with `-pedantic-errors` at both
`-O0` and `-O2`. Cases cover:

- Different concrete enum/record instances of one source binding, whole/field
  combinations, nested products, pure aliases and phantom-only storage.
- Snapshot timing and mutation after capture; effectful computations remain in
  their original monomorphic bindings. Traps remain at source positions and
  function arguments retain left-to-right evaluation.
- Distinct storage under shadowing despite shared capture layouts, constructor
  namespace resolution, imported nominal names, borrowed-root snapshots and
  sixteen source projection steps through a captured concrete record.
- Generated alternating record/enum shapes at six depths, each materialized at
  independent integer and boolean types and compared with the source oracle.
- Owner/phantom-owner rejection, mutable/weak-polymorphism rejection, active-loan
  restrictions and unresolved-use diagnostics.
- Exact acceptance at 256 concrete leaves and diagnostic rejection at 257,
  including otherwise unused values. The flat capture product avoids the
  recursive-let stack failure found during development.

`src/typed/local_values_tests.rs` separately asserts reconstruction details:
source binding/use identities; exact concrete use types; one original snapshot;
written capture order; shared alias capture roots; distinct shadowed roots;
absence of fabricated source nominal layouts; explicit generated-layout origin;
and exact temporary read operands. Forged IR changes to capture types, scope,
nominal instances, loan modes, owner status and capture metadata are rejected by
verification and both IR consumers.

Private tests check the exact 100,000-work and 256-capture boundaries, bounded
shared-DAG height, and repeated expansion: typed lowering accepts six uses of
the tested 8,191-node DAG while the seventh reports the work-limit diagnostic.
C emission can separately reject those large frames under its existing stack
budget. This tests charging before expansion, not an eventual C stack error.

## Verification commands

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast
cargo clippy --locked --features c-import --all-targets -- -D warnings
```

Final author checks passed on Linux x86-64 with Rust 1.99.1 and GCC 14.2:
formatting, all 494 default tests, all 528 `c-import` tests, and strict all-target
Clippy in both configurations. Direct and generated SDL3 binding tests also ran
against the actual library without a skip. Native AArch64/NEON execution was not
run on this host; optional target checks retain their existing availability gates.
The [local-value example](../../examples/local-values/main.rill) passes `check`
and prints `42` through the CLI. The 12 source-format/normative-example tests pass
with that new file included.

One intermediate feature run was invalidated by an overlapping default-feature
Cargo invocation replacing the shared CLI binary. Its 23 CLI-related failures
all reported that `c-import` was disabled. That failed run is not counted above;
the complete feature suite was rerun serially and all 528 tests passed. Future
runs sharing a target directory must remain serial through test execution, not
just compilation.

## Explicit remaining boundary

A closed type is still needed for a real use, even when its value is statically
`None`. Unconstrained match scrutinees, weak call/field results and an unresolved
`main` result are not given arbitrary types. Owner programs retain their source
ownership restrictions. Capture records are private compiler storage, not source
record declarations or a stable ABI. Concrete IR verification protects its own
operations but is not a proof of source-to-IR semantic preservation.

## Combined integration seam

The separately preserved author commit is `e2f0a1b`. Integration starts from
`fedb2d4`, retaining the bounded type-materialization policy, typed vectors and
the initialization/cleanup-tree foundation. The checker additions remain only
binding/use identities; all fallible charged scheme copies remain in place.
Capture eligibility now reads `Initialization::fully_initialized()` rather than
the removed availability boolean. Normal concrete IR verification builds full
initialization state for the generated immutable capture records. Existing
cleanup metadata, moves, joins and loan checks are unchanged.

The two new vector fixtures snapshot `u8x16` into a generalized marker and
record, mutate the original vector, then use independent whole/field/read
instances. Both produce `42` in the source oracle, typed/default interpreters
and C at `-O0`/`-O2`. The initial combined focused run passed all 193 library,
4 type-materialization, 17 local-value and 6 vector integration tests.
The frozen integrated production commit is `2d1db129`. Its final author matrix
passed all 536 default tests and all 570 `c-import` tests with normal test
threading, formatting, and strict all-target Clippy in both configurations.
A separate complete feature run with `--test-threads=1` also passed all 570
tests. The direct and generated SDL3 tests ran against the actual library and
passed without skips. All eight recorded production hashes remained unchanged.

The first integrated normal-thread feature run had 569 passes and one failure:
an unchanged import-worker fixture could not start its freshly written script
because the operating system returned `ETXTBSY` (text file busy). That failed
run is preserved and is not counted as a pass. The exact fixture, complete
single-thread feature suite, and complete normal-thread feature retry then
passed without changing the fixture or compiler. The cause is not established;
no production retry or assertion workaround was added.

An independent review of the exact integrated commit also passed clean normal
536/570-test aggregates, formatting, both strict Clippy configurations, 110
native differential executions, 59 capped source probes, and four additional
capture/initialization/cleanup tests. Its scope is this local-value elaboration
and the integration seams, not complete P1/C03, general owned-record support,
complete MIR, or whole-compiler resource safety.
