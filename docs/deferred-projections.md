# Deferred nominal record projections

Status: accepted bounded increment with [independent review/execution](verification/2026-11-04-deferred-projections-independent.md)
and [full integrated verification](verification/2026-11-04-deferred-projections.md)
on source `81c8c151`: 1,103 default and 1,151 importer-enabled tests, both strict
lint modes and both Windows GNU compile-only modes pass. The historical
[author checkpoint](verification/2026-11-04-deferred-projections-author.md) and its
clean-source replacement remain separately documented. This does not complete
P1, P2, C02 or the full language plan. The [examples](../examples/deferred-projections)
use existing syntax.

## Constraint boundary

Every function component still installs monomorphic placeholders, all closed
results and all closed parameters, then checks bodies once in source order.
Unknown nominal field projections now retain an obligation until ordinary type
equalities within that component establish their nominal identities. This covers
field reads, assignment and read/inout field operands together.

Each occurrence retains one instantiated root, its exact field-name segments,
current intermediate type, projected result variable and canonical prefix.
Ready obligations are resolved before collecting local-generalization inputs.
A deterministic source-ordered worklist resumes only unresolved suffixes and
retries earlier dependencies when a solved projection supplies an equality.
All obligations must resolve before component generalization and before a
CheckedProgram is returned. Complete existing FieldInfo is published only after
the whole path resolves. Known nominal paths remain eager.

No record is selected by searching for matching field labels. Known non-records,
missing labels, incompatible nominal types, infinite type equations and unknown
roots still reject. For example, `fn project(p) { p.value }` is rejected if its
own component supplies no nominal constraint, even when a later caller passes
a record and only one declaration has that field. This is not a claim of universal
declaration-order independence. Other inference obligations retain their existing
boundaries and multiply-invalid programs may report a different first error.

## Generalization and loans

Field initializers are expansive, as are constructors containing a field read.
Existing weakening and the independent environment/level test remain in place.
Aliases cannot regain weak projection variables. Unrelated eligible variables
still generalize, including an independent payload in `Pair(alias,None)`. Once
the component is solved, ordinary SCC generalization resumes without permanent
projection-specific monomorphism. No deferred local schemes were added.

Source loans retain binding IDs and complete field-name segments until canonical
indices are available. Root paths overlap all fields, prefixes overlap descendants,
and different segments are siblings (`a` is distinct from `ab`). Comparisons do
not concatenate paths or infer raw-pointer pointee independence.

Exclusive field access requires a mutable, empty-scheme root. Its one monotype
fixes every nominal prefix and the corresponding name/index mapping. Quantified
local roots remain immutable and unborrowed, so they permit only shared field
reads; their existing typed local shapes materialize exact-use read temporaries.
Whole-root operations still overlap all instances. This argument must be revisited
before relaxing those scheme/mutability restrictions. Source shadowing uses binding
identity, not spelling. Independent typed record and loan verification continues
to check canonical nominal/index paths, modes, authority and overlap.

A read operand is inferred exactly once. Loan preflight reads its binding ID and
source path without creating a second projection instance. Left-to-right effects,
value snapshots, computed read temporaries and original-slot writes are unchanged,
including replacement of a root or prefix during an assignment's right-hand side.

## Resource policy

Per analysis, projection and field-loan checking permits:

- 1,000,000 work units, including eager projection path/name admission, unresolved
  attempts, schema lookup and substitution-name preflight, field-loan scan entries
  and complete-segment comparisons. Name work charges UTF-8 bytes before the work.
- 4,096 deferred obligations admitted cumulatively, even after earlier entries resolve.
- 1 MiB of cumulatively retained field-path name bytes for obligations and source
  field loans. Immutable shared paths avoid re-cloning strings in loan snapshots.
- The existing 1..16 source field segments per path.

The counters do not reset when an obligation or loan ends. Admission precedes
new retained name clones and result-variable allocation. Existing solver type
materialization limits apply independently. Unchanged root-only loan comparisons
are not charged to this new field-path budget. Exhaustion reports
`E-PROJECTION-COMPLEXITY: projection and field-loan checking budget exceeded`.
These deterministic policy units are not allocator measurements or a whole-compiler
memory/time bound. Oversized eager paths, schemas or field-loan work can newly
reject even without deferred obligations.

## Compatibility and execution limits

Compatibility corpus case 5 intentionally changes from E-RECORD to the same exact
canonical schemes as case 6. The existing three-function intermediate-parameter
negative becomes positive. All other corpus entries and quantified-variable
membership remain frozen. Historical annotation/signature reports describe their
original checkpoints and are not rewritten as evidence for this increment.

Supported Copy cases use existing typed interpreter/C output; no physical layout,
owner or native ABI form is added. The checked reference interpreter remains a
differential oracle. Its separate uniform C backend still cannot represent local
polymorphic shapes: the unrelated-None, selective-Pair and polymorphic-read fixtures
explicitly retain its `E-C-ADT` local-specialization rejection while exercising
production typed C. Other new C-compatible fixtures exercise both backends.

Unknown physical storage, owned records, broader owner/native combinations,
kinds, skolems, structural records, higher-rank interfaces, polymorphic recursion,
borrowed returns and broader optional provenance remain outside this contract.
