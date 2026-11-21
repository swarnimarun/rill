# Learn Rill in the playground

Build a signal amplifier, inspect how the compiler types it, and compare its interpreter result with native C11. The playground runs the Rust compiler on your machine each time you submit an edit. All commands in this chapter start at the repository root. Shell examples use POSIX syntax.

## Start the workbench

Install a Rust toolchain with Cargo and a host linker. Cargo needs the dependencies recorded in `Cargo.lock` on the first build. Start the server and open the address it prints. The default is `http://127.0.0.1:7878/`.

```sh
cargo run --locked -- playground
```

Leave the process running while editing. Use `Ctrl-C` in the terminal to stop it. If the port is occupied, run `cargo run --locked -- playground --port 0` and open the printed address. The **Language book** link opens the searchable manual from the same compiler build. The interactive service listens on the local IPv4 loopback interface.

Select **Shape a signal**. Its `main` calls `amplify(14, 3)`. Run it to see `42`. Change the sample from `14` to `25`, predict the value, and run it again. The product is 75, so the cap returns `60`. Now remove the semicolon after `let level = sample * gain`. Inspect the parser diagnostic, repair the edit and compare the tokens, syntax tree and inferred signatures. [How the compiler handles this program](compiler-core.md)

## Work through the lessons

| Lesson | Observation | Experiment |
| --- | --- | --- |
| Shape a signal | A final expression returns a block's value. | Move the sample across the clipping threshold. |
| Read an inferred type | `keep` works at two types in one program. | Make the two arms of `if` return different types. |
| Handle a missing reading | `if let` binds a payload only on success. | Replace `Some(18)` with `None` and predict the fallback. |
| Move a cursor | Writing through `inout` changes the caller's field. | Remove `mut` from the caller's binding. |
| Follow an allocation | A read borrow leaves a box available for its later move. | Inspect the host Typed IR, then read the caller's binding after the move. |
| Repair a type error | The checker identifies the right operand of integer addition. | Convert the boolean to an integer with an explicit conditional. |
| Trace a native branch | The C11 output has a conditional and an executable entry point. | Change the input's sign and compare the two results. |
| Classify GPU lanes | One kernel maps `-1, 0, 7, 8` to `0, 1, 1, 2`. | Inspect the two signed comparisons in Rill IR and change the upper threshold. |

The lesson buffers are real files under `examples/playground/`. The sixth lesson deliberately fails checking until you repair it. The playground accepts one source buffer per inspection. Use the CLI for projects with local modules, native headers, library linking, terminal I/O or graphics. Reading C11 or target-specific LLVM text in the output pane only requires the Rill compiler. Running a native executable needs a C11 compiler. Running device code needs a compatible target toolchain and driver.

## Run the same source from a terminal

Build once. In the following commands, `target/debug/rill` is the freshly built executable. On Windows its name is `target/debug/rill.exe`. `cargo run --locked --` is an alternative wherever the command begins with the executable.

```sh
cargo build --locked
work=$(mktemp -d)
target/debug/rill check examples/playground/signal-gain.rill
target/debug/rill fmt examples/playground/signal-gain.rill --check
target/debug/rill run examples/playground/signal-gain.rill --interpret
target/debug/rill emit-c examples/playground/signal-gain.rill > "$work/gain.c"
target/debug/rill build examples/playground/signal-gain.rill --opt-level 0 -o "$work/gain"
"$work/gain"
target/debug/rill run examples/playground/signal-gain.rill --opt-level 3
```

Expect three `42` results, one each from interpretation, the built executable and the optimized native run. `check` and `fmt --check` succeed quietly for this file. To see the formatted source, omit `--check`. `emit-c` writes C11 to standard output. By default `run` and `build` invoke the selected host C compiler with optimization level 3. [Native compiler options](native-builds.md)

To run a multi-file application, use the checked-in project or create a fresh one. Each project has an entry point and a versioned `rill.json`. The bundled example imports a local math module and returns 42.

```sh
target/debug/rill project check examples/project
target/debug/rill project run examples/project --interpret
target/debug/rill new "$work/my-tool"
target/debug/rill project build "$work/my-tool"
target/debug/rill project docs "$work/my-tool"
target/debug/rill project watch "$work/my-tool"
```

The watcher runs until interrupted. Edit a source file while it is running to trigger a rebuild. The standalone language manual opens directly from disk and can also be copied as a directory to a static host. [Project workflows](projects.md) · [Documentation command](docs-command.md)

```sh
target/debug/rill docs --language -o "$work/book"
python3 scripts/check-docs-links.py "$work/book"
```

Open `index.html` in that output directory. Use the search box to find “ownership”, “kernel” or “inference”. Keep the server running for interactive compilation.

## Launch the three applications

These commands use the CLI and a C11 compiler. Start with a new temporary directory so the database and editor open disposable files.

