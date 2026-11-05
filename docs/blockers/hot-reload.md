# Native per-function hot reload

Generated native executables do not yet support replacing individual Rill
function bodies while preserving arbitrary live program state. File watching
and rebuilding/restarting a program provide a useful edit loop. An embedded
checked-program reload API can replace supported function definitions under
host control. An experimental scalar C export API now supplies a narrow native
call boundary. None of these implements native generation replacement or
live-state migration.

## Evidence

- [PLAN.md section 14.5](../../PLAN.md#145-practical-development-loop)
  requires an explicitly versioned plugin ABI, state migration and quiescing
  active jobs/callbacks. It excludes assuming that suspended frames, vtables
  and pointers into retired code survive unloading.
- [The typed C emitter](../../src/typed/c.rs) emits private `static` functions
  named `rl_fn_<id>` and direct calls. Its executable mode adds a fixed `main`;
  its new verified scalar-library mode instead emits explicit C wrappers.
  [The reference emitter](../../src/backend.rs) uses the
  same private indexed-function structure. These indices are internal to one
  compilation and are not exported stable function identities.
- [Typed instances](../../src/typed/instances.rs) specialize concrete calls.
  A source function can have several instances with different layouts; a
  source name alone cannot identify a native callable ABI.
- [C interop](../c-interop.md) currently supports scalar/raw-pointer calls,
  not exported Rill callbacks, aggregate adapters or checked foreign retention.
  Header compatibility does not prove that old code or state can be retired.
- [Resource-capturing functions](function-resources.md) remain unsupported.
  The checker does not yet track affine captured environments, retained loans
  and destruction across indirect invocation. A reload protocol cannot infer
  their safety from Copy-callable support.
- [Scalar exports](../c-exports.md) check every source declaration, admit only
  monomorphic value-i32 endpoints and emit a header and ABI-version-1 JSON
  manifest. Independent C callers exercise several roots, recursion, repeated
  calls, traps and separate host threads at O0/O3. A private library verifier
  retains all ordinary body checks and rejects symbolic/control proofs that
  assume a single source main. The artifact has no loader or dispatch table.
- [Phase status](../phase-status.md) leaves canonical public interfaces,
  general ownership/effects, concurrency and stable ABI work open. The ordinary
  native CLI still builds executables. Scalar export generation is an embedding
  API and does not produce a versioned reloadable plugin by itself.

These are concrete missing compiler/runtime contracts. A polling interval,
shared-library loader or symbol lookup alone cannot discharge them.

## Required next work

1. Build on the admitted scalar export contract. Add explicit target/compiler
   and build identities, generation content identity and compatibility checks.
   The host must match public C names and exact signatures, not private
   specialization IDs. Wider signatures still need their own checked contract.
2. Add loading and host call indirection for opted-in exports.
   Preserve ordinary direct calls outside that boundary. Compile and validate
   a candidate generation before making any replacement visible.
3. Specify host-owned state independently of implementation-private layouts.
   Give state a version, validation rules and explicit migration callbacks.
   Reject an incompatible update without changing the active generation;
   migration failure must retain valid ownership and usable old state.
4. Define a safe replacement point. Wait for active calls, jobs and retained
   callbacks to finish, or retain the old generation until every relevant
   reference is released. Specify recursive and reentrant calls. Do not unload
   a generation while any return address, function pointer, environment,
   vtable or destructor still depends on it.
5. Track source, bundled-library, native-header, compiler/target and build-option
   identities. Changes to a dependency must invalidate every affected candidate.
   Publish an entire accepted generation atomically; failed checking, linking,
   loading or migration must leave the old generation available.
6. Add acceptance tests for successful body replacement, incompatible signature
   and state rejection, failed builds, failed migrations, active calls and
   callbacks, recursive/reentrant dispatch, cleanup exactly once, and repeated
   reloads. Run generated native tests at debug and optimized levels with
   explicit platform loader coverage. Measure update latency and retained
   generations/resources separately from program execution time.

Until those contracts have implementation and acceptance evidence, document
the exact limits of watch/restart and embedded reload support. Do not describe
them as arbitrary native function patching or transparent preservation of
live Rill state.
