# Port the current work onto v1-ernie

The integration starts at `c3f41ef6027033553bd19202bbf457e5d9a3c9d1`.
The complete source snapshot is preserved by the local archive branch
`archive/source-snapshot-69277817` at
`69277817ff0408c44dacb723bb1edc20da86159d`.
The original working copy stays intact while the port is developed separately.

## Acceptance

Keep the upstream typed-native backend, checked source origins, explicit native
configuration replay, phantom layouts, nonreturning and discarded results,
closed signatures, recursive constraints and deferred record projections.
Port or explicitly account for every functional addition in the archive.
No unsupported combination may invent a layout, signature or source origin.
Each independently usable slice must have fresh verification before its commit
is pushed normally to `v1-ernie`. Never force-push that branch.

The final gate is the integrated default and minimal-feature Rust suites,
formatting, strict Clippy, editor package tests and real Helix workflows,
plus available SDL and GPU integration checks. Record environmental skips and
remaining unsupported combinations. These checks do not establish a stable
language or complete a delivery phase in PLAN.md.

## Checklist

- [x] Read the Principles section of the poteto-mode skill.
- [x] Frame the work and compare both implementations before editing.
- [x] Preserve the source snapshot and create isolated integration worktrees.
- [x] Capture the upstream baseline before integrating features.
- [x] Port boolean negation and unit-valued conditional statements.
- [x] Port callable syntax, inference, checked conversion and source evaluation.
- [x] Port library checking and immutable editor overlays.
- [x] Port native action validation, automatic headers and guarded interfaces.
- [x] Port ordinary optimized execution and reference runtime compaction.
- [x] Port grammar and shared queries, then individual editor clients.
- [x] Port the LSP and adapt explicit result annotation hints.
- [x] Port SDL lifecycle handling and portable capture, retaining the matrix.
- [x] Port the CPU voxel reference, then the cohesive GPU demo.
- [x] Reconcile current documentation without deleting upstream evidence.
- [ ] Run the integrated verification gates and review all retained changes.
- [ ] Audit the decision trail with a different model family.
- [ ] Push the verified slices and update the working checkout safely.

## Workflow

Compiler, native imports and editor packages use separate branches/worktrees.
Their owners commit focused units and report evidence. The integration owner
reviews each diff, checks the resulting artifact and serially incorporates it.
Shared compiler files retain upstream checks rather than replacing them with
older copies. Each push is a fast-forward after checking the remote head.

The compiler port is the highest semantic risk. Its gates include upstream
recursive signatures, projections, phantom products, nonreturning and discarded
results. Native imports retain structured facts and fresh configured replay.
Editor analysis consumes guarded interfaces without executing dependency tools.
The uniform payload optimization applies to the reference backend; the normal
native backend remains typed.

The canonical decision trail is
`docs/verification/2026-11-04-ernie-decisions.tsv`. Historical reports retain
their checkpoint context. A fresh integration report records actual commands
and outcomes for this stack.

## Baseline outcome

The isolated upstream run reproduced host launch-deadline and raw-filename
capability failures. It was stopped with evidence retained while the complete
integrated default/minimal suites run on frozen functional source `f105d7d`.
A test-only handshake separates bounded macOS startup from existing execution
deadlines. Full acceptance and final editor workflow checks remain pending.
