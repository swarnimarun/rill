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

## Native and shader recipes

Version 2 retains the required name and entry fields and accepts optional native
and shader recipes. Version 1 continues to reject those additional fields.

```json
{
  "version": 2,
  "name": "engine-3d",
  "entry": "src/main.rill",
  "output": "build/engine-3d",
  "native": {
    "sources": ["native/renderer.c"],
    "include_dirs": ["native"],
    "packages": ["sdl3"],
    "libraries": ["m"]
  },
  "shaders": "rill.shaders.json"
}
```

Every native field is optional and accepts at most 64 unique strings. Sources
must name existing `.c` files, include directories must exist, and all native
and shader manifest paths stay inside the project. Package and library names
use the same validation as source `@pkg_config` and `@link` declarations.
Unknown fields and duplicate keys reject, including keys inside `native`.
Recipes invoke compiler tools with structured arguments and do not execute
shell commands or project build scripts.

`project check` checks the native C sources without publishing objects and
compiles/validates shaders in a temporary directory. `build`, `run` and every
watch candidate rebuild native objects in a unique temporary directory and
prepare the shader artifacts. Objects precede dependency libraries at link
time and remain present until linking or execution finishes. Native compilation
still runs after a generated-C cache hit. A failed native compilation does not
replace an existing executable. Shader publication preserves the previous
complete artifact directory if a shader build fails.

`shaders` selects the manifest used by `rill shader`. See
[shader tooling](shaders.md) for targets and validation rules. Native run/watch
processes receive absolute `RILL_PROJECT_ROOT` and `RILL_PROJECT_SHADER_DIR`
environment variables. Programs can locate shader assets through the latter
even when the watcher uses a temporary executable path. Directly running a
built executable does not set these variables; applications should also support
an installation-relative asset path. Raw source commands can apply the same
recipe with `--project-root <directory>`, and the source must match its entry.

Native recipes reject `--interpret`. Project recipes cannot combine with a
saved `--native-config` recipe, since each would otherwise define a separate
native configuration. Shader compilation and native headers do not establish
GPU resource lifetimes or a Rill shader type system.

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
`rill.json`. Version 2 automatically watches native source/header directories,
the shader manifest and shader source/include inputs. Generated shader output
is excluded, including custom output directories. Add other assets using repeated
`--watch <path>`. Unused source libraries do not trigger rebuilding. Native and
shader recipes reload after manifest edits, while entry/compiler options remain
those selected at startup. Restart `project watch` after changing the entry or
compiler options. This mode does not preserve
process state or replace live function bodies.

Project build/run/watch enable checked generated-C reuse by default, using
`build/.rill-cache` inside the project. Use `--cache-status` to see hits and
misses, `--no-incremental` to disable reuse, or `--cache-dir build/other-cache`
to select another confined project directory. Every native build still compiles
and links C using the current options and native inputs. See
[incremental build contracts](build-performance.md#incremental-checked-c-reuse).


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
