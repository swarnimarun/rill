# Independent discarded-result verification

Date: 2026-11-04 UTC.

## Exact candidate and test changes

- Production/test candidate reviewed: `ee5763f16686cf5a67decc80dd78979fa8d3ec80`, tree `3f9589dd1e8d62d0765e0332be34c68504ec8ac0`.
- Execution handoff: docs-only HEAD `83c05ba415f4e781d90fe33903c18c78930da05a`, tree `cceaaf897caf2f090a9765186c821fd01048999e`.
- Accepted independent tests: `94f2048f092a76ad65fb08847649ae47912ec897`, tree `c7f6a52733f840dc726528012b20d0a08ab654e6`.
- Independent changes are test-only: five private test modules, their `#[cfg(test)]` declarations, two standalone public probe sources, and a resource guard. No production implementation was edited during independent execution.

## Results

- 25 independent private/source-relation tests passed, repeated after formatting. The executable contains 443 library tests in total; this independent run selected 25 and filtered out the other 418. Full aggregate execution is a separate gate.
- 35 explicitly typed public cases passed. Each requires successful checked lowering, standalone verification, typed evaluation equal to the reference interpreter, deterministic typed C, and default backend C identical to typed C. Reference fallback is never counted as typed support.
- 12 typed unsupported-boundary cases and 8 source-checker negatives passed.
- 212 C compilations and executions passed in the final matrix: GCC and Clang, each at `-O0` and `-O2`, using `-std=c11 -pedantic-errors`. These are 35 ordinary programs plus 18 separately instrumented exact event-trace variants, each in four compiler/optimization combinations.
- The complete C matrix also passed before final formatting, for 424 retained executions across the two runs. The unique final-matrix count remains 212.
- Six optional-origin cases passed through both function-origin and copy-node-origin APIs. Two additional type-invalid sources were correctly rejected earlier by the module loader.
- `cargo fmt --all -- --check` and default `cargo clippy --all-targets -- -D warnings` passed.
- No production defect remained in the independent execution matrix.

The public probes cover the requested normally returning `absent()` computation; forwarding; mixed Observe/Discard instances; normal leaves across forty sequential calls; the 32-frame boundary; argument effects before the entry guard; an argument trap before a rejected entry; constructor operands and traps in written order; nested constructors; read temporaries and sibling field loans; dormant branches and represented matches; lexical shadowing; represented record field order; vector/byte/string operands; and K1/K2/K3/K3b controls.

The private probes cover exact demand/callee correspondence, weak-class separation, nominal identity and complete inactive schemas, missing/foreign/extra logical metadata, cross-function opaque scope, immutable nonuse recounting, retained dead syntax, original CheckedProgram correspondence, independently reconstructed positive IR flow, logical depth preflight, and the exact frozen-evidence budget boundary.

Both internal-only discard and source-only interruption suffixes are tested with only Value-result functions and no physical ADTs. The suffix fixture has no DiscardLet tag. It rejects missing control/domain/schema/scope tables, foreign compilation metadata and extra binders.

## Review finding closed before the execution handoff

A source-only nominal in an omitted interrupted-call suffix initially escaped the independent pure-source boundary. In the fixture below, consistently changing inactive `B.BS` to an owner/native field in both retained schema copies was accepted by the old standalone verifier:

```rill
type B<a> = BN | BS(a)
fn spin(n:i32) { spin(n) + 1 }
fn ignore(a,b) { 42 }
fn main() { ignore(spin(0),BN) }
```

The author confirmed the red case at `9112d7946453ed7c07cd3f255e0644adc3ee0fe7` and fixed it in `ee5763f16686cf5a67decc80dd78979fa8d3ec80`. `Frozen::check_source_purity` now independently rebuilds complete schema dependencies and checks every retained source signature and node, including omitted suffixes, nominal arity and referenced foreign names. The independent paired-schema mutation tests reject both `SystemBoxI32` and `u32` at every public consumer.

Source review also checked that iterative actual-IR and witness depth/charge preflights precede recursive consumers; exact Frozen environment/evidence validation binds suffix-only metadata; and lower-side reservations cover the new branch/match scope copies and demand-child temporary vectors. The last point is a source audit, not an exhaustive allocation stress proof.

## Compiler diagnostics and preserved first failure

