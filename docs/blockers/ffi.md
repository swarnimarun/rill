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

Checked typed native transport now covers the existing foreign scalar/pointer
values, calls/constants, conversions, null operations and same-type equality,
composed with supported closed Copy generics, aggregates and slot loans. It
preserves the complete declaration manifest, target-header assertions and
left-to-right snapshots, including once-only local constant captures. The
evaluator rejects reached native work and does not call host C. Unrelated
unsupported Copy layouts retain whole-program uniform fallback. Exact pointer
identity and slot constness do not prove lifetime, effects or pointee independence.
See [F2 verification](../verification/2026-11-03-typed-native-transport.md).

## Remaining work

`PLAN.md` sections 15 and 19 require more than this direct-call subset:

- Expand the optional libclang selected-declaration importer beyond the
  current scalar/raw-pointer and integer/bool constant subset. Add aggregate
  adapters, callbacks, calling conventions, target/package configuration
  persistence, non-Unix worker supervision and full import cache identities.
  The Unix CLI now isolates complete import generation with bounded IPC and
  a cancellable deadline; the library API remains in-process, and neither is
  a memory/security sandbox.
  `long` and `size_t` are accepted only when their canonical C type matches a
  supported target stdint type; matching width alone is insufficient.
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
