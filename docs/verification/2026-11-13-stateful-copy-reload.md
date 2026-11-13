# Copy-wire and stateful reload verification

This record covers the uncommitted working-tree snapshot based on `96d3c98`
on 13 November 2026, including the concurrent Copy ABI, native reload, live
reload and reload-renderer changes. The host was macOS arm64 with Rust/Cargo
1.99.1 and Apple Clang 17.0.0. It is not evidence that a release target,
remote CI job or every optional integration ran. No PLAN phase is marked
complete.

## Scope

The review fixes document the separate scalar i32, Copy-wire and host-state
reload APIs. The renderer example and integration tests share
`examples/reload-renderer/app.rs` and the private
`examples/reload-renderer/adapter.rs` loader. The integration test compiles a
GPU-independent C shared-library fixture and exercises the same `dlopen`,
`dlsym`, open/frame/close dispatcher path used by the example. It does not
include the whole example with a broad `dead_code` allowance. Public Copy
`Value` accessors and function decoders stay available and have focused
integration coverage.

## Commands and results

The final focused renderer commands below ran after restoring the C dispatcher
fixture and its temporary-directory drop guard. Other focused commands are
from earlier passes in this working tree. Commands were not run as a single
combined suite. The parent also reran the focused ABI, native reload and state
suites in both feature configurations, the documentation tests, docs generation,
link/search checks, formatting and both strict Clippy configurations.

| Command | Result |
| --- | --- |
| `cargo fmt --all` | Passed; formatted changed Rust files |
| `cargo fmt --all -- --check` | Passed after formatting and again after restoring the fixture |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed in initial fix pass and after final fixture change |
| `cargo clippy --locked --no-default-features --all-targets -- -D warnings` | Passed in initial fix pass and after final fixture change |
| `cargo test --locked --test copy_abi` | Passed, 9 tests; C bridge compiles and executes at `-O0` and `-O3` |
| `cargo test --locked --test native_reload_renderer -- --nocapture` | Initial pass: 4 tests passed. Final pass: 5 tests passed, including compiler-spawn cleanup and loaded C dispatcher coverage. Optional GPU test explicitly skipped because `RILL_RELOAD_RENDERER_EXE` was unset |
| `cargo test --locked --lib live_reload::` | Passed, 8 tests |
| `cargo test --locked --lib native_reload::copy::` | Parent final check passed, 3 tests |
| `cargo test --locked --test native_reload` | Passed, 7 tests |
| `cargo test --locked --test copy_abi --no-default-features` | Passed, 9 tests; C bridge compiles and executes at `-O0` and `-O3` |
| `cargo test --locked --no-default-features --test native_reload_renderer -- --nocapture` | Initial pass: 4 tests passed. Final pass: 5 tests passed, including compiler-spawn cleanup and loaded C dispatcher coverage. Optional GPU test explicitly skipped because `RILL_RELOAD_RENDERER_EXE` was unset |
| `cargo test --locked --no-default-features --lib live_reload::` | Passed, 8 tests |
| `cargo test --locked --no-default-features --lib native_reload::copy::` | Parent final check passed, 3 tests |
| `cargo test --locked --no-default-features --test native_reload` | Passed, 7 tests |
| `cargo run --locked --release --no-default-features -- docs --language -o target/site` | Passed; generated 38 HTML pages in initial pass |
| `python3 scripts/check-docs-links.py target/site` | Passed after final documentation edits; 38 pages, 3,802 links, 286 search addresses |
| `node scripts/test-docs-search.cjs` | Passed after final documentation edits |
| `cargo test --locked --no-default-features --bin rill docgen` | Passed, 24 tests; 17 other binary tests filtered out |
| `cargo test --locked --no-default-features --test docs_cli` | Passed, 13 tests |
| `cargo build --locked --example native_reload_renderer` | Passed after final adapter extraction |
| `cargo run --locked --example native_reload_renderer -- --headless --frames 3 --source examples/reload-renderer/logic.rill --checkpoint target/review-loop/example-state-final.json --opt-level 3` | Passed; printed three logic-only frames without graphics |
| `RILL_REQUIRE_GPU=1 RILL_RELOAD_RENDERER_EXE="$PWD/target/debug/examples/native_reload_renderer" cargo test --locked --test native_reload_renderer real_raylib_window_runs_without_restarting_for_logic_replacement -- --nocapture` | Exit 101; pkg-config could not find `rill-raylib.pc`, so no GPU execution was claimed |
| `pkg-config --exists rill-raylib` | Exit 1; Raylib SDK package is unavailable |
| `git diff --check` | Passed after final edits |

The final C fixture checks the real shared-library loader and dispatcher. It
observed one OPEN, nine FRAME calls, one CLOSE and the library destructor's
UNLOAD event for each O0 and O3 reload run. The lifecycle log is read after the
adapter drops and asserts CLOSE immediately precedes UNLOAD.

## Review disposition and final cleanup check

Three fresh-context review rounds ran, including the initial project review.
The final round confirmed the original findings were resolved and identified a
small temporary-directory leak if the renderer compiler could not start.
The parent restored a drop guard across compilation and loading, then added
`renderer_build_directory_is_cleaned_when_compiler_cannot_start`. This narrow
cleanup correction received parent inspection and focused validation, not a
fourth independent review round.

A standalone host check used a temporary `rill-raylib.pc` containing only
Name, Description and Version fields, with empty Libs and Cflags. It set
`PKG_CONFIG_PATH` to that fixture, `TMPDIR` to its temporary directory, and
`CC` to a nonexistent compiler, with `RILL_TOOLCHAIN_ROOT` unset. Running the
built example with `--frames 1` exited 1 with `compile renderer: No such file
or directory`. No `rill-reload-renderer-*` directory remained. This check
exercised compiler-start failure only, not Raylib or graphics.

The first post-cleanup Clippy attempt exited 101 because the parent had removed
the still-needed `PathBuf` import from the example. Restoring the import fixed
the compile error. Formatting, strict Clippy in both configurations, example
build and a three-frame O3 headless run then passed. The parent also confirmed
`git diff --check` passed. No full compiler suite was rerun for these fixes.

The review's proposal to remove public accessors solely because they had no
repository callers was rejected; their public behavior is now tested. A later
formatting accusation based only on source spacing was also rejected after
fresh `cargo fmt --all -- --check` runs returned zero.

## Graphics, importer and optional tools

The real reload-renderer GPU test was not accepted as executed evidence. The
required run failed with exit 101 because pkg-config could not find
`rill-raylib.pc`; `pkg-config --exists rill-raylib` returned 1. The regular
optional test printed its explicit skip because `RILL_RELOAD_RENDERER_EXE` was
unset. No real Raylib window, device or GPU readback was tested.

SDL3, importer/libclang execution, Zig target checks and AArch64 NEON checks
were not run as part of this focused verification. `zig` is present on PATH,
but this does not establish that a target check ran. No benchmark or timing
measurement was performed. Parent-side earlier full-suite output from before
this record is not used as GPU evidence because successful Rust tests capture
stderr, including optional-test skip messages.

## Limits

These commands verify the changed API/documentation/example seam on the local
host. They do not prove stable ABI compatibility, general owner migration,
foreign lifetime/effect contracts, callback retention, GPU resource migration,
Windows native loading, or any PLAN phase completion. The required real-GPU
reload run remains blocked on provisioning the `rill-raylib` pkg-config package
and a usable graphics device.
