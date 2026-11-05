# Incremental builds and native reload implementation plan

> **For agentic workers:** Use the delegate skill to implement the independent
> cache and loader tasks, then review and verify the combined changes.

**Goal:** Reuse checked generated C for unchanged pure programs and replace
checked scalar native functions between synchronous host calls.

**Architecture:** Cache exact loaded source graphs and compiler identity, not
executables or C toolchain results. Every native build still invokes the current
C compiler and linker. Native reload compiles a private candidate image and
validates its complete scalar contract before changing the host dispatch table.

**Tech stack:** Rust, C11, existing Unix libc loader bindings, no new crates.

**Spec:** User requests for incremental performance, function-level reload,
[build contracts](../../build-performance.md) and
[reload boundaries](../../blockers/hot-reload.md).

## Constraints and traced boundaries

- Preserve `v1` history. `v1-ernie` is already its ancestor.
- Check every imported declaration before publishing generated C.
- `modules::load` validates filesystem/import identities before cache lookup.
- `native_headers::load_graph` consumes that exact graph on a cache miss.
- Do not cache native header/configuration proofs or interpreter executions.
- Keep current native flag validation and structured compiler/linker arguments.
- Never serialize address-keyed checked-program maps or private source IDs.
- Native calls admit only the existing checked value-i32 export contract.
- Preserve traps, depth checks, ownership and current executable C output.
- No callbacks, live Rill objects, running-frame patching or state migration.

## Task 1: Checked generated-C cache

Files: `build.rs`, `src/fingerprint.rs`, `src/build_cache.rs`, `src/main.rs`,
`src/native_headers.rs`, `src/project.rs`, `src/watch.rs`,
`tests/incremental_cli.rs`.

Interfaces:

```rust
pub(crate) fn digest(bytes: &[u8]) -> [u8; 32];
// Binary-private cache, with failures treated as misses.
impl Cache {
    pub(super) fn new(directory: PathBuf) -> Self;
    pub(super) fn lookup(&self, graph: &ModuleGraph) -> Option<String>;
    pub(super) fn store(&self, graph: &ModuleGraph, c: &str);
}
```

- [x] Embed a framed SHA-256 build identity covering compiler sources, bundled
  modules, Cargo inputs, compiler version, features and build settings.
- [x] Store bounded framed keys and C integrity hashes with atomic publication.
  Reject malformed entries and unsafe cache paths without failing compilation.
- [x] Add `--incremental`, `--no-incremental`, `--cache-dir`, `--cache-status`.
  Project build/run/watch enable reuse by default; raw source commands opt in.
- [x] Avoid a redundant parse/use walk for graphs without native includes.
- [x] Verify exact source/import invalidation, corrupt entries, compiler failure,
  unchanged C compilation/linking, project confinement and watch forwarding.
- [x] Measure clean, warm reuse and edited-source behavior with expected program
  outputs checked outside the timing interval. Commit this independent change.

## Task 2: Native scalar generations

Files: `src/abi.rs`, `src/typed.rs`, `src/typed/library.rs`, `src/typed/c.rs`,
`src/native_reload.rs`, `src/lib.rs`, `tests/native_reload.rs`,
`examples/native_reload_host.rs`.

The host builds a `Candidate` from a fresh `Program`, explicit public exports
and a host C compiler. `Session::call_i32(&mut self, name, arguments)` and
`Session::replace(&mut self, candidate)` require exclusive access. Session is
neither Send nor Sync, and never exposes generation function pointers.

- [x] Emit a private fixed-signature dispatcher using verified function IDs.
  Sort public names and signatures for compatibility; source order is irrelevant.
- [x] Bind a bounded identity query to emitted code, interface and build inputs.
- [x] Compile to a unique private absolute image path on macOS/Linux. Load,
  query identity and resolve the dispatcher before replacing the active image.
- [x] Reject incompatible candidates and retain the old callable generation on
  every preparation/load failure. Increment generation only after publication.
- [x] Exercise O0/O3 body/helper changes, reordered exports, repeated reloads,
  signature rejection, bad compiler output, trap subprocesses and host state.
- [x] Document host/compiler trust and exact platform limits. Commit separately.

## Final verification

- [x] Review diffs and untracked files, excluding build output and caches.
- [x] Run affected tests, `cargo fmt --all -- --check`, full locked tests and
  `cargo clippy --all-targets -- -D warnings` after freezing compiler sources.
- [x] Update build/reload guides, blockers, phase status and cloud handoff with
  actual acceptance evidence. Keep broader language and ABI gates open.
