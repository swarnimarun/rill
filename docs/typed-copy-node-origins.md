# Copy function-body origins

Bounded implementation, verified 3 November 2026. Combined native integration,
independent probes and the full default/importer-enabled matrix passed on source
`e805e3cf995fc29b1f5bdf29bd82d0bcb8b59942`. P1, C18 and complete typed/C mapping
remain open; see the [integrated verification](verification/2026-11-03-copy-node-origins.md).

`typed::provenance::lower_main_with_copy_node_origins(SourceProgram)` is a
separate opt-in API. It owns the exact source snapshots, ordinary verified typed
result, existing function-instance index and validated body metadata. Ordinary
`lower_main`, backend selection, evaluation and generated C are unchanged. The
older `lower_main_with_function_origins` retains its accepted owner subset.

## Coverage and exclusions

The capability is `Coverage::CopyFunctionBodies`. It includes every reached
function root, parameter, inferred result slot, expression, call argument,
record-field tuple, Copy arm, recursive Copy pattern and embedded `bytes_new`
capacity operand. Stored field order and declaration indexes remain separate.
Root/field loan arguments have no fabricated child `Read`; computed read slots
have their actual evaluated expression child and no invented local ID.

Coverage includes pure scalars, static UTF-8 strings, fixed bytes, vectors,
finite Copy records/enums, concrete recursive function instances, root/field
loans, conditions, loops, assignments and existing local capture/shape
elaboration. Every checked owner obligation is excluded, including obligations
in unreachable functions and inactive variants. Native declarations, includes,
raw runtime values and native operations are excluded. The final structure walk
also checks stored registry dependencies and function signatures rather than
assuming that reached expression kinds prove this boundary.

No rows claim complete layout, source-binding/storage, owner initialization,
cleanup, native ABI, package/directive or generated-C byte coverage. Source type
declarations queried through this body API are `OutsideCoverage`. The existing
function index retains `NotInstantiated` for unused definitions. No unknown or
unavailable origin row appears inside a successful result.

## Snapshot identities and queries

`TypedNodeId`, `ShapeId` and `RelationSetId` are separate opaque handles sharing
the exact wrapper's private snapshot domain. New lowering receives a new domain,
even for identical bytes. Moving or cloning the whole immutable wrapper retains
its domain. Foreign handles fail lookup; no API attaches these tables to edited
IR. `into_typed` and `into_function_origins` deliberately discard body metadata.

Node ordinals follow final function-instance order, then function root,
parameters, result and semantic body preorder. Match children are scrutinee then
each pattern/body arm. Record fields retain written evaluation order. Hash-table
iteration, source addresses, fresh-local allocation and emitted names never
choose public node order. These are deterministic snapshot addresses, not stable
cross-edit identities or persistent cache keys.

`nodes`, `node`, `children`, `typed_node`, `shape`, `relations`, `source_nodes`
and `source_representation` are read-only. `source_nodes` enumerates branded
source IDs, including erased bindings, annotations and declarations outside
this API's body coverage. The typed node accessor borrows the exact
owned IR by validated parent/child links. Raw `Type` and `LocalId` attributes
remain compilation-local values. Source queries reject the wrong instance,
foreign source handle or a source from another function. `ancestors` takes an
explicit maximum output count; query work never consumes the compilation budget.

A direct node has `NodeOrigin::SourceDerived` with its exact flattened AST
occurrence and rule. `direct_source` returns the complete `ResolvedNode`, keeping
all parser/module source contributions. A unary-negation zero retains its
parser-generated provenance rather than claiming a written zero.

Generated nodes have a reason and ordered relation set, with no direct span.
Relations distinguish uses, source bindings, selected shape nodes and concrete
capture evaluations. Aliases share a finite shape DAG. Each actual materialized
output occurrence still has a separate typed identity.

## Capture and erasure correspondence

Concrete capture leaves retain their once-only initializer evaluation origins.
Generated capture environments, products and field tuples relate to their
specific source binding and ordered leaves. Deduplicated layouts do not merge
environment roots. Materializations relate to their own use and selected shape;
they do not claim to evaluate the initializer again.

A zero-capture let retains `StaticShapeOnly` and forwards the exact continuation
fragment unchanged. Constructors/records stored in shapes have
`RepresentedByShape`; alias occurrences have `AliasOfShape`. Parameter annotations
are `SignatureOnly`. Loan-only source operands and embedded capacity literals
have explicit `OperandOnly` and `EmbeddedOperand` dispositions.

Captured-record projections distinguish the generated temporary let, capture
read and source-derived final field use. The internal environment field does not
consume one of the source's sixteen permitted projection steps.

