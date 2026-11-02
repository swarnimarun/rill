# Delivery status

The gates in `PLAN.md` section 19 define completion. This file records progress,
not a replacement for those gates.

- P0: the supplied baseline and provisional decisions are documented. There
  was no previous compiler or test suite in this repository. Normative examples
  exist; review of the remaining decisions is still needed.
- P1: a sequential expression subset, type checker, reference interpreter and
  C emitter exist. Generic ADTs and exhaustive patterns work in the interpreter.
  Braced `fn` declarations and semicolon-separated blocks are supported;
  legacy expression syntax remains accepted. The C backend supports concrete
  payload layouts with bounded inline storage;
  reachable generic functions specialize for C; polymorphic local bindings,
  compact layouts, a lossless CST, a verified MIR boundary and the full
  acceptance suite remain open.
- P2: initialized mutable locals, nominal records, and call-scoped shared `read`
  and exclusive `inout` loans of roots or record field paths up to 16 segments
  are supported.
  A hosted `i32` box and a fixed-length owned byte buffer have restricted moves,
  direct owner returns with caller cleanup transfer,
  call-scoped shared/exclusive borrows and scoped cleanup in acyclic
  monomorphic owner programs. Finite concrete enums can contain nested owners;
  consuming matches transfer their fields and clean up the active variant.
  Structured branches can move enclosing owners when all arms agree on the
  resulting ownership state.
  `std.alloc` exposes recoverable system box/byte allocation through ordinary
  affine `Result` and canonical `AllocError` values, with checked sizes and
  fault-injection coverage. General move checking, loan inference, allocator
  capabilities and cleanup are not implemented.
  P3–P11 remain open; their dependencies and evidence remain as stated in the
  plan. Explicit native dependency directives, exact scalar/raw-pointer
  declarations,
  target-header compatibility checks and a direct SDL3 game exercise part of
  P5. Automatic declaration import, aggregate ABI adapters and checked foreign
  lifetime/effect/resource contracts remain open. See
  [C interoperability](c-interop.md). Static UTF-8 literals and
  fixed-capacity byte values support JSON validation and bounded decimal and
  string serializers, but do not establish the P3 owned-library gate. File-based
  imports, exports, nested local paths and explicitly bundled `std` modules
  exercise part of P4. Reusable native binding modules support exported
  functions/constants, private wrappers, transitive dependencies and checked
  duplicate declarations. Package and stable interface support remain open.
  `std.json_input.parse_i32` returns an ordinary `Result` with typed byte-offset
  errors. Explicit `u8x16` operations and opt-in AArch64 NEON exercise a
  small part of P10, without completing its optimization or conformance gate.

Run `cargo test` for the currently supported subset. The C differential tests
require `cc` and compare interpreter and emitted C results at `-O0` and `-O2`.
