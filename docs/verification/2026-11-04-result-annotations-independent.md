# Closed result annotations: independent verification

4 November 2026. **Current status: fresh independent checks pass.** The separate
[integrated record](2026-11-04-result-annotations.md) records bounded acceptance.
A second environment loss removed the preceding recovery workspace, including
its raw evidence and local commit
`7361c986f26ae16c41aac0d2d22c5de4766df086`. That commit is not durably recovered. The current checkout was independently restored from verified
`3730911edc13bdfa1b8116ac3bc1a9a692d57f22`, tree
`651ccf28dec7ca0cf6dd8751700e31c448d452ae`.

The small Consumer test type alias and the historical portion of this report
were reconstructed from the retained conversation, not recovered from the lost
commit. No production behavior changed. Fresh second-recovery checks ran on reconstructed checkpoint
`93b1188bb5c3b771e6adc6192f46b200b573d9b5`, tree
`4057da64ea27762c00458f8c73c93d214080de27`, with initial/final source hashes
unchanged. The historical results must not be presented as fresh evidence. Publication, P1, C02 and full-language
acceptance are not established by this record.

## Fresh second-recovery results

The final selected GCC suite passes **664 tests** (489 library and 175
integration). Eleven annotation execution tests also pass with Clang. The five
independent public tests execute **68 C11 programs**, 34 per compiler, evenly
split O0/O2. The freshly repeated author harness separately executes **280**,
140 per compiler. Four deliberately incompatible unused-header builds reject.
These counts exclude historical and repeated runs and do not add the Clang
repeats to the count of distinct tests.

All three independently authored private corruptions pass within the library
suite. Public checks retain the SCC permutations, logical identities, exact
origins, native/owner boundaries and public-AST depth cases described below.
Formatting, default strict all-target Clippy and the example output `42` pass.
Clang retains twelve `-Wparentheses-equality` warnings, with no test failure.
No further executable source change was needed after the reconstruction.

The intentional failing-guard probe passes on the freshly compiled binary:
exit 86 after one guard invocation, zero compiler-spy calls and no subsequent
orchestration marker. Tests run serially with RUST_TEST_THREADS=1 and a fail-fast
shell. This verifies serial stopping only, not cancellation of already-running
parallel children or other processes. No actual capacity guard failed. Builds
used two jobs, zero debug information, no incremental compilation and the fixed
two-GiB filesystem/three-GiB available-memory floors.

The current toolchain is Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7.
Current environment SHA-256:
`6ad4935f5be1872a6725f741d733d0a4a30f7394d660a4b39d25e0610933c3c6`.
Fresh default rlib SHA-256:
`9060b26aa09f1dbdf161225aef3c019aa4f52acdc2087fe43c06fe2569daf3cd`.
Fresh independent test binary SHA-256:
`11d1a2e345ed7cc2e86d90d36259d11acfc62a8d71122a36307b682f8ff9e9ac`.
The reconstructed test source matches the historical source hash listed below.

Current evidence includes `03-selected-gcc.log`, `04-clang.log`,
`05-clippy.log`, `06-fmt.log`, `07-example.out`, `08-source-integrity.log`,
`fresh-final-summary.json`, `build-artifacts.sha256` and
`latch/verification.txt`, plus guards, compiler diagnostics and retained fixtures.
These newly produced files are distinct from the unavailable older evidence.

## Historical verification from the first recovery

The first recovery also started from `3730911`, which includes author
implementation `936ea919` and the independent tests. Its only subsequent
executable-source change was a local test type alias required by strict Clippy.
That exact bounded change is reconstructed here.

### Earlier environment and compatibility evidence

The original environment's raw logs, libraries, executables, fixtures and helpers
were already unavailable after the first replacement. Its committed reports and
source tests were recovered. The earlier record reported five public GCC tests
and 34 C executions passing, a fixture-name collision before Clang C execution,
an interrupted GCC rerun with 16 C executions, and two private tests covering
three corruptions. Those results were not added to the first recovery's counts.

The old pre-annotation library hash and sixteen-row capture described in the
author report remain historical evidence. No historical rlib was invented or
replaced with a new library. First-recovery tests compared the restored,
committed canonical scheme expectations, including quantified membership. This
remains a bounded compatibility corpus, not a universal inference-equivalence
proof.

### Source review

The result-only SCC pass preserves parameter timing. Borrowed input budgets span
the complete module graph, and a separate resolved-name pass charges exact name
growth before annotation cloning. Raw C pointee spelling follows the existing
bypass and conservative owner-classifier boundary.

Mandatory contextual signatures preserve complete logical result identity,
independently of optional origin maps. Result origins require the actual
FunctionResult child, not an identical parameter type, nested child, or another
function's result. No new storage, owner-record, foreign-lifetime or optional
origin coverage is implied.