## Validation and limits

The new API performs one analysis, specialization, lowering and ordinary typed
verification. Construction fragments are optional observers at the existing
semantic producers. An independent iterative final IR walk checks every output
occurrence, exact attributes and child roles, consuming every fragment exactly
once. It rejects missing, duplicated and unreachable fragments. Ordered child
cursors and independently built request/binding indexes avoid repeated sibling,
request-graph or alias-binding scans.

Source validation checks exact parent/child routes, literal/operator/builtin
payloads, field indexes, final callee requests and closed types under the final
instance substitution. Independent lexical replay verifies ordinary parameter,
let and pattern binders and selected local roots. Separate shape validation
checks grammar, checked binder/use identities, DAG edges, capture order,
environment translations, materialization children and explicit erasure.
These checks establish metadata correspondence, not semantic preservation,
correct inference, correct cleanup or generated-C correctness.

Initial logical caps are 1,000,000 body metadata records and 4,000,000 body
edges, with one shared 8,000,000-unit work counter across request discovery,
source indexing, construction and final validation. The existing independent
1,000,000 request cap remains. Allocation/work reservations precede metadata
collection growth. Simultaneously retained construction and final tables are
both charged. String bytes, allocator overhead and source loading retain their
existing limits; these caps are not a byte-accurate memory guarantee.

Failures share a stop latch across request and body observers; both become
no-ops while ordinary lowering continues. The actual diagnostic is retained
rather than replacing a correspondence error with a generic limit error. Ordering
is ordinary semantic/lowering/verification errors first, explicit native/Copy
coverage boundaries next, and then `E-TYPED-SOURCE-MAP` or
`E-TYPED-SOURCE-MAP-LIMIT`. No partial wrapper is returned. Existing semantic
instance/layout/type-depth/local-shape/capture/frame limits are unchanged.

Generated C receives no comments, IDs, `#line` directives or other mapping
instrumentation. Acceptance requires byte equality, evaluator agreement,
optimized/unoptimized C execution, adversarial correspondence mutations, exact
limit tests and the full existing compatibility suites.

## Focused verification

On the pre-F2 source base, 28 private body/shape/structure tests, all eight
existing private function-origin tests, 16 public body-origin tests and 85 tests
across the eleven affected integration suites passed. Public tests compare
ordinary/reference execution and emitted C bytes; three representative cases
also execute generated C at both `-O0` and `-O2`. Formatting and strict all-target
Clippy passed. These are focused results, not the full release acceptance gate.

Corruption checks include valid same-typed let/pattern binder swaps, a valid
local-read change and a complete return-only function-instance substitution
whose refreshed construction fingerprints and edited IR pass ordinary
verification. Independent source correspondence still rejects the changes.
Tiny injected limits verify charge-before-growth and both directions of the
shared stop latch, while ordinary unresolved-type and local-shape-limit errors
retain precedence.

The F2 integration explicitly rejects native types and expression forms. Both
provenance APIs run the complete native-representation gate after ordinary
lowering succeeds and before metadata-error checks/finalization, including raw
types hidden in inactive fields or phantom arguments. On the combined source,
172 focused tests and 39 independent probes passed, followed by 661 default and
695 importer-enabled tests, formatting, both strict Clippy checks and both
Windows GNU compile checks. Windows binaries were not executed.


Ordinary typed compilation also supports the bounded
[nullary-phantom representation](typed-nullary-phantoms.md). This origin API
still exports closed types: after ordinary lowering and verification succeed,
opaque logical arguments produce an explicit `E-TYPED-UNSUPPORTED` mapping
boundary, including opaque metadata confined to bodies or capture shapes.
No concrete witness or partial successful index is substituted. Closed mapping
behavior and ordinary semantic-error precedence remain unchanged.


## Closed result annotations

Written braced-function result types and all nested arguments use the distinct
`FunctionResult` AST role and `SignatureOnly` dispositions. Their checked result
slot uses `AnnotatedResultType`, with ordered definition, body and annotation
relations. The annotation must be this function's actual result child, not a
same-shaped parameter type. Full closed result identity still passes the existing
function-instance validation. Unannotated result provenance is unchanged.
The bounded [integrated acceptance](verification/2026-11-04-result-annotations.md)
includes independent corruption checks and the full default/importer matrix.
Another function's same-shaped result annotation is rejected as the generated
result's source relation. Written annotations cannot claim direct runtime spans,
and unannotated functions cannot acquire an invented annotated-result reason.
The existing closed Copy-body scope, native/owner exclusions and contextual/opaque
mapping boundaries remain unchanged. The first-stage and expanded author reports
remain historical implementation records.
