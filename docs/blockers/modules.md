# File modules

The file-based `check`, `run`, `build`, and `emit-c` commands support a small
module subset. A leading `import name;` or `import name as alias;` loads
`name.rill` from the entry file's directory. Qualified imports such as
`import helpers.math;` load `helpers/math.rill` under that same directory,
including when imported transitively. The default alias is the last segment. Each path segment and alias uses lowercase
ASCII letters, digits and underscores, beginning with a letter. Imports must
precede declarations. Qualified references such as `alias.function(...)` and
`alias.Constructor` resolve only through that alias; they do not search other
modules.

Declarations are private by default. `export fn` or legacy `export let` exposes a function, and
`export type` exposes a nominal type and all of its constructors. Exported
types and function signatures cannot expose private nominal types. Imported
modules cannot define `main`. The entry file must
define a zero-argument `main`. Each module is resolved independently before
the combined program goes through the existing type and loan checker. The
interpreter and C emitter use that checked program. An example lives in
[examples/modules](../../examples/modules/main.rill).

Binding modules can declare foreign functions and constants. Use
`export extern fn` or `export extern const` for qualified access by importers,
or keep declarations private behind exported Rill wrappers. Foreign calls
retain their raw C symbol names; ordinary Rill functions retain distinct
internal names. Compatible declarations of the same C symbol coalesce, ignoring
parameter names. Conflicting kind or exact type signatures report
`E-MODULE-EXTERN`. Existing visibility rules apply to qualified native symbols.
Native dependency directives aggregate transitively and select native `run`
automatically. See [C interoperability](../c-interop.md) and
[the SDL binding module](../../examples/sdl3-direct/sdl.rill).

`import std.json_input;`, `import std.json_output;`, `import std.result;`, and `import std.alloc;`
load sources embedded in the compiler. `std` is a reserved import root;
unknown names report `E-MODULE-STDLIB` and never fall back to local files.
Bundled modules may depend only on bundled modules. Imports remain explicit,
and `std.result` is an ordinary exported ADT without special error semantics.
See [library APIs](../../stdlib/README.md) and
[the runnable example](../../examples/stdlib/main.rill).

The in-memory `compile(source)` API has no filesystem context and continues to
reject imports and qualified syntax. The graph loader canonicalizes paths and
rejects paths outside the entry directory, duplicate imports or aliases,
missing files, cycles, invalid UTF-8, and inaccessible exports. It caps each
source file at 1 MiB, graph size at 256 files and import depth at 64. It
processes dependencies in sorted name order and retains each source snapshot.
`Module.path` is canonical for local files and synthetic, such as
`<std.json_input>`, for bundled sources. Bundled diagnostics do not depend on
the compiler build machine's source directory. The same graph limits apply
to bundled modules.

Path confinement is not race-safe against an adversary replacing paths while
they are being loaded. Use only trusted, stable source trees. On Unix, duplicate opened file identities are rejected, including hard links
with different paths. Other platforms currently compare canonical paths only.
Race-resistant handle-based loading and portable file-identity checks are
needed before claiming hostile-filesystem safety.

## Remaining work

- Add spans and source identities throughout the resolved and checked
  representations. Some errors do not yet identify the original declaration
  or use precisely. The current resolver lowers qualified names to internal
  collision-free names before checking; it is not the verified MIR described
  in `PLAN.md`.
- Introduce explicit signatures and true opaque types. Exporting a type
  currently exposes its constructors; name visibility alone is not an opaque
  representation contract.
- Define package roots, versioning, local packages and directory imports.
  Nested file paths are supported only within the entry directory. Reusable
  native modules do not yet provide automatic header declaration import,
  checked foreign effects or foreign resource ownership contracts.
- Persist canonical interfaces with type schemes, ownership and region
  relations, effects, layout obligations, and version information as those
  features are implemented. Add cache identities, separate compilation, and a
  compatibility policy. An implementation change must not silently widen a
  published effect contract.
- Expand cross-file fixtures for nominal identity, transitive exports, malformed
  interfaces, source changes during loading and race-safe paths. Check compiler
  resource use on large dependency graphs.

The current functionality is a useful file-module subset, not completion of
P4 or a stable package and interface system.