### First-recovery execution, now historical

The final selected GCC invocation passed **664 tests**: 489 library and 175
integration tests. The Clang invocation passed the eleven author/independent
annotation execution tests. These were repeated compiler dimensions, not 675
unique tests.

- The five independent public tests executed **68 C11 programs**: 34 GCC and
  34 Clang, split evenly between O0/O2, with `-pedantic-errors`.
- The author execution harness completed **280 C executions**: 140 per compiler,
  also split O0/O2. Earlier runs and C executions from older regression harnesses
  were not added to either count.
- Four deliberately incompatible unused-header builds were rejected, one per
  compiler/optimization combination. These were compile-fail checks, not runs.
- The two independent private tests passed all three corruptions. They relabel
  phantom witness expression and function/instance types coherently despite equal
  physical tags; claim a written annotation as a direct runtime origin; and invent
  an annotated-result reason for an unannotated function. Ordinary consumers or
  the independent optional-origin validator rejected them as appropriate.
- Formatting, strict default all-target Clippy, diff checks and the CLI example
  passed. The example printed exactly `42` and a newline.

Public coverage included all six mixed-SCC declaration permutations, contextual
nested phantom results, retained layout/owner exclusions, byte-identical inferred
and annotated native C, reached-native rejection versus pure evaluator paths,
unused native header obligations, module-alias parameter shadowing, exact result
roles and foreign-snapshot handle rejection. All six public checker/evaluator/
emitter entrypoints accepted programmatic annotation depths 127/128 and rejected
129/10,000 with the exact annotation-budget diagnostic. Deep caller-owned test
trees were dismantled iteratively.

The first strict lint run rejected the function-pointer array spelling with
`type_complexity`. A local Consumer alias fixed the test without suppressing lint.
The complete selected suite, both annotation compiler matrices, latch probe and
lint were rerun after that change. Clang emitted twelve existing
`-Wparentheses-equality` warnings. No production defect was found in those bounded
checks. Their raw diagnostics were subsequently lost with the first recovery.

### Historical resource-latch verification

Runs used RUST_TEST_THREADS=1, two Cargo jobs, debug information disabled and
incremental compilation disabled. Guards checked two-GiB filesystem and three-GiB
MemAvailable floors before builds and the new harnesses' C compiler invocations.

An intentionally failing guard was tested against the final compiled independent
binary with a compiler-spy wrapper and a shell using `set -euo pipefail`. The test
process exited **86** after exactly one guard call. The spy recorded **zero C
compiler calls**, and the subsequent orchestration marker was absent. This
established the tested serial fail-fast behavior, not cancellation of children
already running in parallel or in another process. No actual capacity guard
failed in the first recovery's verification. The current recovery reran this
probe before its acceptance matrix, as recorded above.

### Historical toolchain and artifact identities

The first recovery used Rust/Cargo 1.99.1, GCC 14.2.0 and Clang 19.1.7. Its direct
environment SHA-256 was
`6ad1ee1d516cdc4ee1568f5cafa2b9e89e3f09dd8664e0ae76626a1554ad70c3`.
Its default rlib SHA-256 was
`31d304aaea52c2dc78777a7d6afb48ce36efba5e5b57e7c111de550fafb0d6ea`.
Those artifacts are lost. The independent test source SHA-256 after the local
alias correction was
`781016fd3ce23014a4fdc4aebeb760b771f2a485dbcf95b653a20b110fa9d9c6`;
this source can be checked directly after reconstruction.

The first recovery's final logs were named `10-final-selected-gcc.log`,
`11-final-clang.log`, `12-final-clippy.log`, `13-final-fmt.log`, and
`latch-final/verification.txt`. It also held example output, source/artifact hashes,
guards, warnings, expected failures and generated fixtures. None of those raw
files is claimed to survive the second loss. Their reported results above are
preserved as historical conversation evidence only.

## Reproducing the second-recovery checks

The commands below followed the intentional serial guard probe, with a new
evidence directory, serial tests, fixed resource floors and fail-fast orchestration.
Failures and warnings are preserved. Importer-feature, Windows and full aggregate
results are recorded separately in the integrated acceptance report.

```sh
CC=gcc cargo test --locked --lib --test result_annotations \
  --test result_annotation_origins --test result_annotation_execution \
  --test result_annotations_independent --test ast_source_map \
  --test type_solver --test value_restriction --test typed_discarded_results \
  --test typed_discarded_records --test typed_local_values \
  --test typed_node_origins --test typed_source_origins --test typed_foreign \
  --test returned_owner --test owner_adt -- --test-threads=1 --nocapture
CC=clang cargo test --locked --test result_annotation_execution \
  --test result_annotations_independent -- --test-threads=1 --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -- run examples/result-annotations/main.rill
```
