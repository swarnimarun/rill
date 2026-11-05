# Experimental scalar C exports

`rill::abi::emit_i32_library` emits a checked C11 library, a public header and a
versioned JSON interface. An independent C program can call selected Rill
functions without an executable Rill `main`. This is a prerequisite for native
function reload. It does not load or replace native code.

## Build the example

The [source library](../examples/c-exports/update.rill) accepts host-owned integer
state and returns its next value. The [Rust generator](../examples/c_exports.rs)
selects `update` explicitly as the C symbol `rill_update`.

```sh
mkdir -p build
cargo run --locked --example c_exports -- build/c-exports
cc -std=c11 -O3 build/c-exports/update.c examples/c-exports/host.c -Ibuild/c-exports -o build/c-exports/host
build/c-exports/host
```

The [C caller](../examples/c-exports/host.c) prints `42`. The generator requires
a new output directory and refuses to overwrite an existing one. Choose a new
directory when generating another candidate. Compile all generated C and the
caller with a compatible target, C ABI and compiler options. There is no new
`rill export` CLI command in this increment.

## Compiler API

```rust
use rill::{abi::{emit_i32_library, Export}, parser};

fn main() -> Result<(), String> {
    let program = parser::parse("fn update(state: i32) -> i32 { state + 1 }")?;
    let artifact = emit_i32_library(&program, &[Export {
        source: "update".into(),
        c_name: "rill_update".into(),
    }])?;
    assert!(artifact.header.contains("int32_t rill_update(int32_t arg0);"));
    Ok(())
}
```

`Artifact` owns three strings, `c`, `header` and `manifest`. The API performs no
filesystem writes and does not invoke a C compiler. It checks every source
declaration before validating the selected exports. Invalid unused declarations
still reject. To resolve source imports, use
`modules::compile::load_library_with_sources_and_overlays` with an empty overlay
map, then pass its `program()` to this API. Source names then use that loaded
program's resolved identities. See
[source-preserving library loading](module-source-origins.md#library-buffers-and-editor-overlays).

`export fn` controls Rill module visibility. It does not automatically create a
C symbol. The host selects the exact source function and external name through
`Export`. Private runtime functions and specialization indices never appear in
the public header or manifest. Wrappers call the verified instances selected
from the checked signatures, independently of export order.

## Admitted interface

- There must be 1 through 64 distinct exports and no source `main`.
- Each endpoint must have a closed, monomorphic signature with 0 through 32
  value-mode `i32` parameters and an `i32` result. Inference may establish this
  signature; annotations are not mandatory. Generic endpoints, `read`, `inout`,
  aggregates, booleans, unit and pointers reject at this external boundary.
- Explicit C names must start with `rill_`, contain only ASCII letters, digits
  and underscores, have a nonempty suffix and fit within 128 bytes. The
  `rill_abi_` prefix is reserved. Source names and C names must each be unique
  within the artifact. The host must prevent name collisions between artifacts.
- Generic private helpers may specialize to closed ordinary Copy layouts.
  Reached symbolic phantom and nonreturning/control representations are outside
  this increment and reject. No synthetic `main` or fabricated argument value
  is used to discover a function's layout.
- Native dependencies/declarations, raw C values, owners, allocation contracts
  and callable values reject across the checked source library. Unused raw
  parameter/result signatures and raw fields in unused type declarations also
  reject. This interface does not export closures, retain references or
  transfer owned memory.

ABI generation and interpreter reload admission share a bounded raw-type scan.
It examines checked signatures, expression/application metadata and declaration
schemas without expanding nominal layouts. More than 4,000,000 scheduled type
and declaration nodes reports `E-LIBRARY-TYPE-COMPLEXITY`. This additional limit
applies to these pure-library boundaries, not ordinary executable compilation.

The header uses `int32_t` and `RILL_I32_ABI_VERSION`, currently `1`. It permits
repeated inclusion and multiple generated headers in one caller. The JSON
manifest has `abi: "rill-c-i32"`, `version: 1` and an ordered `exports` array with
each public `name`, `arity`, `parameters` and `result`. Each type is `"i32"`.
Output is deterministic for identical source and export order. The manifest
does not encode a target triple, compiler flags, content identity, private IDs
or a complete loader compatibility decision. ABI version 1 describes this
experimental scalar schema, not a stable language release.

## Runtime and reload boundary

Generated libraries use a hosted C11 runtime, including stdio and process exit
for traps. Calls use the target C compiler's ordinary C calling convention. Signed
arithmetic overflow, invalid division and other ordinary Rill traps print
`rill: runtime trap` to stderr and terminate the process with status 1. There
is no recoverable error result or unwind across C. The Rill call-depth guard
allows at most 32 active Rill frames per thread. Public wrappers add no Rill
frame. The guard uses C11 thread-local storage in libraries, so repeated and
concurrent independent C calls do not share a depth counter. This does not
provide a Rill worker runtime or make an async signal handler a legal caller.

The scalar library retains no Rill state between calls. Keep persistent state
in the host and pass it as integers. Internal Rill calls remain direct calls
within one artifact. Updating a leaf function that has been inlined or called
directly requires recompiling its containing artifact.

A future reload host must validate a complete candidate, compare the required
public signatures and target/build identities, then publish dispatch changes
at a safe call boundary. It must wait for active calls before unloading old
code, or keep the old generation until those calls finish. State validation,
migration, callbacks, retained pointers, loader coverage and failed-update
rollback still need explicit contracts and tests. The current manifest and
header alone do not discharge these obligations. See the
[native reload blocker](blockers/hot-reload.md) and the existing
[watch/interpreter workflows](hot-reload.md).