```sh
cargo build --locked
work=$(mktemp -d)
printf 'lllq' | target/debug/rill run examples/scene-engine/main.rill

cc -std=c11 -O2 -c examples/modal-editor/terminal.c -o "$work/terminal.o"
target/debug/rill build examples/modal-editor/main.rill --link-arg "$work/terminal.o" -o "$work/editor"
printf 'ihello\n\033wq' | RILL_EDITOR_FILE="$work/notes.txt" "$work/editor"
cat "$work/notes.txt"

cc -std=c11 -O2 -c examples/embedded-db/file.c -o "$work/file.o"
target/debug/rill build examples/embedded-db/main.rill --link-arg "$work/file.o" -o "$work/db"
printf 'put 10 42\nchanges 0\n' | RILL_DB_PATH="$work/local" "$work/db" > "$work/changes"
RILL_DB_PATH="$work/replica" "$work/db" < "$work/changes"
printf 'get 10\n' | RILL_DB_PATH="$work/replica" "$work/db"
```

The terminal scene ends with a score of 1. The POSIX editor saves a line containing `hello`. Run `RILL_EDITOR_FILE="$work/notes.txt" "$work/editor"` in a terminal to edit it interactively. The database query prints `42` from the replica. To open the SDL3 scene window, install SDL3 development files, pkg-config and libclang for the default C importer, then run the command below. [Scene engine](../examples/scene-engine/README.md) · [Editor controls and storage](../examples/modal-editor/README.md) · [Database file format and sync benchmark](../examples/embedded-db/README.md)

```sh
target/debug/rill run examples/scene-engine/window.rill \
  --c-arg -Iexamples/scene-engine --link-arg examples/scene-engine/scene_window.c
```

## Inspect a compute kernel

Start with CPU evaluation of a verified Rill kernel. The first command extracts the named unary function. Each of the four signed inputs is one independent invocation.

```sh
work=$(mktemp -d)
target/debug/rill kernel lower examples/playground/signed-bands.rill band > "$work/band.rir"
cat "$work/band.rir"
target/debug/rill kernel eval "$work/band.rir" -1 0 7 8
target/debug/rill kernel emit-llvm nvptx "$work/band.rir" > "$work/band-cuda.ll"
target/debug/rill kernel emit-llvm vulkan "$work/band.rir" > "$work/band-vulkan.ll"
```

Read the generated modules next to one another. CUDA receives device pointers and the element count as kernel arguments. Vulkan reads buffers and the count through descriptor bindings. Both guard an access with the lane index and count. For actual GPU execution, follow the compiler-version, hardware and validation commands in the GPU chapter. Its supplied device smoke harnesses use the **clamp** fixture and compare against clamp's results. [LLVM and device execution guide](gpu-compilation.md)

## Connect a real editor

`target/debug/rill lsp` serves the Language Server Protocol over standard input and output. Configure an editor client to start that executable with the single argument `lsp` for files with extension `.rill`. Use an absolute executable path if the editor starts outside the checkout. Open `examples/playground/signal-gain.rill`, hover over `amplify`, navigate to its definition and remove a local-binding semicolon to exercise diagnostics. Use the client-specific guides to install syntax highlighting and connect the same binary to the editor. [Helix setup](../editors/helix/README.md) · [Neovim setup](../editors/neovim/README.md) · [VS Code setup](../editors/vscode/README.md) · [Supported editor requests](editor-support.md)

## Test your understanding and changes

Run the lesson contract after changing a lesson or compiler result. In `examples/playground/lessons.json`, each stable ID names its `.rill` source and initial view. For successful Run, Type check and Kernel evaluator lessons, `--check-lessons` compares the entire trimmed output with `expected`. For compiler dumps and deliberate rejections, it looks for the expected substring in the output or diagnostic respectively. Use `"ok": false` when rejection is the teaching point. An exercise describes a modification to try after the initial observation. Test that edit separately before publishing a lesson.

Focused tests let you follow one subsystem at a time. Rustfmt and Clippy are Cargo toolchain components. Native tests use `cc`. C import tests also need libclang. SDL3, Zig and architecture-specific checks depend on the corresponding host capabilities.

```sh
cargo run --locked -- playground --check-lessons
cargo test --locked --test syntax_tree --test operand_diagnostics
cargo test --locked --test core_models --test option_core_proof
cargo test --locked --test kernel_cli --test lsp_protocol --test docs_cli
cargo fmt --all -- --check
cargo test --locked --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

To exercise the browser controls, start the playground in one terminal, then use `node scripts/test-playground-browser.cjs http://127.0.0.1:7878/` in another. The browser test needs Node.js, Playwright or playwright-core and a Chromium installation. Set `CHROMIUM_EXECUTABLE` to a compatible browser executable if using an existing system installation.

The proof artifact can be checked separately with Lean 3.51.0 using `lean proofs/OptionCore.lean`. Its theorems concern a closed Option/conditional calculus. The type models separately exercise the production checker, interpreter and generated C against finite independent oracles. [Proof scope](option-core-proof.md) · [Type models](type-models.md)
