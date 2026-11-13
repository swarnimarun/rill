# Native function generations

The experimental `rill::native_reload` APIs replace checked native functions
between synchronous host calls on macOS and Linux. The scalar `NativeSession`
retains no Rill state; hosts pass `i32` arguments and results. The bounded
`CopySession` carries explicit closed Copy values through a versioned byte ABI.
`live_reload::LiveSession` adds versioned host-owned Copy state and explicit
migration. Project `watch` still restarts whole executables.

## Use the host API

```rust
use rill::{
    abi::Export,
    native_reload::{NativeCandidate, NativeCompiler, NativeSession},
    parser,
};

fn main() -> Result<(), String> {
    let roots = [Export {
        source: "update".into(),
        c_name: "rill_update".into(),
    }];
    let compiler = NativeCompiler::default();
    let first = parser::parse("fn update(state: i32) -> i32 { state + 1 }")?;
    let mut session = NativeSession::new(NativeCandidate::build(
        &first, &roots, &compiler,
    )?)?;
    let state = session.call_i32("rill_update", &[10])?;
    let next = parser::parse("fn update(state: i32) -> i32 { state + 7 }")?;
    let candidate = NativeCandidate::build(&next, &roots, &compiler)?;
    session.replace(candidate)?;
    assert_eq!(session.generation(), 1);
    assert_eq!(session.call_i32("rill_update", &[state])?, 18);
    Ok(())
}
```

Run `cargo run --locked --example native_reload_host`. It prints
`generation 0: 11` and `generation 1: 18`. Compilation is synchronous; a host
can build a candidate separately while keeping its active session available.
The API does not watch files or start a background builder.

`NativeCompiler::default()` selects `cc` at O0. Select another trusted native
compiler with `NativeCompiler::new("clang", Optimization::O3)`. Arguments pass
directly to the executable. Arbitrary target/compiler/linker flags and a
`zig cc` convenience mode are outside this boundary.

## Functions and compatibility

The [scalar export rules](c-exports.md#admitted-interface) apply to every
candidate. Explicit roots have zero through 32 value-i32 parameters and an i32
result, with at most 64 roots. Every declaration is checked, including unused
code. Source has no `main`, native dependencies, foreign declarations, owners,
raw values or escaping native function pointers. Supported pure Copy helpers,
records/enums and concrete generic helper specializations remain available.

Replacement preserves the complete sorted set of public C names and scalar
signatures. Adding/removing/renaming a root or changing arity rejects. A new
session is needed for a different interface. Source/export order can change.
Helpers and registered bodies change together, and a call uses one complete
accepted generation. Resolve source export mappings afresh for each module
graph; internal IDs are not stable hook names. Tests include resolved
`std.reload` source hooks.

`manifest()` exposes a candidate's canonical scalar interface.
`build_identity()` exposes its SHA-256 identity for logging. The private
versioned identity binds the manifest, emitted base C, Rill compiler build,
recorded C compiler bytes/path, fixed flags and native host profile. It is a
consistency check for trusted local compilation, not authentication or a complete
SDK dependency certificate. Every candidate compiles afresh.

## Publication and lifetime

Candidates own unique private temporary directories and absolute image paths.
The loader uses local symbol scope, checks a bounded identity copy and resolves
the fixed version-1 dispatcher before publication. Dispatch calls verified
private function IDs directly, avoiding public symbol interposition. No API
loads arbitrary library paths or exposes native function pointers.

Calls and replacement require `&mut NativeSession`. Session is neither Send nor
Sync, with compile-fail checks for both. Replacement occurs after previous
synchronous calls return. Retired code unloads before its files are removed.
There are no callbacks or native references that outlive a call.

Generation starts at zero and advances only after successful replacement;
exhaustion rejects. Source, compiler, contract, image, identity and symbol
failures leave the old generation callable. Failure to remove retired files
does not report a failed replacement after publication. For `NativeSession`,
host state crosses the boundary as integer inputs/results.
`CopySession` accepts only the closed schemas described in
[Copy-wire exports](c-exports.md#experimental-copy-wire-abi). `LiveSession`
validates its versioned state schema and lets the host supply migration code.
A state checkpoint uses format version 1, strict JSON decoding and a 1 MiB
limit. A changed schema requires a strictly higher application state version;
unchanged schemas may keep or increase the version. Restore migration must
produce the active session's exact version and schema. Neither API transports
owned Rill resources or native pointers.

## Limits

Arithmetic and recursion-depth traps terminate the entire host process with
status 1. They do not return `Err` or unwind. Use a process boundary if the host
must survive a trap. API Results cover build/load/contract/call-admission errors.

The selected compiler, wrapper/environment and native execution are trusted.
This is not a sandbox. Other loader platforms reject explicitly; Windows
compile-only checks do not establish loader execution there.

`NativeSession` and `CopySession` replace a complete compatible library
contract. `LiveSession` accepts 1 through 64 unique roots. `Pure` functions
receive host arguments, `Query` takes one state value and returns a fixed Copy
result schema, and `Step` takes and returns the same state schema. A selected
set is built and checked before publication; each root keeps its own complete
reachable helper cohort. Unselected roots remain on their old code, so select
together roots that must share helper changes.
Publication occurs between exclusive synchronous calls. The host migration
callback and schema validation finish before code/state publication; failure
keeps the previous session active.

These APIs do not patch active frames, migrate owned objects or foreign/GPU
resources, retain callbacks, or coordinate arbitrary threads/jobs. Native
traps remain process-fatal. The renderer example keeps its window and graphics
resources in a long-lived C/Raylib adapter and migrates only its explicit Copy
state. See [the runnable example](../examples/reload-renderer/README.md) and
the [remaining reload blocker](blockers/hot-reload.md).
