# Closed result annotations: integrated acceptance

4 November 2026. The bounded annotation increment is accepted on exact source
`93b1188bb5c3b771e6adc6192f46b200b573d9b5`, tree
`4057da64ea27762c00458f8c73c93d214080de27`. The following documentation checkpoint
changes no executable source, examples or tests. This is not completion of P1,
C02, general annotated interfaces, or the full language plan.

## Accepted contract

Braced `fn` and `export fn` may constrain their result with `-> TypeExpr`, using
the existing closed parameter-type vocabulary. An annotation is an equality
constraint, not a cast, default type or new storage representation. All annotated
results in a recursive group constrain monomorphic placeholders before any body
is inferred. Existing parameter timing, unannotated inference, generalization,
legacy syntax, unused-declaration checking and module privacy remain intact.

Borrowed input and resolved-name passes bound complete result trees across the
module graph before cloning. Public programmatic ASTs receive the early result
preflight independently of parser depth limits. The policy permits depth 128,
16 MiB logical weight per tree, 64 MiB cumulatively and 4,000,000 visits; each
node costs 64 units plus its name bytes. It is not a whole-compiler memory bound.

The distinct FunctionResult source role survives parsing and module rewriting.
Written types have SignatureOnly dispositions; generated checked result slots
have exact definition/body/annotation relations. Full logical identity remains
mandatory under contextual lowering, independently of optional provenance.

## Fresh aggregate evidence

All seven serial stages passed from 11:41:21 through 11:57:36 UTC:

- Formatting.
- Default tests: **1,034 passed**, zero failed or ignored.
- Default strict all-target Clippy.
- Importer-enabled tests: **1,082 passed**, zero failed or ignored.
- Importer-enabled strict all-target Clippy.
- Windows GNU all-target compilation, default features.
- Windows GNU all-target compilation with `c-import`.

Neither test mode reported a SKIP notice. The before/after ref snapshots are
byte-identical; HEAD and tree remained exactly those identified above. The
Windows commands are `cargo check` only: Windows execution is not claimed.

```sh
cargo fmt --all -- --check
cargo test --locked -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo check --locked --all-targets --target x86_64-pc-windows-gnu
cargo check --locked --all-targets --target x86_64-pc-windows-gnu --features c-import
```

RUST_TEST_THREADS=1, two Cargo jobs, zero dev/test debug information and disabled
incremental compilation were used. Fixed guards checked two-GiB filesystem and
three-GiB MemAvailable floors. The new annotation harnesses additionally guard
C compiler invocations. The independent serial failure probe verifies exit 86,
zero subsequent compiler calls and fail-fast orchestration; it is not a parallel
or cross-process cancellation guarantee.

## Independent and native checks

The [fresh independent record](2026-11-04-result-annotations-independent.md)
passes 664 selected GCC tests and eleven Clang annotation tests. Its own public
matrix executes 68 C11 cases, while the repeated author matrix separately executes
280. Both use GCC/Clang at O0/O2 with `-pedantic-errors`; four deliberately invalid
unused-header builds reject. Repeated runs and older harness executions are not
added to these counts. Private corruptions reject incorrect logical identities,
runtime spans and annotation roles. The CLI example prints `42`.

These checks cover mixed-SCC permutations, native declaration/header parity,
pure versus reached-native evaluation, owner obligations/cleanup, exact origins,
module namespaces/privacy, contextual logical results and hostile public ASTs.
Twelve existing Clang equality-parentheses warnings are retained, not hidden.

Toolchains are Rust/Cargo 1.99.1, GCC 14.2.0, Clang/libclang 19.1.7 and SDL3 3.2.10
on x86_64 Linux. The supported configured libclang setup explicitly propagates
RILL_TEST_CLANG_RESOURCE_DIR as `--clang-resource-dir`, supplying the relocated
Clang resource headers. This does not assert working raw default header search.
Direct real-SDL3 execution passes in both test modes; generated SDL3 bindings
also run with the importer enabled. No new framebuffer captures, Zig, AArch64,
GPU rendering or physical-input verification is claimed.

## Limits and recovery provenance

Kinds, skolems, higher-rank/polymorphic-recursion interfaces, local ascriptions,
genuinely unknown stored layouts and broader owner/native combinations remain
outside this increment. Optional Copy-body provenance retains its closed-type,
owner/native and contextual/opaque exclusions. Raw C pointers gain no checked
lifetime or automatic foreign-resource cleanup; the inherited owner-name
classification boundary is preserved.

Two environment losses preceded the fresh checks. The author-stage reports and
older reported results remain historical; their lost raw artifacts are not
claimed to survive. The independently restored `3730911` source received only
the reconstructed test alias and honest report at `93b1188`, then underwent the
new independent and aggregate runs. The lost intermediate `7361c986` commit is
not represented as durably recovered.

The aggregate evidence set is identified by
`integrated-93b1188bb5c3-20261104T114121Z`. It contains status.tsv, seven stage
logs, exact source/tree identities, version/tooling hashes and before/after refs.
The frozen current environment SHA-256 is
`6ad4935f5be1872a6725f741d733d0a4a30f7394d660a4b39d25e0610933c3c6`.
The final documentation-only changes do not extend these acceptance claims.
