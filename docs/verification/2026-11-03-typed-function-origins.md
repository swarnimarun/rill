# Function-instance origin index, 3 November 2026

This increment starts from `214987d51f2feb367bb38ebf0f2ea60447ca8cbe`.
The final tested Rust/test source is
`555b8ea36e0629cc322ab595ac51e9392ace035b`; subsequent changes only publish this
verification record and documentation links. See the
[API and scope contract](../typed-function-origins.md).

The optional API consumes the existing validated source result and owns the
same ordinary verified typed program plus an immutable function-instance index.
It records actual resolver definitions, exact closed parameter/result types,
parameter modes, every specialization request and explicitly uninstantiated
function definitions. No typed expression, storage, capture, cleanup, native-call
or generated-C range coverage is claimed.

## Fresh author checks

Environment: x86_64 Linux, Rust/Cargo 1.99.1 and GCC 14.2.0 as `cc`.
Checks ran serially in the shared temporary target with two Cargo jobs,
dev/test debug information disabled and incremental compilation disabled.
Resource checks kept at least 2 GiB free target storage and 3 GiB available RAM.
There were no installations, cache removals or remote operations in this work.

```sh
cargo test --locked --lib typed::provenance -- --nocapture
cargo test --locked --lib discovery_trace_work_limit_has_an_exact_boundary -- --nocapture
cargo test --locked --test typed_source_origins -- --nocapture
cargo test --locked --test ast_source_map --test module_origins --test typed_specialization --test typed_local_values --test typed_copy_patterns --test typed_copy_loans --test typed_field_loans --test typed_records --test typed_vectors --test typed_c
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo test --locked --test sdl3_direct -- --nocapture
cargo test --locked --features c-import --test sdl3_direct -- --nocapture
```

All listed commands passed:

- 17 new focused tests: eight private provenance tests, one exact discovery-work
  boundary and eight public integration tests.
- 77 existing affected integration tests, including existing interpreter/C
  differential cases at `-O0` and `-O2`.
- Full default suite: 558 passed, no failures or ignored tests.
- Full `c-import` suite: 592 passed, no failures or ignored tests.
- Formatting and both strict all-target Clippy configurations passed.
- Explicit uncaptured SDL checks ran against the real library without a skip:
  one default test and two feature tests, including generated bindings.

The full suites include the focused/affected tests; these counts are not
additive. The full suites and Clippy were rerun after the final lint-only source
edit. The existing x86_64 unsupported-NEON-target test ran; AArch64-only execution
is not compiled on this host. No fresh Zig or AArch64 runtime campaign is claimed.

Early private-test runs exposed two fixture errors: a deeply nested expression
hit the existing parser syntax-depth guard, and a fixture looked up an original
name in the renamed program. The 32-level concrete signature now uses shallow
sequential constructor bindings, and identity lookup uses the resolver target.
The structural-path fixture received the same original/renamed-name correction.
No language limit or compiler semantics were weakened to make these tests pass.
An unused private traversal wrapper and one strict-Clippy style warning were
also corrected before final aggregate verification.

The temporary external runner was edited to add later SDL invocations while
its full-suite process was running. It emitted a trailing shell command error
after all five recorded commands had individually exited successfully. The
corrected runner passed syntax validation; SDL invocations, a direct feature
Clippy recheck and formatting then completed with successful outer exit status.
This was a runner bookkeeping error, not a failed repository check.

## Covered correspondence and limits

Public tests cover return-only generic distinctions, repeated callers,
recursive request graphs and stable record order; imported same-spelled private
functions/types; source changes and deletion; foreign snapshot handles and
whole-result clones; repeated call syntax and CRLF/tab anchors; and explicit
unused-function disposition. One fixture checks every source call through
record field order, field/root assignment, loops, statement discard, conditionals,
match scrutinee/arms and nested arguments.

Scalar, generic/phantom local shape, loan, temporary, owner, fixed-byte and UTF-8
fixtures compare ordinary and mapped interpreter results and exact C text.
The deep concrete signature fixture preserves all 32 nominal arguments without
repeating discovery. Native/fallback cases preserve the ordinary error or output
while rejecting unsupported mapping. Byte equality is not a complete generated-C
source map and does not by itself prove the shared lowering refactor correct;
existing differential regressions and source review are separate evidence.

Private corruption cases reject missing/duplicate requests, wrong paths,
invalid caller/callee IDs, stale pre-permutation entry IDs, same-parameter but
wrong-result callees, swapped/missing definition identities and wrong typed
signatures. Foreign-domain and invalid-slot handles do not resolve. Tiny request
and work budgets establish exact boundaries and preserve ordinary semantic
error precedence after a metadata failure latches.

## Independent source review

An independent source-only review found that signature/count checks alone could
let a privately corrupted entry instance claim a same-signature unused
function. The entry now must identify actual source `main`, and source indices
are checked before indexing bridge tables. The new regression passes.
The reviewer also recommended charging validation child scheduling before
worklist growth; this was adopted without changing successful node-unit totals.
Logical metadata work units are explicitly not a byte-accurate memory cap.

The reviewer reported no remaining source-correctness blocker at `9538504`.
The only subsequent production change is the four-line Clippy-prescribed
collapse of the nominal insertion branch. Independent public-harness execution
and final combined integration remain the separate integration gates;
the author does not claim that the source reviewer executed this test matrix.

## Integration boundaries and remaining work

The parallel native-call increment changes the lowering prologue. Integration
must keep native-manifest construction, owner/native rejection and native-value
checks in the shared `lower_main_traced` helper. The optional map's narrower
native rejection stays after ordinary lowering/verification.

Its narrow direct-CStr argument filter must be placed in shared
`runtime_children_with_steps`, not only the plain wrapper. Retained arguments
keep their original `Argument(index)` even when a CStr operand is omitted.
Preserve actual-extern, matching-arity, CStr-parameter/literal and NUL-free checks.
The combined native/origin result needs its own final test matrix.

Complete typed-node and generated-C origins, source-aware inference diagnostics,
portable/cache identities, debugger locations and the PLAN phase gates remain
open. This increment changes no owner/native-language eligibility, lifetime or
cleanup contract and is not a completed P1, P2, P4 or C18 milestone.