The final matrix retained two nonempty compiler diagnostic files, both on the existing K1 weak-mutable match control:

- `retained_k1_weak_mutable-clang-O0-build.stderr`
- `retained_k1_weak_mutable-clang-O2-build.stderr`

Each reports exactly two `-Wparentheses-equality` warnings, at generated C lines 129:19 and 133:24:

```text
warning: equality comparison with extraneous parentheses [-Wparentheses-equality]
129 | if (((rl_t_14.tag == 1))) {
133 | else if (((rl_t_14.tag == 0))) {
2 warnings generated.
```

The diagnostics also suggest removing redundant parentheses or using assignment. The full unmodified stderr is retained. This legacy match control was not dropped and the warnings were not treated as new discarded-result support failures. The other 210 final C compilations were diagnostic-free. All Rust probe compilations were diagnostic-free.

The first origin harness completed four cases, then incorrectly expected a SourceProgram for an already type-invalid source. `load_program_with_sources` instead returned `E-MODULE-CHECK` containing `E-TYPE: incompatible types`. This was a fixture expectation error, not a production failure. The initial source, executable and `origin-run-1.log` remain preserved. The corrected harness distinguishes six optional-API cases from two earlier module-loader rejections; final evidence is `origin-run-3.log`.

The explicit uniform reference C backend classified 34 public cases as unsupported and only the retained K1 weak-mutable control as supported. These classifications are recorded separately and do not weaken typed acceptance checks.

## Freshness, toolchain and resources

Toolchain: rustc 1.99.1, cargo 1.99.1, GCC 14.2.0, Clang 19.1.7.

The build used a RAM-resident checkout, the installed isolated toolchain, and the sole `CARGO_TARGET_DIR` on the same RAM filesystem, with two Cargo jobs, dev/test debug information disabled and incremental compilation disabled. The persistent and RAM roots were assigned by the execution environment and checked separately; checking only the working directory would not cover the persistent filesystem. Root source timestamps were refreshed only after hashing their bytes, followed by successful hash rechecks. Cargo JSON records `fresh: false` and the actual RAM `src/lib.rs` path for both the final library and private test executable.

Final artifact SHA-256:

- Library, 29,184,550 bytes: `c4839ab4837fbb0f81202d83bed02949c86d8fa2004fd5bc42a97dd494718558`.
- Private test executable, 15,327,416 bytes: `aac53c1a1ff94ca38e355dd1acb4e651916a89483edb35dab62a4df6089f3ab1`.
- Public matrix executable: `79cdcdf8d021f36d7ec708572ed8d4a4661b42b2c2b554cbec9e02350f5c3916`.
- Origin executable: `75feffe64eb7c18d6ba7da8da432131b213bfc257b5f69316d07d10c30bed20d`.

Every Rust/C build and Clippy invocation was preceded by the resource guard. Across 434 retained guard samples, minima were 2,097,720 KiB persistent free, 2,683,252 KiB RAM-source/target free, and 6,313,528 KiB MemAvailable. Required floors were 2,097,152 KiB for both filesystems and 3,145,728 KiB memory. No guard failed. No install, deletion, new checkout/target, remote write or backup upload was performed by this independent run.

## Evidence and reproduction

Retained evidence root:
`${CARGO_TARGET_DIR}/discarded-results-independent-review`, with `CARGO_TARGET_DIR` supplied by the execution environment on the `K4_RAM_ROOT` filesystem.

Key files: `private-build-2.jsonl`, `private-artifact-2.json`, `private-run-2.log`, `lib-build-2.jsonl`, `library-artifact-2.json`, `public-run-2.log`, `public-output-2/`, `origin-run-1.log`, `origin-run-3.log`, `origin-output-3/`, `fmt-check-1.log`, `clippy-1.log`, `evidence-summary.json` and `evidence-manifest.json`.

- Evidence manifest SHA-256: `fdf9d8ef76d2c55c2561c4d1227a3c66a38a3e9765488cd8b5558fd36fc359f3`.
- Evidence summary SHA-256: `d9a9e30e036e2bbacf6eda545aa66f097a1623420ba2f3dd9383428799310326`.
- Final private log: `48304848e45d648d977735418c9e978beed6c9cea01f9a0f9949b05dc8309dce`.
- Final public log: `bcad5e3bec18b314b306a7b3b90a4364a0eda500458042d9f782692baa2fe66b`.
- Final origin log: `fd58c80e1b51ccc1cef51f0de0e1b4f4606cc8d9149a4ab2bd58cc8b1d185e14`.

