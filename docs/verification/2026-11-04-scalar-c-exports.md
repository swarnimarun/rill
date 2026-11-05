# Scalar C export verification

This increment adds an experimental fixed-i32 C11 export API and tightens the
raw-type admission contract shared with interpreter reload. It implements no
native loader, dispatch replacement, state migration or PLAN.md phase gate.
See the [export contract](../c-exports.md) and
[remaining native reload work](../blockers/hot-reload.md).

## Compiler and consumer evidence

- Every declaration passes ordinary library analysis before export selection.
  Real monomorphic signatures seed typed instances. No synthetic entry or
  fabricated arguments are used for native library lowering.
- A private alternate entry verifier admits the fixed-i32 endpoints and retains
  the complete common signature/body/local/loan verifier. Unsupported logical
  and control proof tables reject rather than being reinterpreted as multiple
  roots. Malformed typed signatures, IDs and bodies have direct regressions.
- Public C names resolve to the actual verified IDs after specialization and
  sorting. Separate C translation units exercise reversed export order and
  source declaration permutations. Public headers/manifests stay identical
  across the tested source reorder.
- All **14 ABI tests** pass. Independent C11 callers run at O0/O3 with multiple
  artifacts, repeated/multiple headers, direct/mutual recursion, signed limits,
  overflow/division/depth traps, repeated calls and independent host threads.
  The maximum 64 exports and 32 parameters compile and execute. Generic bool/i32
  helpers execute, and loaded `std.reload` hooks use resolved symbol metadata.
- The checked-in Rust generator and independent C caller print `42` when built
  with Apple Clang at O3. The same emitted source and caller also compile and
  print `42` with Zig 0.17.0 `zig cc -std=c11 -O3`. That Zig smoke check does not
  repeat the Clang trap/thread matrix. Generated files and executables remain
  outside Git.
- Factoring the depth declaration preserves the captured factorial executable C
  byte for byte. Both files have SHA256
  `00d405ecadcb0b0d6f997352f968132ad2c468fd96dde0f2a8b8caa99da9332e`.
  This is a concrete compatibility fixture, not a universal byte-equivalence
  proof. Library emission alone selects C11 thread-local depth.

## Admission fix and variant search

Independent review reproduced an admission gap: an unused raw parameter could
escape an expression-only query. Exact ABI use matched once; searching all
`has_c_values` consumers found six locations. ABI and `reload::Session` had the
same whole-library contract violation. Two provenance consumers deliberately
describe reached layouts, and ownership/typed routing retain ordinary checked
and reached-representation policies. No unsafe emitted-code bypass was found in
those four consumers; their behavior is unchanged.

The new private raw-source scan includes all checked function signatures,
expression/application types and declaration schemas. It visits nested nominal
arguments, callable environments/parameters/results and internal CStr. Schemas
are scanned once without nominal expansion, respect generic parameter binding,
and share a 4,000,000-node scheduling budget. Children are charged before their
reference worklist grows. Hash-map traversal accumulates the complete result so
budget outcomes do not depend on iteration order.

Three new ABI rejection tests were observed failing before the fix and passing
afterward. Positive generic shadowing remains accepted and executes in C. Both
new Session creation and replacement reject dormant raw scalars, pointers,
declaration fields, phantom arguments, nested callable types and internal CStr.
All **10 reload tests** pass. Rejected candidates preserve accepted generation 1
and old calls still return `42`. Two small-budget scanner tests cover admission
charging and internal-CStr/binder precedence without allocating huge fixtures.

The regression commands below are the CI guard for this family. Do not replace
the library-wide query with the expression-only routing query.

## Executed checks

On arm64 macOS, Rust 1.99.1 and Apple Clang 17:

```sh
cargo test --locked --lib --bin rill --test abi_exports --test reload --test docs_cli --test project_cli --test typed_source_origins --test typed_node_origins --test owned_box --test typed_foreign_raw
cargo test --locked --no-default-features --lib --bin rill --test abi_exports --test reload --test docs_cli --test project_cli
cargo test --locked --no-default-features --test reload
cargo test --locked --test abi_exports --test reload --test docs_cli
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

The default selection passes 669 tests and the minimal selection passes 601.
Both include all 14 ABI cases. They compiled before the two additional reload
regressions were written; the final reload selection passes all 10 cases.
Counts overlap and are not summed. Both all-target strict lint configurations,
formatting and diff checks pass. Independent review also ran scratch C11 UBSan
probes for signed arithmetic, record/loan helpers and traps at O0/O3.

The final boundary run passes all 37 ABI/reload/docs CLI tests, including the
new reload cases. The regenerated example-project site passes its HTML audit
across 27 pages, 2,017 links and 191 search addresses. Changed Markdown source
links also resolve. No generated output is committed.

## Aggregate native matrix

```sh
SDKROOT="$(xcrun --show-sdk-path)" cargo test --locked --no-fail-fast
```

The complete command exits successfully and reports **1,457 passed, 0 failed
and 0 ignored** across 142 result groups. This includes native C, configured
real SDL3, owner cleanup, logical/control representations, module/editor and
project/watch regressions. Both AArch64 NEON cases pass, including actual vector
execution and the generated XOR instruction check.

The aggregate run began before the later admission-only fix and additional
tests. Its initial library and ABI targets contain 530 and 8 tests, respectively;
its later reload target contains all 10. The final targeted runs above verify
the current 532 library tests and all 14 ABI tests, plus the final boundary
selection. These scopes are kept separate rather than presenting the older
prefix as a full frozen-source run of the final admission change. Counts overlap
and are not added.

The GPU target has availability-based early returns that the runner counts as
passes, not ignored tests. A separate `--test sdl3_voxel_gpu -- --nocapture` run
confirms three hardware preflights return status 77 because the video driver
adds no displays. The fourth, 256-cubed capacity branch is not enabled without
`RILL_TEST_GPU_256=1`. All four GPU hardware branches are therefore skipped;
this is not GPU execution acceptance. No environment or source workaround was
used to change those outcomes.

## Limits

The manifest describes the generated scalar interface; it does not authenticate
a binary or record complete target/build compatibility. Wider signatures,
retained resources, callbacks, loader coverage, quiescence and state migration
remain open. No runtime-speed or large-project-build speed claim follows from
these semantic tests.

Generated documentation remains offline and addressable. Visual browser QA is
still unavailable because the browser policy rejects the local file URL; no
protocol workaround was used. This record makes no rendered-layout claim.
