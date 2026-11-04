# Recursive closed signatures: independent verification

4 November 2026. Independent review and selected checks pass on author production
source `abffdc9f76c5b3ba50040c352120a44e09ad6be4`, tree
`e26131c8ac337871c0a1272bf6aea9a42be8a502`, with the independently added
`tests/recursive_signatures_independent.rs`. No production change was needed.
This records the independent stage; subsequent full default/importer, strict
lint and Windows compilation results have [integrated acceptance](2026-11-04-recursive-signatures.md).
Source identities and counts below remain those of this stage. This is not P1,
C02 or full-language acceptance.

## Source review

The SCC change keeps monomorphic placeholders and the established result pass,
then constrains every closed parameter before any body. Body traversal, modes,
local binding creation and final generalization are unchanged. Components remain
sorted by source index. Annotations use the existing closed declared-type path,
not annotated recursive schemes or deferred field constraints.

The borrowed budget now visits every parameter and result tree. All six public
checker/evaluator/emitter routes enter this admission pass before recursive
consumers. The module source pass shares one allowance across the graph, and the
resolved-name pass shares a second full allowance before function cloning. Raw
pointer descendants remain unrewritten but are completely charged, including
malformed descendants and siblings. The changed generic diagnostic is explicit:
`E-TYPE-COMPLEXITY: signature annotation input budget exceeded`.

Reviewed private tests establish depth, UTF-8 byte weight, wide-tree work,
per-tree reset, cumulative parameter/result work and weight, multiple parameters,
legacy/unused functions, exact name growth and symbol-ID digit boundaries.
These budget checks run in the selected library suite. Owner-record support
is unchanged.

## Independently authored checks

Seven new public tests cover:

- All six orders of a three-function SCC combining a nominal result annotation,
  one directly annotated nominal parameter, other inferred parameters and a
  shared generic argument. Exact canonical schemes retain quantified-variable
  membership, and callers instantiate the generic argument at bool and i32.
- Inout field forwarding with whole-record replacement during an assignment's
  right-hand side. The exact result `74223` establishes the replacement's sibling
  field, the final x assignment and an untouched outer sibling.
- Value and computed-read snapshots of the same later-mutated root, yielding
  `20207`, plus once-only left-to-right argument/body mutation yielding `123123`.
  Both declaration orders are exercised.
- Competing errors that pin result constraints before all parameter constraints,
  and all parameter constraints before any body, after input admission succeeds.
- Mode/loan errors, nominal phantom identity, pointer spelling and constness,
  mutable-value restriction, and the unchanged sixteen-case historical scheme
  fixture, including its unannotated-result reject/accept pair.
- All six public routes at parameter depth 127/128/129/20,000, with the deep tree
  in a later parameter. Malformed ptr/const_ptr trees place a deep later sibling
  behind an already-invalid arity. Budget admission still precedes semantic
  rejection. Caller-owned deep trees are dismantled iteratively before assertions.
- Ordinary/snapshot/mapped module equality, alias-shadowed nested nominal
  parameters, exact Parameter/Annotation/TypeArgument paths versus FunctionResult,
  equal resolved type identity with distinct source roles, and unused private or
  wrong-module nominal parameters.

Fourteen source/module cases execute both typed and uniform reference C under
GCC and Clang, C11 O0/O2 with `-pedantic-errors`: **112 C executions**, 56 per
compiler. Three evaluators agree, and direct typed C equals default output.
Four existing Clang equality-parentheses warnings are retained. The initial
independent run had two fixture parse failures because call-result field syntax
was not supported; explicit local lets corrected the fixtures. That partial run
and repeated executions are retained but excluded from 112.

## Selected regression and resource evidence

The final selected GCC run passes **715 tests** in 25 test binaries, zero failed
or ignored and no SKIP notices. Strict default all-target Clippy, formatting and
diff checks pass. Its 56 repeated independent C executions and 88 repeated author
executions are not added to the independent total above. Existing source and test
file hashes are unchanged; only the new independent test was added. Git refs
remained unchanged throughout testing.

Builds use two Cargo jobs, zero dev/test debug information, disabled incremental
compilation and serial tests. Fixed guards enforce two GiB filesystem and three
GiB available-memory floors. The new harness guards both C compilation and binary
execution; selected legacy compiler invocations use a guarded GCC wrapper. No
actual resource floor failed.

Independent injected failures test both the author and independent harnesses.
A first-guard failure produces exit 86 after one guard and zero compiler calls.
A pre-execution failure produces exit 86 after two guards and one compiler-spy
call. All four probes record zero generated-binary launches and no subsequent
shell marker. These are serial control-flow checks, not real C executions or
cancellation guarantees for already-running parallel children.

Toolchains are Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7 on x86_64 Linux.
Frozen environment SHA-256:
`6ad4935f5be1872a6725f741d733d0a4a30f7394d660a4b39d25e0610933c3c6`.
Independent test source SHA-256:
`26931ac5b210cced5426d861e277d8b5403cb82feb665c9138b97c1ca437956f`.
Independent test executable SHA-256:
`7d28928ccecb4753053c63e0eb5ae90d3ca191ee32c9e856eb731549c558c67c`.

The evidence set `rill-signature-independent-20261104T123731Z` retains fixture
sources, both C forms, compiler/runtime/guard output, initial failures, source/ref
snapshots and hashes. Key records are `04-gcc.log`, `05-clang.log`,
`07-guard-probes.json`, `08-selected-gcc.log`, `09-clippy.log`, `10-fmt.log`,
`summary.json` and `test-binaries.sha256`.

## Reproduction and limits

Run the author's documented selected regression command with the additional
`--test recursive_signatures_independent`. The fresh compiler dimension is:

```sh
CC=gcc cargo test --locked --test recursive_signatures_independent -- --nocapture
CC=clang cargo test --locked --test recursive_signatures_independent -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

Set RUST_TEST_THREADS=1 and K4_RESOURCE_GUARD for guarded serial execution.
RILL_SIGNATURE_EVIDENCE_ROOT can select an existing retained fixture directory.
This review does not establish universal inference equivalence or source-order
independence. Unannotated callee-result projection and unconstrained intermediate
parameter propagation remain limited. No new representation, optional-origin
coverage, ownership, raw-pointer lifetime, foreign ABI or effect guarantee is
claimed. No Windows execution, new SDL3 capture, Zig, AArch64, GPU or physical-input
evidence is added.
