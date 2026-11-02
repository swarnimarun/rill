# C foreign calls

Explicit native declarations now support exact scalar types, static C strings,
raw nullable pointers and constants, with source dependency directives and
checks against installed C headers. See [C interoperability](../c-interop.md)
for syntax, target checking, compiler selection and runnable examples.
The [direct SDL3 game](../../examples/sdl3-direct/README.md) runs without a shim;
its integration test uses the actual installed library when available.
The older SDL3 and raylib examples retain their C shims. Native declarations
can live in reusable binding modules, with exported functions/constants or
private declarations behind exported Rill wrappers. Dependency directives
aggregate transitively; compatible repeated C declarations coalesce and
conflicting kinds or signatures are rejected. Module visibility applies to
qualified foreign symbols.

Rill checks inference and moves. The target C compiler checks explicit function
and constant declarations against the selected headers. Manual linking without
headers still relies on the programmer's signature assertion. Neither path
checks foreign effects, pointer lifetimes or library resource cleanup.

## Remaining work

`PLAN.md` sections 15 and 19 require more than this direct-call subset:

- Import declarations automatically through a target-configured C frontend.
  Expand supported types, calling conventions and target layout queries.
  Do not equate `long` or `size_t` to a fixed Rill width.
- Add an enforceable unsafe/effect boundary, including unknown foreign effects,
  allocation, blocking, pointer retention, ownership and thread requirements.
- Support ABI adapters and checked layouts for by-value structs, event unions,
  callbacks and retained pointers. Matching size alone proves no ABI contract.
- Add dynamic C-string conversion and checked pointer access with explicit
  validity and lifetime rules. Raw handles are not safe Rill references.
- Extend resource contracts, bidirectional ABI tests, cross-compiler coverage
  and supported-target evidence. A headless SDL test cannot prove all library
  behavior or desktop event handling.

P5 remains open. Header compatibility validates declarations, while the
programmer remains responsible for foreign behavior and resource destruction.
