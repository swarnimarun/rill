# Blocks and source native dependencies

Status: accepted for the experimental implementation, 2026-11-03.

The user requested Rust implementation, C as the first output target, explicit
blocks and statement delimiters, and direct use of system libraries such as
SDL3. These choices refine the surface syntax and first C integration milestone
in `PLAN.md`; they do not complete its stability gates.

## Blocks

Use `fn name(args) { ... }`, braced control flow, `let` statements and `;`
delimiters. A final expression is the block's value; an empty block or trailing
semicolon yields unit. A discarded expression gets its own cleanup scope.
An explicit local remains alive until its enclosing scope ends, unless moved.
Legacy expression syntax remains accepted while existing fixtures migrate.

The parser reports the current token's line and column for missing separators
and delimiters. Source spans throughout inference and a lossless syntax tree
remain separate diagnostic work.

## Native dependencies and checks

`@include`, `@link` and `@pkg_config` declare dependencies in the source preamble.
All commands use the same checked Rill module graph. Native `check` and `emit-c`
also ask the selected C compiler to validate explicit declarations against
included headers. Build and native run use those compiler flags and resolved
library dependencies. Tool arguments never pass through a shell.

The compiler checks exact function types, including widths, signedness and
pointer constness. Integer constants must fit the declared type before
conversion. It does not infer ownership, lifetime, retention or effects from a
C header. Raw pointers remain foreign handles with explicit programmer-managed
resource cleanup. Automatic declaration import and broader ABI support remain
open in [the FFI blocker](../blockers/ffi.md).

Source `@link` declarations supplement the jointly resolved pkg-config
dependencies. They do not add a duplicate `-lName` when already present. This
prevents duplicate SDL3 dylib linkage observed with Zig on macOS. The compiler
preserves pkg-config's output sequence and explicit `--link-arg` arguments,
including repeated libraries required for static-library ordering.

## Evidence

Parser fixtures cover delimiter errors and block cleanup against interpreter
and C execution. Native fixtures cover ABI mismatches, wide values, opaque
pointer identity, null inference, compiler argument handling and dependency
ordering. The direct SDL3 game has run for three frames under the dummy video
driver using both `cc` and `zig cc`. Physical mouse interaction remains a
manual desktop check.
