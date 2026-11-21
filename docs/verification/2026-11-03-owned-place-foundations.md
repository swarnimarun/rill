# Type-shaped initialization foundations

This is an internal prerequisite for the next bounded owned-record increment,
not an expansion of accepted source and not completion of P2. The authoring base
is `7f1e961c442a60bd347980219d93d0809da79cc2` (the reviewed typed-vector merge).

## Implemented boundary

- A normalized `Place` retains local identity and nominal record-field steps.
  Empty paths denote roots. Valid forwarding composition has no second source
  operand depth cap; source field operands retain the existing 16-step cap.
- Typed locals retain a full initialization tree instead of a root boolean.
  Enums remain atomic. Records retain every child's state, including Copy
  fields. A materialized shell whose owner fields all moved is distinct from a
  dead root. Root move adapters use one authority/overlap/state transition.
- Owner-local, value-parameter and consuming-arm cleanup annotations contain
  exact cleanup trees. The verifier independently builds and updates its own
  initialization state and compares the derived cleanup against the annotation.
  Branch equality includes complete trees and projected loan intervals.
- The currently accepted runtime/C paths still execute only whole live atomic
  owner cleanup. Owned-record source, layouts and partial moves remain rejected
  until both checked consumers implement their storage and cleanup behavior.
  No placeholder fields, extra Rill allocations or foreign-owner path was added.

Tree construction and cleanup validation are bounded to 16,384 nodes and 128
record-nesting steps. The node allowance follows the existing 1 MiB nominal
layout budget's minimum 64-byte per-node charge. These local bounds are not a
whole-compiler memory guarantee.

## Tests and verification scope

Fourteen new unit tests cover state shape, atomic enums, empty Copy records,
ancestor/descendant availability, disjoint moves, materialized empty shells,
exact cleanup shape, malformed metadata, borrowed authority, Copy-move rejection,
active prefix loans and long composed paths. An independent numeric-prefix
oracle checks 625 four-move sequences (2,500 transitions) against state and
cleanup results. Forged cleanup annotations are rejected by verification and by
both public execution/emission entry points before consumption.

Author checks use the provided Rust toolchain, GCC 14.2.0 and Clang 19.1.7.
The final full default suite passes 506 tests. Strict all-target Clippy and
formatting pass on this foundation. A separate Clang owner/allocation/record/loan differential selection
passes 62 tests; those suites retain their existing `-O0`/`-O2` comparisons and
optimization/failure-trace checks. The final library selection passes 180 tests.
An earlier full default run passed 505 tests before the last independent oracle
was added. The 506-test aggregate includes that final oracle.

Optional SDL3, Zig, NEON, sanitizer and LTO cases retain their own capability
probes. A passing wrapper test is not evidence that a skipped optional tool or
runtime configuration was exercised. No new sanitizer availability claim is
made by this bookkeeping-only increment. Independent review and combined-branch
verification remain separate gates.
