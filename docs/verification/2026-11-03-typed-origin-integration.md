# Integrated function-instance origin checkpoint

Final tested source: `a9afd97fa46c59922761decc60d208f81a368e15`.
This combines the existing checked scalar native path with the optional
[function-instance origin API](../typed-function-origins.md). The following
checkpoint changes documentation only.

## Integrated result

The complete matrix passed against the clean unchanged source on 3 November
2026, 22:02:44–22:06:06 UTC, using x86_64 Linux, Rust/Cargo 1.99.1,
GCC 14.2.0, Clang/libclang 19.1.7 and SDL3 3.2.10.

- 584 default tests passed, zero failures or ignored tests.
- 618 `c-import` tests passed, zero failures or ignored tests.
- Neither suite reported a dependency skip; direct/generated real SDL ran.
- Formatting and strict all-target Clippy passed in both feature configurations.
- Windows GNU all-target compile checks passed in both configurations. These
  are compile-only, not Windows execution or C ABI evidence.

Commands are the same seven checks listed in the
[native integration report](2026-11-03-typed-native-integration.md). Builds used
two jobs, no incremental compilation and no dev/test debug symbols, with
2 GiB target-free and 3 GiB available-memory floors.

## Shared-target rebuild correction

The first default aggregate stopped before test execution because a shared
build target supplied a library from the preceding raw-native worktree. The
frozen source declared the provenance module and had no raw `CU8` IR variant,
but the cached library accepted a `CU8` probe and rejected the provenance import.
Its timestamp matched the earlier worktree build. The feature library rebuilt
from the correct checkout and its tests passed.

Refreshing this checkout's crate-root timestamps forced recompilation without
changing source bytes, deleting caches or weakening tests. The same two probes
then reversed exactly: provenance was available and `CU8` absent. The entire
seven-stage matrix above was rerun successfully in a new evidence directory;
the original failure and diagnostic probes were retained. Shared-target
verification now explicitly forces current-checkout crate-root recompilation
when switching worktrees and records the package path and source identity.

## Independent acceptance and merge boundary

An independent review accepted merged source
`ba95fb243eaf3cd7d0f81a69a867006846feef28`; later changes before the matrix were
documentation only. Its 60 passing checks comprise five bounded public-API
harness cases, 24 private provenance/worklist/native checks and 31 integration
checks. The source and freshly built library identities were recorded.
The acceptance artifact has SHA256
`004008bd2e0ef45fdbbe53b736cb9fda986d5cb03db1f65323b4e5273fb348e7`.

Shared lowering preserves native-manifest construction and native/owner guards.
The direct-CStr filter lives in the shared traced traversal, retains original
argument indexes and skips only verified literal CStr operands. New integration
and independent first/middle/last CStr cases check these seams. The optional
mapping API continues to reject native mapping after ordinary lowering, while
ordinary native compilation retains its existing supported behavior.

The [author report](2026-11-03-typed-function-origins.md) separately records
558/592 tests on its pre-native-integration branch, early fixture corrections,
review fixes and the temporary runner error. These counts are not additive.

## Scope

The optional immutable index records concrete function instances, resolver
definitions, specialization request sites and explicitly uninstantiated
functions for its supported subset. Ordinary compilation and emitted C remain
unchanged. This is not complete typed-node, capture, cleanup or generated-C
source mapping, a portable cache identity, or a completed PLAN phase.
