# Automatic headers and optimized native builds implementation plan

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](../../verification/2026-11-04-ernie-integration.md).

> For agentic workers: execute the tasks below with delegated implementation
> and focused review, preserving unrelated work in this shared checkout.

**Goal:** Let `@include` supply checked C declarations without handwritten
externs, retain useful tool-free editor analysis, and optimize native builds
and runs by default.

**Architecture:** Before ordinary module resolution, infer requested native
names from source references and import them through the supervised C frontend.
Append generated exported declarations to in-memory module snapshots. Publish
validated header snapshots for editor analysis after successful compiler work;
native code generation always imports against the current headers and flags.

**Tech stack:** Rust, libclang's existing runtime-loaded API, C11 and the
selected C compiler, existing framed Unix import worker, compiler-backed LSP.

**Spec:** User's `@include` and default-optimization instructions;
[PLAN.md sections 15, 19 and 21](../../../PLAN.md), and
[C interoperability](../../c-interop.md).

## Constraints and usage

- Preserve `{}` blocks, `;` statements and explicit manual extern compatibility.
- Do not guess foreign signatures, constants, ownership or lifetimes.
- Pass structured arguments directly. Preserve package/link argument order.
- Unsupported referenced C forms report their header location and reason;
  unrelated unsupported declarations do not prevent using a header.
- Include-generated declarations are exported through ordinary module aliases.
- Editing reads validated snapshots and never runs native dependency tools.
- Preserve checked arithmetic, traps, allocation and cleanup behavior.
- Default optimization is portable `-O3`, with explicit optimization overrides;
  do not enable fast-math or assume the build host is the execution target.

```rill
// native.rill
@include("SDL3/SDL.h");
@pkg_config("sdl3");

// main.rill
import native;
fn main() {
    let ready = native.SDL_Init(native.SDL_INIT_VIDEO);
    native.SDL_Quit();
    if ready { 0 } else { 1 }
}
```

## Tasks and interfaces

- [x] Ground the C frontend, worker, module resolver, source provenance and
  editor flow. Compare eager whole-header generation with automatic referenced
  declaration materialization. Choose the latter to avoid unused unsupported
  APIs and one translation-unit reparse per unrelated macro.
- [x] Implement `selected::Automatic` and `selected::Generated`, with
  `generate_automatic(&Automatic, &OsStr, bool) -> Result<Generated, String>`.
  `Generated` contains a checked module and real transitive dependency paths.
  Extend the supervised worker with a strictly validated automatic operation.
  Own files: `src/c_import/selected.rs`, `src/c_import/selected/clang.rs`,
  `src/import_worker.rs`, `tests/automatic_c_import.rs`.
- [x] Add pure `modules::headers` request discovery/application and public
  `compile::lower_graph`. Track lexical scopes and imported aliases, retain
  explicit declarations, and append only generated declarations in memory.
  Test direct/imported calls, constants, shadowing, exports and duplicates in
  `tests/header_modules.rs`.
- [x] Add atomic, bounded editor snapshots with native-preamble matching and
  transitive-header fingerprints. Source-backed loaders consume valid snapshots
  without executing tools; generated declaration ranges cannot become bogus
  file navigation locations. Own files: `src/modules/headers.rs`,
  `src/modules/compile.rs`, `src/modules/provenance.rs`, `src/lsp/analysis/**`.
- [x] Wire the CLI before resolution, enable the normal automatic-import build,
  and publish editor snapshots only after successful compilation/checking.
  Use the exact selected compiler and aggregate package/target flags. Test real
  CLI source-to-native execution and unsupported-use diagnostics without extern
  declarations in `tests/automatic_headers.rs`.
- [x] Make native build/run/check use the same default optimization flags and
  expose an explicit optimization-level override in `src/main.rs`. Measure a
  safe code-generation improvement before adopting it. Verify optimized and
  unoptimized behavior against the reference evaluator and existing native
  cleanup/overflow regressions.
- [ ] Update C/SDL3/editor docs and examples, run focused regressions followed
  by formatting, strict Clippy and the appropriate complete test inventories.
  Rebuild the Helix server and verify include-generated declarations with
  native tools disabled during editor analysis. Record evidence and skips.

## Acceptance commands

```sh
cargo test --locked --test automatic_c_import --test header_modules --test automatic_headers
cargo test --locked --test selected_c_import --test sdl3_direct --test sdl3_capture --test sdl3_lifecycle
cargo test --locked --test lsp_analysis --test lsp_hints --test lsp_editor_workflows --test lsp_protocol
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --no-fail-fast
```

On macOS, native importer tests use the active SDK root. Also check the
`--no-default-features` build so pure/manual-extern use remains available
without the runtime C importer. No PLAN.md phase-completion claim is implied.
