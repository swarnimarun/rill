# Closed result annotations: first author checkpoint

4 November 2026. This is a small, author-tested durability checkpoint on the
accepted direct-record base `dc470611ab886cb45149bc59709914da5de59992`.
Independent review, the expanded execution/boundary matrix and full aggregate
acceptance are pending. It does not complete P1, C02 or annotated interfaces.

Subsequent [expanded author work](2026-11-04-result-annotations-expanded.md)
corrects two evidence gaps in this historical checkpoint: its module budget reset
per file, and its compatibility fingerprints compared quantifier counts rather
than identities. Read the newer report for the graph-wide policy and stronger
captured baseline. The results below describe only the first spine.

## Implemented spine

Braced `fn` and `export fn` accept optional `-> TypeExpr`. The existing closed
parameter-annotation resolver is reused with an empty variable environment.
Annotations are equality constraints, not casts or fabricated defaults. Every
result annotation in an SCC constrains its monomorphic placeholder before any
body is inferred. Existing parameter-annotation timing and generalization are
unchanged. Legacy `let` functions do not gain annotation syntax.

A borrowed actual-tree preflight precedes result resolution, owner scanning and
module function cloning/rewriting. Public programmatic ASTs do not rely on the
parser's separate depth guard. The preflight permits depth 128, logical weight
16 MiB per result tree and 64 MiB cumulatively, and 4,000,000 node visits. Each
node costs 64 logical units plus its name's byte length. These are policy units,
not a whole-compiler memory guarantee. Traversal retains one child iterator per
depth, charges before growth, and returns `E-TYPE-COMPLEXITY` on exhaustion.
Existing declaration/parameter annotation policies are not expanded here.

The distinct `AstStep::FunctionResult` occurs after parameters and before the
body. The shared parser/CST, exact AST map, module rewriting and module origins
retain its complete nested type tree. Checked function instances keep their
existing full parameter/result identity. Copy-body annotation nodes have
`SignatureOnly` dispositions; the result slot has `AnnotatedResultType` with
exact ordered definition/body/annotation relations. The existing complete
function-signature validator checks the result type. No new result storage,
control representation, owner/native support or source-map coverage is added.

The inherited owner classifier also examines raw C pointee spellings. Both
parameter and result annotations containing `ptr<SystemBytes>` or
`const_ptr<SystemBoxI32>` retain that spelling-sensitive owner rejection. Tests
reproduce this boundary; this checkpoint does not claim to repair it.

## Focused checks

The final focused command passed **560 tests** (483 library tests and 77
integration tests):

```sh
cargo test --locked --lib --test result_annotations \
  --test result_annotation_origins --test type_solver --test brace_syntax \
  --test ast_source_map --test module_origins --test typed_node_origins \
  --test typed_source_origins --no-fail-fast
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
```

The matrix includes observed/mutable closed options, primitive/static-string/unit
results, both declaration orders of a field-informing recursive SCC, generic
parameters with closed results, constrained identity, unused mismatches and
unsupported syntax. A 10,000-deep programmatic annotation is rejected before
recursive consumers; the test dismantles its caller-owned tree iteratively.
Tiny injected preflight budgets cover depth, logical weight, cumulative weight
and work. Six private corruptions reject parameter/result swaps, nested-child
substitution, wrong relationship role/reason, duplicate relations and omitted
nested annotation dispositions.

Module tests check exported nested result identities, exact ordinary/mapped AST
and generated-C equality, direct/nested private leaks, exact annotation roles,
UTF-8/comments and formatter idempotence. New pure cases pass reference, default
and direct typed evaluation and typed/reference C emission. Existing selected
origin tests execute C, but **new annotation-specific C executions have not yet
been run**.

Before the first annotated rebuild, a small harness linked the retained default
library from the preceding accepted checkpoint. It captured canonical signatures
and rejection classes for twelve unannotated programs. The committed baseline
covers ordinary polymorphism, recursive schemes, mutable aliases, the existing
order-sensitive parameter-annotated SCC, raw-pointer obligations and the inherited
owner-pointee spelling boundary. The updated compiler matches every row. This is
a bounded compatibility corpus, not a universal inference-equivalence proof.

The first focused run passed 559 tests before the private mutation test was added.
Its first compilation then failed because the new test omitted an `AstStep`
import. Adding that test-only import produced the final passing 560-test run.
The failed and successful logs remain retained; no production failure was hidden.

All builds used the existing toolchain and sole serial target with two jobs,
zero dev/test debug information and incremental compilation disabled. Every build
passed the required persistent/RAM free-space and available-memory guards.

## Work deliberately pending

- Broader scalar/vector/nested/owner/native and control-boundary acceptance.
- New annotation-specific GCC/Clang C11 O0/O2 execution and exact event checks.
- Expanded hostile-AST/origin mutation and exact resource-boundary cases.
- Independent review and fresh default/importer, lint and Windows compile matrix.
- Final acceptance documentation and publication remain pending.

This checkpoint pauses for durable recovery verification before that expansion.
