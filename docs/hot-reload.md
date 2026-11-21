# Rebuild/restart and function hooks

Use source watching for the native development loop:

```sh
cargo run -- watch examples/project/src/main.rill
cargo run -- project watch examples/project
cargo run -- project watch examples/project --opt-level 0 --interval-ms 200
cargo run -- project watch examples/project --once
```

`watch` builds a candidate executable through the ordinary checked native
compiler. A successful build stops the previous program and starts the candidate.
A failed build leaves the previous program running and waits for another edit.
Each successful replacement starts a new process with fresh state. A normally
exiting program stays stopped until an input changes. `--once` builds and runs
one candidate and returns its status; it is useful for scripted smoke checks.

The default input set follows the entry file's transitive local imports. Editing
an unused library file does not rebuild the program. Watch observes content,
including same-length changes, rather than relying on timestamp resolution. Once discovery succeeds, unchanged
polls reuse the known dependency set and perform no import lexing, parsing or checking.
When import discovery fails, it temporarily scans the entry directory tree so
creating a missing imported module can repair the build. Bundled `std` sources
belong to the compiler executable; rebuild the compiler and restart watch to
change them.

Add native files, headers or assets outside that graph explicitly:

```sh
rill project watch my-tool --watch native --watch assets/player.png
```

An explicit file can have any extension. An explicit directory recursively
observes `.rill`, `.rillcfg`, `.h`, `.c` and `rill.json` files. Hidden directories,
`target`, `build` and `node_modules` are skipped; nested symlinks are skipped and
explicit symlink inputs reject. Source discovery uses the ordinary module
loader's path rules. Watch limits traversal to depth 64, 4,096 files,
1 MiB per file and 64 MiB total observed content. These are development-tool
limits, not new language limits. `--interval-ms` accepts 10 through 60,000 and
defaults to 200. Usual native compiler and linker options pass without shell
parsing. Interpret mode is not a watch option.

Project watch also observes `rill.json`. The command resolves its entry and
options at startup. Restart watch after changing that recipe. It does not
implement an incremental compilation cache, and does not skip checking imported
declarations. See [large library builds](build-performance.md).

On Unix, Ctrl-C and SIGTERM cancel the watcher, stop its active native process
group and reap owned children. Compiler cancellation first gives the driver two
seconds to clean up supervised import workers. Process replacement stops the old
program forcibly, so it does not promise application cleanup or saved state.
Background jobs that outlive their leader or detach into another group are not
managed. Child groups do not receive foreground terminal handoff; use ordinary
`project run` for programs that read terminal stdin. Non-Unix hosts supervise
only the immediate child; Unix signal handling
is not available there. Force-killing the watcher itself bypasses cleanup.

## Embedded per-function calls

`rill::reload::Session` supports a separate embedding workflow. A Rust host
names hooks with closed, monomorphic value-mode `i32` parameters and an `i32`
result. The input is an ordinary checked `ast::Program` library without `main`.
Owners, foreign declarations, native dependencies and foreign values reject.
Pure helpers can use ordinary Copy types, records/enums and polymorphism.
The foreign-value ban includes unused raw signatures, raw declaration fields
and raw types nested in callable metadata. Admission shares the
[bounded whole-source type scan](c-exports.md#admitted-interface) used by scalar
C exports. A rejected replacement keeps the existing generation available.

```rust
use rill::{parser, reload::Session};

fn main() -> Result<(), String> {
    let mut session = Session::new(
        parser::parse("fn update(state) { state + 1 }")?,
        &["update"],
    )?;
    let state = session.call_i32("update", &[40])?;
    let next = parser::parse("fn update(state) { state + 2 }")?;
    session.replace(next)?;
    assert_eq!(session.call_i32("update", &[state])?, 43);
    Ok(())
}
```

`new` and `replace` check the entire library and emit each hook's checked wrapper
without executing the hook. All registered hook contracts must remain compatible.
An incompatible or invalid candidate retains the current program and generation.
`replace` requires exclusive access between synchronous calls. A call uses one
complete accepted generation, including its helper functions. State lives in the
host and crosses the boundary only as integer arguments/results. Generation
starts at zero and advances only after an accepted replacement; exhaustion rejects.

Each invocation clones, checks and lowers the program through the ordinary
interpreter. Use this API for embedding and iteration; it is not a fast native
dispatch API. Resolved module names are compilation-local identities. Hosts that
use the source-preserving library loader must derive names from each compiled
program and reject identity mismatches, rather than persist internal symbol IDs.
No running Rill frame, reference or owned resource survives a call boundary.

Run the host example with `cargo run --example reload_host`.

## Source and standard-library hooks

`std.reload` contains ordinary source functions with explicit host input:

```rill
import std.reload;
fn update(state: i32, generation: i32, previous: i32) -> i32 {
    let context = reload.context(generation, previous);
    if reload.first_load(context) { state + 1 } else {
        if reload.changed(context) { state + 10 } else { state }
    }
}
```

`Context` stores `generation` and `previous` as `i32` fields. `context` constructs
it, `changed` compares them, and `first_load` tests `previous < 0`. The host passes
an accepted generation and tracks the last generation it handled. These helpers
read no filesystem or hidden global state and register no implicit callbacks.
The [runnable source example](../examples/reload/main.rill) prints `13`.

The experimental [scalar C export API](c-exports.md) now emits checked library
functions, headers and a versioned interface manifest. It supports direct native
calls from C, with host-owned integer state and thread-local call-depth guards.
It does not load a replacement or change a dispatch pointer.

Arbitrary native function replacement still requires loader/dispatch,
quiescence and state-migration work recorded in the
[native reload blocker](blockers/hot-reload.md). Neither workflow patches
suspended native frames or transparently migrates live Rill objects.
