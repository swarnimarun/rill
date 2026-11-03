# Integrated cloud verification

The experimental `v1-ernie` continuation is based on the original `v1` checkpoint
`4a1014e0c4b9b28434c1506cf32ce90d47f87ec2`. These are bounded increments;
`PLAN.md` phase completion gates remain authoritative.

## Integrated source checkpoint `2e7b09b`

This local checkpoint combines:

- Selected C declaration import through libclang, with exact target checks.
- Actual SDL3 examples and reproducible headless framebuffer captures.
- Concrete Copy enums and nested/primitive matches on the checked typed path.
- Unix CLI importer workers with bounded IPC, deadlines and process cleanup.

The combined tree passed all of the following, serially, on x86_64 Debian 13
with Rust/Cargo 1.99.1, GCC 14.2, Clang/libclang 19.1.7 and actual SDL3 3.2.10:

```sh
cargo fmt --all -- --check
cargo test --locked --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features c-import --no-fail-fast -- --nocapture
cargo clippy --locked --all-targets --features c-import -- -D warnings
cargo check --locked --all-targets --target x86_64-pc-windows-gnu
cargo check --locked --all-targets --target x86_64-pc-windows-gnu --features c-import
```

- Default suite: **306 passed, zero failed, zero ignored, no dependency skips**.
- Importer-enabled suite: **340 passed, zero failed, zero ignored, no dependency skips**.
- Formatting and strict Clippy passed in both configurations.
- Both Windows-target all-targets compile checks passed. This is not Windows
  execution or linkage evidence; non-Unix CLI import intentionally rejects the
  unavailable process-isolation boundary.
- The real direct and generated-binding SDL3 tests ran successfully. Captures
  are native software-rendered headless frames, not physical desktop windows.

The typed-path migration's full corpus found a JSON interpreter stack regression.
It was fixed by reducing recursive evaluator frame size, without increasing the
existing test stack or weakening the 32-call limit. The existing recursion tests
and a new regression now pass. Independent source/IR probes and native cleanup,
allocation-failure and UBSan checks are recorded in the component reports.

## Integrated source checkpoint `c1f7aa7`

This checkpoint additionally combines the shared lossless lexer and conservative
stdout/check formatter with reachable generic function specialization on the
checked typed path. The same seven commands above passed serially, in the same
actual-SDL3 environment:

- Default suite: **333 passed, zero failed, zero ignored, no dependency skips**.
- Importer-enabled suite: **367 passed, zero failed, zero ignored, no dependency skips**.
- Formatting and strict all-target Clippy passed in both configurations.
- Both Windows-target all-targets compile checks passed; these remain compile
  evidence, not Windows execution or linkage evidence.
- Actual direct and generated-binding SDL3 integration ran successfully.

The formatter's independent review compared 852 parsed fixture candidates,
10,224 whitespace variants and 50,000 adversarial lexer inputs; 64 original/
formatted GCC/Clang executions agreed at `-O0` and `-O2`. Generic specialization
was independently checked with 24 source programs, seven instance/verifier
tests and the real 1024/1025 reachable-instance boundary. See the component
reports for the precise scope and reproducible committed tests versus extra
review probes. These results do not close the remaining grammar CST, local
polymorphism, typed record/loan, solver or general host-stack-safety gaps.

## Integrated source checkpoint `c32c41e`

The additional iterative Copy-let evaluator fix passed the same seven commands
serially, with **336 default tests** and **370 importer-enabled tests** passing.
There were zero failures, ignored tests or dependency skips. Both real SDL3
integrations, strict Clippy configurations and Windows-target compile checks
passed. The new 2 MiB subprocess regressions cover scalar and owner-returning
Copy-let chains at the unchanged 32-call boundary. The reference AST evaluator
and other recursive compiler paths still have documented stack limits.

## Component evidence and remaining limits

- [Selected import](2026-11-03-selected-c-import.md)
- [Actual SDL3 execution and captures](2026-11-03-sdl3-native.md)
- [Checked Copy patterns and compact layouts](2026-11-03-typed-copy-patterns.md)
- [Importer process supervision](2026-11-03-import-worker.md)
- [Lossless lexical source and conservative formatting](2026-11-03-source-format.md)
- [Reachable typed generic instances](2026-11-03-typed-specialization.md)
- [Iterative Copy-let evaluator scopes](2026-11-03-iterative-copy-lets.md)

No full P1–P11 completion, stable ABI, general memory-safety proof, foreign
lifetime/effect proof, hardware rendering, physical input, AArch64 execution or
Zig execution is established. The library importer remains in-process; Unix CLI
worker isolation is not a security or resource sandbox. See the current
[continuation plan](../CONTINUATION_PLAN.md) for remaining work.
