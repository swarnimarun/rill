# Native per-function hot reload

Generated native executables do not support arbitrary code replacement while
preserving arbitrary live program state. Project watching still rebuilds and
restarts the process. The embedded interpreter has a separate checked reload
API. Native APIs cover closed scalar `i32` calls, bounded Copy-wire calls, and
versioned host-owned Copy state with explicit migration and selective root
replacement between exclusive synchronous calls on macOS/Linux. These bounded
APIs do not migrate owners, foreign or GPU resources, or retained callbacks, and
they do not coordinate active jobs or arbitrary threads. See the
[native generation contract](../native-hot-reload.md).

## Evidence

The scalar and Copy native sessions load source-generated private images, check
complete public contracts and versioned build identity, and resolve fixed
dispatchers before publication. Calls and replacement require exclusive
access; session types are neither Send nor Sync. Failed candidates preserve the
active generation. `LiveSession` validates bounded Copy state, runs explicit
host migration, and publishes selected roots only after candidate code and the
resulting state pass validation. Each root retains its own complete helper
cohort. The historical scalar export evidence below describes the original
i32 boundary, not the later Copy-wire or state contracts.

- [PLAN.md section 14.5](../../PLAN.md#145-practical-development-loop)
  requires an explicitly versioned plugin ABI, state migration and quiescing
  active jobs/callbacks. It excludes assuming that suspended frames, vtables
  and pointers into retired code survive unloading.
- [The typed C emitter](../../src/typed/c.rs) emits private `static` functions
  named `rl_fn_<id>` and direct calls. Its executable mode adds a fixed `main`;
  library modes instead emit explicit C wrappers. The
  [reference emitter](../../src/backend.rs) uses the same private
  indexed-function structure.
  Those indices are internal to one compilation, not stable function identities.
- [Typed instances](../../src/typed/instances.rs) specialize concrete calls.
  A source function can have several instances with different layouts; a
  source name alone cannot identify a native callable ABI.
- [C interop](../c-interop.md) supports scalar/raw-pointer calls, not exported
  Rill callbacks, aggregate adapters or checked foreign retention. Header
  compatibility does not prove that old code or state can be retired.
- [Resource-capturing functions](function-resources.md) remain unsupported.
  The checker does not yet track affine captured environments, retained loans
  and destruction across indirect invocation. Reload cannot infer their safety
  from Copy-callable support.
- [Scalar exports](../c-exports.md) admit monomorphic value-`i32` endpoints and
  emit a header and ABI-version-1 JSON manifest. Independent C callers exercise
  multiple roots, recursion, repeated calls, traps and separate host threads
  at O0/O3. The Copy-wire export API is a separate bounded versioned codec; it
  does not expose native aggregate layouts. Neither export artifact alone has a
  loader or dispatch table.
- [Phase status](../phase-status.md) leaves canonical public interfaces,
  general ownership/effects, concurrency and stable ABI work open. The ordinary
  native CLI still builds executables. These embedding APIs do not provide a
  general plugin ABI.

A polling interval, shared-library loader or symbol lookup alone cannot
establish safe live-state migration or concurrent replacement.

## Remaining work

The bounded synchronous stateful API does not establish general live-resource
migration or concurrent replacement. Contracts for foreign lifetimes/effects,
retained callbacks, jobs, threads, GPU resources and owner-bearing state remain
open. Tests must establish quiescence and cleanup before any broader replacement
claim. Keep these features rejected until their contracts and evidence exist.

## Required GPU verification blocker

The dated [Copy-wire/stateful reload verification](../verification/2026-11-13-stateful-copy-reload.md)
records the required real-Raylib test attempt. It exited 101 because pkg-config
could not find `rill-raylib.pc`; `pkg-config --exists rill-raylib` exited 1.
The ordinary integration test explicitly skipped when no standalone renderer
executable was supplied. No real window or GPU readback was established.

To close this evidence gap, provision the alpha SDK or a system installation
that supplies `rill-raylib.pc`, headers and libraries, and run on a host with a
usable graphics device. Build the standalone example, then run the required
test from the repository root:

```sh
cargo build --locked --example native_reload_renderer
RILL_REQUIRE_GPU=1 \
RILL_RELOAD_RENDERER_EXE="$PWD/target/debug/examples/native_reload_renderer" \
  cargo test --locked --test native_reload_renderer \
  real_raylib_window_runs_without_restarting_for_logic_replacement -- --nocapture
```

Record the actual pkg-config, build and device outcomes in a dated verification
update. Do not count the optional skip as hardware acceptance.