The five private modules run with `cargo test --lib independent_k4`. The two files in `tests/support/discarded_independent_*.rs` are standalone Rust executables linked with `--extern rill=<fresh rlib>` and the target dependency directory; they are not automatically run by Cargo's integration-test discovery. The public executable requires a fresh `K4_PROBE_OUTPUT` directory and `K4_RESOURCE_GUARD` pointing to `tests/support/discarded_resource_guard.sh`. The origin executable requires a fresh `K4_ORIGIN_OUTPUT` directory. The portable guard requires explicit `K4_PERSISTENT_ROOT` and `K4_RAM_ROOT`; it never falls back to the working directory. Source and target must both reside on the filesystem selected by `K4_RAM_ROOT`. Both filesystem floors remain 2,097,152 KiB and the MemAvailable floor remains 3,145,728 KiB.

## Guard portability correction

The recorded Rust/C matrix executed the earlier guard from test commit `94f2048f092a76ad65fb08847649ae47912ec897`, SHA-256 `0a6aea119cf3be64ee3fed6fa6640160f88220c89f4c61d7a5aebbc5bd1f8dee`. That revision contained the run-specific filesystem roots assigned by its execution environment. Its source and all previous execution evidence remain preserved.

The later publication-only correction removes those roots from committed artifacts and requires the explicit variables above. This changed guard was checked with `bash -n`, a successful smoke run with the actual configured roots, and failing smoke runs with each required variable missing and with both missing. No Rust/C compilation or matrix rerun was performed for this correction. The earlier artifact and evidence hashes above refer to the original executed guard, not this portability revision.

## Retained K3b expectation migration

The subsequent full aggregate on `096eea3c960e9f438f6322e741bfb85990ef4105` found one stale K3b test expectation in both modes: default 937 passed/1 failed and `c-import` 985 passed/1 failed, with no ignored/skipped tests. The exact source `type O<a> = None | Some(a) fn absent(){None} fn main(){let unused=absent();42}` was still listed as unsupported even though it is the intended K4 positive. Initial aggregate evidence remains at `${CARGO_TARGET_DIR}/evidence/integrated-096eea3c960e/`; no logs were replaced.

Test-only commit `0e77b273b7b8efb8f44877bde1fc45c2e298ed56` moves that unchanged source into the named `independent_returning_immutable_discard_uses_typed_path` test, using the existing `compare(..., Some("42"))` helper. This requires explicit typed/reference/default parity and typed C execution. The five original genuine exclusions remain unchanged; normally returning `absent()` used-result and mutable-result negatives were added. No production implementation changed.

The complete resulting six-test binary passed with GCC and Clang, six tests each, zero failures/ignored/filtered tests. Each compiler performed 40 C11 compilations and executions across O0/O2, for 80 total; every compile passed the explicit persistent/RAM/memory guard. GCC produced no diagnostics. Four Clang compile logs retained only the known `-Wparentheses-equality` warnings from two unchanged Copy-match controls, two warnings per log: generated lines 105:13/109:18 and 106:18/110:23, each at O0 and O2. Formatting and strict default all-target Clippy passed. The full aggregate was not rerun as part of this narrow correction.

Evidence is under `${CARGO_TARGET_DIR}/discarded-results-independent-review/expectation-migration/`: `build.jsonl`, `artifact.json`, `gcc-run.log`, `clang-run.log`, per-compiler artifact/guard directories, `fmt.log`, `clippy.log` and `summary.json`. Cargo records `fresh: false` for the actual test source. Test executable SHA-256: `79ee2f9892345c9a256e8d03333a5953643aa137fc62c3376757d708dd36d5d6` (9,143,136 bytes). Source SHA-256: `c926f449a5072146fceb7dc797090972de1d3d910bde5016367e10025cf7b89a`. GCC/Clang run-log SHA-256 values are respectively `8c59aa01d8e5abcf8ffe78af5b8a7df88cfce72bd00f0cf31c8c36816d709d9f` and `68aa71d206ec74a0e6a535e7f7221d2ccc068c33e38643d60f56a9a2e8c6850e`.
