# Checked finite Copy records

This bounded P1/P2 increment migrates finite Copy records onto the shared typed
interpreter/C path. It changes no source syntax and completes no phase. Owned
records, partial moves, field-loan operands, escaping views and full MIR
initialization/drop analysis remain separate work.

## Representation and semantics

The nominal registry distinguishes named records from positional enums. A
record stores declaration-order field names and one product of concrete field
types. Identity includes the resolved declaration and concrete type arguments,
including phantom arguments. The independent verifier checks shape, field
count, names, uniqueness and Copy ownership. Positional record construction
and patterns remain invalid. C emits tag-free structs in dependency order,
with a dummy byte for empty records rather than an invalid empty aggregate.
These private layouts are not a stable foreign ABI.

Construction preserves written evaluation order separately from storage order.
Field reads and assignment carry a root local ID followed by nominal record
IDs and field indices. The verifier rechecks every step, final type, scope,
mutability and active whole-root loans, retaining the 16-segment source limit.
It does not trust the source checker's projection result.

Assignment evaluates its right-hand side before resolving the target in the
original root slot. The right-hand side may replace the root or an intermediate
record; the eventual write preserves the replacement's other fields. Rust cell
borrows do not span evaluation or Rill calls. Ordinary reads, declarations,
value arguments and returns remain snapshots. Whole-record `read` and `inout`
parameters use existing root loans and forwarding, including concrete generic
instances. Computed read-record arguments materialize independent temporaries.
No C `restrict` or no-alias claims are added.

Copy records coexist with restricted owners, including Copy record parameters,
results and payloads of owner-bearing enums. Allocation, call-graph and cleanup
traversals visit record fields and assignment right-hand sides. Owned records
remain rejected until initialization and partial-move cleanup are implemented.
Owner generic/recursive and borrowed owner-enum restrictions remain unchanged.

Direct field operands to `read` or `inout` remain explicitly typed-unsupported,
not silently converted to computed temporaries. Copy programs retain the
checked fallback for these calls; owner programs cannot use copying fallback
and reject them. Unresolved local polymorphic record layouts also remain
outside this migration. Checker-side field-scheme instantiation is a separate
concern from elaborating consistent concrete whole-record and field uses.

## Recovery and fresh evidence

The earlier reviewed, uncommitted record implementation became unavailable
with its cloud filesystem. This is a reconstruction on the recovered source
and separately checked root-loan increment. Earlier test/review reports are
not evidence for these reconstructed bytes.

- Eight new source groups cover 27 successful/trapping programs across typed
  evaluation, production interpretation, identical typed/production C and the
  checked AST oracle where supported. Owner cases use explicit results because
  the oracle rejects owners. Generated C uses strict C11 at `-O0` and `-O2`.
- Cases cover empty/tag-free layouts, phantom record identity, computed read
  temporaries, named display, source-order side effects/traps, root/prefix
  replacement, generic functions/records, duplicate imported short names,
  nested enums/records, byte-buffer snapshots, UTF-8/NUL strings, unit fields,
  whole-record loans and the 16-step projection boundary.
- Boundary tests retain two actual field-loan fallback cases, positional and
  owned-record source rejections, owner/field-loan rejection and an unresolved
  local layout diagnostic. Existing typed-byte/C fallback fixtures now use
  actual field operands rather than a record declaration alone.
- Four private malformed-IR groups mutate record/enum shape, ownership,
  field names/counts, constructor completeness/types, local/nominal identities,
  path indices/depth, mutability and whole-root loan intervals. All three typed
  entrypoints reject malformed IR. A fifth private group checks written-order
  owner effects, normal cleanup and allocation-trap injection.

Fresh affected checks passed all 232 tests across the library and 30 integration
binaries, including 115 library tests, record/field fallback, owner cleanup,
fallible allocation, generic specialization and recursion regressions. Strict
all-target Clippy passed in default and `c-import` modes, as did formatting and
diff whitespace checks. The recovery toolchain is Rust 1.99.1, GCC 14.2 and
Clang 19.1.7 on x86_64 Linux.

Fresh independent review found no blocking issue. Twenty additional source
cases (18 successful, two trapping, including 16 AST differentials and four
owner cases) passed strict C11 with GCC at `-O0`/`-O2` and `-O2` with UBSan.
Fourteen source restriction checks, the 17-step and module nominal-identity
rejections, and two actual field-loan fallbacks passed. Forty forged-IR
mutations were rejected by verification, evaluation and C emission. Four
typed/native owner scenarios matched exact traces at every one of seven
allocation-failure positions. The frozen ten-file implementation manifest was
rechecked after testing. Clang UBSan was not run.

The full combined-checkpoint aggregate remains integration work. No P1/P2 gate
is claimed complete. Remaining boundaries are documented in
[ownership](../blockers/ownership.md), [C layout](../blockers/c-adt-layout.md)
and [stack safety](../blockers/stack-safety.md).
