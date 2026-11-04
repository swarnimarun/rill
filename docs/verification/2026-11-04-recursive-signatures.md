# Recursive closed signatures: integrated acceptance

4 November 2026. The bounded signature increment is accepted on exact source
`31cac927305e22be3e28d62c3e9f37fdcb9446c1`, tree
`395bc349d40010745012f4cee4d1486b20edd856`. All seven aggregate stages pass.
The following documentation-only checkpoint changes no executable source, tests,
examples or build inputs. This does not complete P1, C02, general annotated
interfaces or the full language plan.

## Accepted contract

Each recursive group receives monomorphic placeholders, all closed result
constraints, all closed parameter constraints, then source-order body inference
and ordinary generalization. Existing modes, loans, value restriction, snapshots,
nominal and pointer identity, module privacy and source-role distinctions remain.
The original f-first annotated-callee example now evaluates to `42` in both
declaration orders. See the [contract and example](../recursive-signatures.md).

Borrowed input admission and a separate exact resolved-name pass each share an
allowance across their entire program/module graph before recursive consumers
or annotation cloning. Each pass permits depth 128, 16 MiB logical weight per
tree, 64 MiB cumulative weight and 4,000,000 visits. A node costs 64 units plus
its UTF-8 name bytes. Parameters and results now share these totals, so oversized
parameters or combined annotations can newly reject. Raw pointer subtrees retain
their spelling while all malformed descendants remain charged. This is not a
whole-compiler resource bound.

The diagnostic is now
`E-TYPE-COMPLEXITY: signature annotation input budget exceeded`.
Exact-text result tests explicitly migrated from the former result-only wording.
Earlier parameter constraints can change error precedence in multiply-invalid
programs. Universal acceptance equivalence or inference-order independence is
not claimed.

## Fresh aggregate evidence

Seven serial stages passed from **12:49:04 through 13:11:52 UTC**:

- Formatting, 12:49:04–12:49:06.
- Default tests, 12:49:06–12:58:01: **1,065 passed**.
- Default strict all-target Clippy, 12:58:01–12:58:02.
- Importer-enabled tests, 12:58:02–13:10:48: **1,113 passed**.
- Importer-enabled strict all-target Clippy, 13:10:48–13:11:14.
- Windows GNU all-target compilation, default, 13:11:14–13:11:35.
- Windows GNU all-target compilation with c-import, 13:11:35–13:11:52.

Both test modes report zero failed or ignored tests, SKIP notices or warnings.
Before/after ref snapshots are byte-identical; the source remained clean with
the exact HEAD and tree identified above. The summary's retained log hashes and
raw test totals were independently checked before documentation finalization.
Windows checks are compile-only; Windows binaries were not executed.

```sh
cargo fmt --all -- --check
cargo test --locked -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo check --locked --all-targets --target x86_64-pc-windows-gnu
cargo check --locked --all-targets --target x86_64-pc-windows-gnu --features c-import
```

Tests used GCC, RUST_TEST_THREADS=1, two Cargo jobs, zero dev/test debug
information and disabled incremental compilation. Fixed guards checked two-GiB
filesystem and three-GiB MemAvailable floors before and after each stage. The
new signature C harnesses additionally guard compilation and binary execution.
No actual resource floor failed.

## Separate focused evidence

The [author record](2026-11-04-recursive-signatures-author.md) reports **708
selected tests** and **176 GCC/Clang C11 O0/O2 executions** on `a895550`.
The separate [independent record](2026-11-04-recursive-signatures-independent.md)
reports **715 selected tests** and **112 C executions** across fourteen cases.
These overlapping selected Rust suites and repeated C invocations are not added
to the aggregate test totals. Four independent Clang equality-parentheses
warnings are retained; they are distinct from the warning-free GCC aggregate.

Focused checks cover all six SCC declaration orders, exact canonical schemes
and quantified membership, constraint-error phase ordering, snapshots,
original-slot replacement, once-only effect order, module identity/privacy,
exact parameter/result source roles and hostile public-AST admission. Private
budget cases cover mixed weight/work totals, malformed raw descendants and exact
graph-wide resolved-name growth. Existing owner regression coverage remains;
owner-record support is unchanged.

Independent injected failures in both new harnesses produce exit 86: the first
guard prevents all compiler calls, and the pre-execution guard permits one
compiler-spy call but no generated-binary launch. No subsequent shell marker is
written. These are serial control-flow checks, not real C executions or
cancellation guarantees for already-running parallel children.

## Environment, native checks and retained evidence

The environment uses Rust/Cargo 1.99.1, GCC 14.2.0, Clang 19.1.7 and SDL3 3.2.10
on x86_64 Linux. Real direct SDL3 execution passes in both test modes; generated
real SDL3 bindings execute in importer mode. Configured libclang tests retain
the explicit RILL_TEST_CLANG_RESOURCE_DIR/`--clang-resource-dir` setup rather
than assuming relocated headers work through raw default search.

The evidence set is `integrated-31cac927305e-20261104T124904Z`. It retains
`VERIFIED_SUMMARY.json`, `status.tsv`, seven stage logs, source/tree identities,
version/tooling hashes and before/after refs. The frozen environment SHA-256 is
`6ad4935f5be1872a6725f741d733d0a4a30f7394d660a4b39d25e0610933c3c6`.
The author and independent reports retain their own source and executable hashes,
initial unsuccessful probes, warning records and separately counted matrices.

## Limits

The historical sixteen-case scheme fixture retains its unannotated-result
projection reject/accept pair. Unannotated callee-result projection and propagation
through unconstrained intermediate parameters still need separate work. Kinds,
skolems, higher-rank/polymorphic-recursion interfaces, unknown physical storage,
owner records, effects and broader foreign/optional-origin coverage remain open.
Raw pointers gain no checked lifetime or automatic foreign-resource cleanup.
No new framebuffer captures, Windows execution, Zig, AArch64, GPU or physical-input
evidence is claimed. This acceptance is limited to the documented increment.
