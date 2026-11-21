# Owned module source origins, 3 November 2026

This increment was implemented on reconstructed AST-origin checkpoint
`e183244f5813a82338e14821c4df031d5138ec1b`. The original cloud filesystem and its
missing source-map checkpoint were not recovered. No old verification is claimed
for this implementation. See [the API contract](../module-source-origins.md).

The opt-in loader records declaration transfers, actual resolver symbols and
name/qualified-value changes in the existing lowering pass. Only after ordinary
checking succeeds does it replay the shared parser over loaded snapshots and
validate owned source origins for every flattened structural node. Native
coalescing keeps each validated contributor. Ordinary APIs, source semantics,
privacy, canonical allocation identities and native dependency order remain
unchanged. There are no checker, typed IR or backend changes.

## Fresh checks

Rust/Cargo 1.99.1 on x86_64 Linux with an isolated target directory:

```sh
cargo test --locked --lib modules::provenance -- --nocapture
cargo test --locked --test module_origins --test module_compile --test module_graph --test module_audit --test native_modules --test bundled_stdlib --test fallible_allocation_module --test ast_source_map --test syntax_tree
cargo test --locked --features c-import --lib modules::
cargo test --locked --features c-import --test module_origins --test native_modules
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --features c-import -- -D warnings
```

All passed: 68 affected default integration tests plus 4 private invariant tests;
with `c-import`, 7 module unit tests and 18 integration tests. The 13 new public
acceptance tests cover:

- Exact `Program` equality between the ordinary and opt-in loader, including
  native metadata and allocation identities; all 33 repository examples give
  the same accepted program or exact error (29 accepted, 4 rejected).
- Nested imported modules, aliases, private/exported symbols, same-named
  declarations, reordered import discovery and local alias shadowing.
- Resolved declaration/constructor identities, explicit qualified `Field` to
  `Var` changes, repeated identical expressions and distinct source occurrences.
- All parser provenance categories, including record variants, unary zero,
  sequence/discard/unit generation and block anchors that do not contain their
  continuation's later source span.
- Compatible foreign functions/constants retaining both original identities
  and all structural origins, including differently named ABI parameters;
  invalid duplicate parameter names and incompatible signatures still fail.
- Bundled allocation/result/JSON sources, canonical allocation identity and
  original native include/link/package ordering.
- UTF-8 byte/scalar anchors, exact retained CRLF/comments, moving the result to
  another thread after source replacement/deletion, and non-UTF8 local paths.
- Import, cycle, private type leakage, parse/name/export errors, symlink escapes,
  hardlink identities and existing source-size/module-count/import-depth limits.
- Complete coverage through near-limit expressions, block bindings and nested
  type annotations on a two-MiB thread stack.
- A subprocess with trap `cc`, `clang` and `pkg-config` executables and nonexistent
  header/package names: loading and repeated metadata reads launch no native
  validation tool. This does not claim that the ABI was checked.

Private injected-failure tests reject missing, duplicate and invalid transfer
slots; unrecorded name/shape changes; invented qualification markers; missing, duplicate or inconsistent
source symbol identities; malformed snapshots; and extra output nodes. Failures return
`E-MODULE-SOURCE-MAP`, with no partial result. Fresh valid parsing/mapping remains
available afterward. Independent review found that the first frozen version
could accept an injected missing or renamed symbol table. The follow-up adds
complete source declaration/variant symbol coverage and validates each original
name, kind, export flag, owner and transferred target without a second resolver.
The regression includes both review reproductions and additional corrupted
kind/export/owner/duplicate-slot cases. All listed focused checks were rerun
after this correction.

## Independent pre-increment comparison

The clean `e183244` library was built separately. A small identical Rust harness
called ordinary `load_program` from the old and new libraries for all 33 example
inputs, serializing the complete `Program` or exact error. Results were
byte-identical, including native configuration and allocation metadata.
The evidence ledger retains both dumps, the harness and the comparison log;
the common dump SHA-256 is
`0e15fcffc38d46d8a74007cbeaca1f0ee76192051dbfdead6e499758d1f5196e`.
This supplements current-API equality tests rather than assuming that two paths
through the modified implementation prove its refactor unchanged.

## Remaining gates

Independent review of this frozen increment and combined full default/`c-import`
regression remain integration gates owned by the parent continuation. No full
aggregate was launched concurrently from this branch. GitHub publication remains
blocked separately; only local commits are authorized here.

Typed/inferred/backend origins, source-aware checker diagnostics, incremental
cache identity, a separately budgeted editor service, LSP integration and the
full P1/P4 PLAN acceptance gates remain open.
