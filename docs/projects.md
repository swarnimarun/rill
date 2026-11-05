# Local projects

Create a project and run it through the ordinary checked compiler:

```sh
cargo build --locked
rill new my-tool
rill project check my-tool
rill project run my-tool
rill project build my-tool
rill project docs my-tool
./my-tool/build/my-tool
```

Use the built `target/debug/rill` executable when `rill` is not installed on
`PATH`. `new` requires a new directory and an existing parent directory. It
creates `rill.json`, `src/main.rill` and a `.gitignore` entry for `/build/`.
An existing directory is always an error; existing files are not overwritten.
Directory names containing spaces are accepted. The generated package name
replaces punctuation with `-` and starts with a letter.

A project has one executable entry point and local imported source modules:

```json
{
  "version": 1,
  "name": "my-tool",
  "entry": "src/main.rill",
  "output": "build/my-tool"
}
```

`version`, `name` and `entry` are required. `output` is optional and defaults to
`build/<name>` with the host executable suffix. Names contain 1–64 ASCII letters,
digits, `-` or `_`, starting with a letter. Unknown or duplicate fields reject.
The manifest must be a JSON object of at most 65,536 bytes. Entry and output
paths are relative to the project root and cannot contain `.` or `..` segments.
The entry must be an existing `.rill` file inside that root. Output cannot
replace a `.rill` source, the manifest or a directory. Entry and output symlinks resolving
outside the project root reject. Inputs must remain stable during a command;
this is a development driver, not a filesystem sandbox.

When already inside a project, omit the directory:

```sh
rill project check
rill project run --interpret
rill project build --opt-level 0
rill project build -o build/debug-tool
```

`project` changes the compiler subprocess's working directory to the project
root. Relative compiler, include, native configuration and linker paths
therefore resolve from the project root. Options pass as structured process
arguments, with no shell evaluation. The default native optimization is the
ordinary compiler's `-O3`; `--interpret` retains the existing evaluator limits.
A build creates missing output parent directories. An explicit `-o` overrides
the manifest output and follows the same relative-path restrictions.

The checked module graph, ABI validation and ownership rules are exactly those
of the corresponding `check`, `build` and `run` source-file commands. A failed
compiler command fails the project command. This format adds no registry,
dependency downloader, build scripts, library ABI, test runner or stable package
interface.

Run the checked-in module example with:

```sh
cargo run -- project check examples/project
cargo run -- project run examples/project
cargo run -- project build examples/project
```

It prints `42`. Other working program examples are indexed in the repository
README and retain their existing source-file commands.

## Rebuild and restart

```sh
rill project watch my-tool
rill project watch my-tool --interval-ms 200
rill project watch my-tool --once
```

`watch` rebuilds changed project input and restarts the native process. Project
watch follows the entry file and its transitive local imports, and also observes
`rill.json`. Add native inputs or other assets using repeated `--watch <path>`.
Unused source libraries do not trigger rebuilding. It uses the entry and options
resolved when the command started. Restart `project watch` after
changing the manifest's entry or output recipe. This mode does not preserve
process state or replace live function bodies.


## HTML documentation

```sh
rill project docs my-tool
rill project docs my-tool -o public/reference --open
```

The docs command validates the manifest and generates language guides, project
README/docs Markdown and parsed source API pages. Output defaults to
`build/docs` under the project root; relative `-o` paths use that same root.
It does not compile or execute the project or import native headers. The
self-contained site has search, page/section links and local assets, and can
be opened from disk or copied to a static host. See
[the docs command contract](docs-command.md) for source comments, safe output
regeneration and limits. `rill docs` also detects the current project;
`rill docs --language` generates only the bundled language manual.
