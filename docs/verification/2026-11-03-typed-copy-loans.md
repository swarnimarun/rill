# Checked root loans for Copy values

This bounded P1/P2 increment migrates existing root-only `read` and `inout`
calls onto the shared typed interpreter/C path. It adds no source syntax and
does not complete a phase. Records, field loans, escaping references, CFG loan
inference and general initialization/partial moves remain separate.

## Representation and semantics

Typed call operands distinguish shared Copy roots, exclusive Copy roots and
computed read temporaries. Supported types are integers, booleans, unit,
static strings, fixed byte buffers and finite Copy enums, including concrete
generic function instances. Value arguments remain snapshots. A direct local
borrows its existing storage; a computed expression evaluates once into fresh
storage at its position in the argument list.

Independently of source checking, the typed verifier checks local identity,
concrete type, calling mode, availability and mutability. Loans remain active
through every later argument, including nested calls, branches, loops and
shadowing. Shared loans permit other shared loans and ordinary reads. Writes
and exclusive reborrows conflict with shared loans; any overlapping access
conflicts with an exclusive loan. Nested calls restore the enclosing loan
state rather than clearing it. A shared parameter cannot become exclusive.

The interpreter uses internal reference-counted cells for addressable Copy
storage. Only explicit loans share cells; ordinary reads, declarations, value
arguments and returns copy values. Temporary storage lives through its call.
These cells are interpreter implementation storage, not new Rill owners or
Rill allocator effects. C uses per-type pointers, const-qualified for shared
parameters, with explicit left-to-right argument evaluation. It adds neither
heap allocation for loans nor `restrict` or no-alias claims.

Restricted owner programs now permit borrowed non-owner scalar, string, buffer
and finite Copy-enum parameters. Borrowed owner-bearing enums, record
parameters, generic owner functions and recursive owner call graphs remain
unsupported. Owner transfers and cleanup keep their own representation.
Allocation metadata, call-graph, buffer-type and owner-runtime traversals all
visit computed read temporaries.

## Recovery and fresh evidence

The original reviewed implementation became unavailable with its cloud
filesystem; its checkpoint was absent from the durable source archive. This
increment is reconstructed
on the recovered source checkpoint and must pass fresh checks. Earlier test
and review results are not evidence for these reconstructed bytes.

- Seven private groups cover malformed modes/IDs/types, immutable roots,
  shared/exclusive overlap, later reads and writes, nested loan release, shared
  parameter mutation and storage snapshot independence. Invalid IR is rejected
  before evaluation and C emission. Temporary traversal tests cover scoped
  owners, allocation success/failure and forged allocation metadata, plus a
  recursive call hidden inside a temporary.
- Four source groups cover 29 successful/trapping programs and eight source
  loan rejections. Copy cases compare the checked AST oracle, typed evaluation,
  production interpretation and identical typed/production C. Owner cases use
  explicit expected results because the AST oracle rejects owners. C compiles
  in strict C11 at `-O0` and `-O2`, including the 32-call success/33rd-call trap.
- Existing fallback tests now use actual record expressions rather than borrowed
  scalar helpers. The stale `owner_scopes` negative for a computed scalar read
  temporary is an explicit positive assertion; actual owner cycle, generic,
  implicit-transfer and double-move rejections remain covered.

Fresh focused runs passed 134 and 69 tests, covering 197 distinct tests after
six repeated owner-scope tests are counted once. This includes all 110 library
tests, existing field-loan fallback, C/backend, owner-transfer/cleanup and
fallible-allocation checks. Strict all-target Clippy passed in default and
`c-import` modes, as did formatting and diff whitespace checks. The fresh
recovery toolchain is Rust 1.99.1, GCC 14.2 and Clang 19.1.7 on x86_64 Linux.

Fresh independent review found no blocking issue. It reran 35 repository tests
and 26 additional source programs through typed/production execution, the AST
oracle where supported, GCC `-O0`/`-O2` with UBSan, and Clang `-O0`/`-O2`.
Seven owner cases checked exact allocation/free/trap traces, including injected
second-allocation failure. Eleven private IR/storage probes and twelve source
rejections passed. Direct success/trap probes verified that temporary loan
handles release and cells remain borrowable after calls. Clang UBSan could not
link because its compiler-rt archive was absent; GCC UBSan succeeded.

Full combined-checkpoint regression is integration work, and no full phase gate
is claimed. Remaining boundaries are documented in [ownership](../blockers/ownership.md),
[C layout](../blockers/c-adt-layout.md) and [stack safety](../blockers/stack-safety.md).
