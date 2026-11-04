# Source origins after module lowering

`modules::compile::load_program_with_sources(path)` is an experimental, opt-in
P1/P4 source-provenance increment. It returns `modules::provenance::SourceProgram`
only after the same bounded graph loader, strict parser, resolver, foreign
_declaration_ validation, checker and exported-interface privacy checks used by
`load_program` succeed. The flattened `Program`, including native configuration
order and canonical allocation identities, is the same. This is not a new
resolver, grammar, semantic compiler path or completion of a PLAN phase.

Ordinary `load_program` and binding checking do not construct source maps.
Existing compilation errors retain their code, message and precedence. Once
ordinary checking succeeds, an internal correspondence failure in the opt-in
pass returns `E-MODULE-SOURCE-MAP`, never partial or guessed metadata. Neither
loader invokes native tools or verifies the C header ABI; the driver's existing
native-validation step is still required. Reading metadata does not load files,
resolve packages, run external tools or repeat checking.

## Owned result and lookup

- `program()` borrows the exact immutable flattened `Program`. `into_program()`
  consumes the result and discards its provenance. A separately cloned/edited
  `Program` has no claim to the old metadata.
- `root()` and `sources()` retain the loader's root and sorted source snapshots.
  Each snapshot has a `SourceId`, logical module name, exact UTF-8 text and
  `PathBuf`. Local paths remain canonical, including non-UTF8 path bytes on
  platforms that support them. Bundled modules retain `<std.name>` diagnostic
  paths. These are the loaded snapshots, not promises about later file contents.
- `nodes()` contains every structural AST node in the flattened program:
  declarations, variants, parameters, written types, expressions, patterns,
  match arms and foreign type slots. Entries follow sorted module order, source
  declaration order, then semantic preorder. Coalesced declarations contribute
  origins to existing entries rather than duplicate output nodes.
- `origin(&AstPath)` retrieves a `ResolvedNode` and all its `SourceOrigin`s.
  `node(&AstPath)` resolves that path into this result's exact program using
  the shared `AstPath::resolve_program` accessor. Invalid paths return `None`.
- A `SourceOrigin` contains the original source's structural path, source ID,
  copied UTF-8 byte/line/scalar span, grammar kind, parser provenance, native
  contribution category and module-lowering category. `origin_text` reads its
  exact anchor from the owned snapshot. Spans are **source anchors**, not
  necessarily whole semantic-subtree extents: a braced binding's continuation
  can have a later span outside the binding's declaration anchor.
- `symbols()` exposes the actual resolver symbols, original names, visibility
  and module ownership, source paths and flattened target paths. Top-level
  declarations target roots; constructors target type variants, including
  synthetic record variants. `symbol(id)` looks up one identity. Different
  modules' compatible native declarations retain different symbol identities
  even when they target the same output declaration.

`SourceId`, resolver IDs and paths are local to one load. They are not branded
against accidentally mixing separate results, stable across edits or suitable
for incremental caches. Use only IDs and origins obtained from the same result.
No borrowed syntax tree, tree-local `NodeId`, AST pointer identity or fabricated
`ParsedModule` envelope is retained. The result remains valid after the files
change or are deleted and after moving it to another thread.

## Recorded transformations and coalescing

The existing lowering pass records source/destination declaration indices at
its actual append/coalescing sites. Its existing recursive rewrite records the
structural role of each child while resolving names. `Lowering::ResolvedName`
records the actual resolved symbol, including name resolution that retains the
entry `main` spelling. `Lowering::QualifiedValue` explicitly records the one
shape change: a module-qualified `Field` expression becomes a resolved `Var`.
A field rooted in a local shadowing an import alias remains a local field.
Builtins, local variables and untouched nodes use `Lowering::Unchanged`.

After successful semantic checking, the source-map pass replays the **same
parser** over the already-loaded snapshots and builds their optional AST maps.
It joins by recorded source/declaration/child slots, never by name, equal AST
payload, source-text search or memory address. It validates source and output
node categories and child roles, expression/pattern/foreign-type shapes, name
rewrite targets, unique transfers, complete output coverage and exactly one resolver identity
for every original declaration/variant (including coalesced declarations).
Original name, kind, export flag, module owner and transferred target are checked
against the exact source slot and recorded transfer. Name comparison
is an invariant check of an already-recorded symbol target, not a way to find
correspondence. Grammar IDs are consumed only while their tree is alive; owned
spans, grammar kinds and explicit parser provenance survive it.

For compatible native duplicates, every declaration is validated before its
coalescing record is accepted. Each output node has one `Contribution::Retained`
origin followed by every `Contribution::CoalescedNative` origin in sorted module
order. Parameter names in compatible signatures can differ, so a coalesced
parameter origin describes its contributing ABI position, **not** the literal
name retained in the output. Declaration, parameter and type origins are all
preserved; invalid duplicates still fail the ordinary loader. Raw C names,
privacy, native dependency order and allocation identities are unchanged.

The seven parser-origin categories from [parsed AST origins](ast-source-origins.md)
are retained separately from module rewriting. Generated block bindings,
sequences, discard nodes, units, unary zeros and record variants keep explicit
provenance; the result never presents their anchors as written generated text.

## Limits

This API retains private source declarations because they are inputs to the
same compilation. It does not make private symbols accessible to source code,
change exported interfaces, or authorize compilation. Imports and native
configuration strings remain in snapshots/CST syntax, not separate semantic
AST nodes. Local binding identities and every scalar name/field/place component
are not independently resolved symbols in this metadata.

The graph loader's source-size, count, depth, path and cycle checks remain in
force, as do ordinary parser/checker limits. The optional map adds memory and
parse work; it is not a separately budgeted editor service or security sandbox.
Mappings for inferred types, specialization, checked typed IR/MIR, cleanup,
generated C, debugger locations, incremental queries, an LSP and source-aware
checker diagnostics remain open. See [verification](verification/2026-11-03-module-source-origins.md).

## Library buffers and editor overlays

`modules::load_with_overlays` loads the same bounded graph with immutable open
buffer text overriding reached files. Keys may be absolute or relative to the
entry directory. New unsaved files require an existing parent directory and a
matching overlay. Unreachable overlays do not expand import access. Canonical
paths, hardlink identity, root containment, cycles and source-size limits still
apply; loading does not alter disk contents.

`compile::load_program_with_sources_and_overlays` retains those exact snapshots
and source origins. `load_library_with_sources_and_overlays` uses the same
checks without requiring a `main`; an existing `main` still needs zero
parameters. Both retain signature budgets, nominal identity, privacy and
ownership validation. This foundation does not launch external native tools.
