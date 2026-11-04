# Interrupted arguments: author verification

On 4 November 2026 the bounded K3b candidate was implemented from accepted base
`29b6c65cfd29c0e8043024b5d0acb4f3b13d2d6e`.

- Production and private regressions: `6a695b1663f36a638ad03822421315869d6b3c98`.
- Public tests and explicit old-negative migration:
  `55804411f6711b5ed3ab6794100d7b5a3afe0053`.

The [contract](../blockers/interrupted-arguments.md) defines the precise slice.
This is author verification, not independent acceptance or the full PLAN gate.

## Executed checks

The final production passed all 395 library unit tests and 161 selected public
regression tests: **556 passed, zero failed/ignored**. Public suites covered
interrupted arguments, K1/K2/K3, Copy root/field loans, local shapes, native
scalar/raw transport, both optional origin APIs and existing closed-owner
controls. Default strict all-target Clippy and formatting passed.

The new eight-test public suite separately passed with GCC 14.2 and Clang 19.1
at C11 `-pedantic-errors`, O0 and O2: **68 C executions** across those two final
runs. It compares explicit typed, default and reference evaluation, verifies
default C equals typed C and rejects accidental uniform-value fallback.

Exact C event tests retain the real main. They observe one earlier prefix,
31 recursive argument evaluations, 32 admitted frames and a 33rd entry attempt
with no suffix or outer-body events. An argument trap suppresses that 33rd
attempt. A preceding trap prevents all recursion. Existing outer read/inout
calls with concrete argument layouts retain their ordinary K3 path.

Malformed tests reject changed prefix/barrier/order, foreign source brands,
outer identities, full logical signatures, retained suffix syntax and missing
proof metadata before verify/evaluate/emit. Concrete nested-barrier flow is
recomputed independently rather than trusting its tag. An exact/one-short
budget check and an over-deep forged logical signature fail with hard
complexity errors. Original CheckedProgram correspondence is tested separately
from standalone frozen-source verification.

Rust/Cargo 1.99.1 and the existing installed C toolchains were used, with two
Cargo jobs, disabled incremental compilation and zero dev/test debug information.
Fresh Cargo JSON and artifact hashes were retained for the actual checkout;
root timestamp refreshes preserved source bytes. No stale other-worktree rlib
was linked for the initial public probes.

## Retained initial failures

The first compile identified four exhaustive optional-origin matches needing
explicit extension rejection. A test fixture used a nonexistent CopyLocals
constructor. Strict Clippy identified two unnecessary test Box replacements and
helper placement after a test module. These were corrected without bypassing
private IR boundaries or dropping tests.

The initial broad unit run passed 387 tests and failed two malformed-depth
assertions because new canonical arena validation ran before the existing
iterative depth preflight. Restoring depth-first diagnostic precedence produced
a complete 389-unit pass; the six later malformed tests bring the final count
to 395. Source review also prevented a compatibility regression: excluded outer
read/inout modes now fall through to the existing ordinary call path, rather
than rejecting already-representable calls. The full final affected run above
covers both corrections.

## Pending acceptance

Independent adversarial execution, the complete default/importer test suites,
both strict lint feature modes and both Windows GNU compile modes belong to
the accepting checkpoint. No new SDL screenshot, physical-input, Windows
execution, Zig or AArch64 NEON result is claimed here. The remaining language
plan stays open.
