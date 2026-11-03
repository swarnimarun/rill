# Optional function-instance origins

The experimental `typed::provenance::lower_main_with_function_origins` API
consumes a validated `modules::provenance::SourceProgram` and returns an immutable
`FunctionOriginProgram`. This is the first bounded origin-index increment,
**not a complete typed or generated-C source map**.

## Using the index

```rust
use rill::{modules::compile::load_program_with_sources, typed::provenance};

fn report_origins(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let sources = load_program_with_sources(path)?;
    let mapped = provenance::lower_main_with_function_origins(sources)?;
    for instance in mapped.instances() {
        let definition = mapped.definition(&instance.definition).unwrap();
        println!("{}: {:?} -> {:?}", definition.symbol.name,
                 instance.parameters, instance.result);
    }
    // Existing emitter APIs consume the same verified typed result.
    let _code = rill::typed::emit_c(mapped.typed())?;
    Ok(())
}
```

`instances()` follows final typed-function order. `requests()` begins with the
entry and then preserves deterministic discovery order, including requests to
already-discovered instances. A `RequestOrigin::Call` identifies both the final
caller instance and a source node accessible through `source_node()`. The node's
existing `SourceOrigin` entries refer to snapshots in `sources()`.

## Guarantees

- The pair retains the exact source snapshots and flattened program, ordinary
  verified `TypedMain`, reachable function instances and specialization requests.
  It uses the same semantic checker, worklist, lowering and verifier as ordinary
  typed compilation. Evaluator results and emitted C bytes are unchanged.
- A function instance identifies the actual resolved source declaration plus
  its complete closed parameter/result types. Parameter modes are retained.
  Nominal types refer to resolved type declarations and concrete arguments, not
  decoded internal names or generated C spellings. Definitions expose original
  names, visibility, module ownership and transferred AST targets already
  validated by the loader.
- Every discovery request is recorded, including duplicate callers and recursive
  edges. Caller and callee IDs undergo the worklist's existing final permutation.
  The entry request explicitly has no caller/call span. This finite graph is not
  a runtime stack trace. No traversal recursively expands a recursive call chain.
- Function definitions that were checked but never reached explicitly report
  `NotInstantiated`. Calls refer to validated source AST paths and retain the
  loader's original anchor/provenance without changing parser-generated origins.
- Opaque instance, definition and source-node handles are branded to the pair.
  Lookup rejects foreign domains and invalid slots. Moving a result or cloning
  the entire immutable result preserves its domain; every fresh lowering gets
  a new domain, even with identical input. Token identities are private, are not
  printed or serialized, and are not persistent cache identities.
- `sources()` exposes the existing read-only `SourceProgram`; its legacy raw
  SourceId/SymbolId/path APIs remain compilation-local and are not retroactively
  branded. Use the pair's branded query methods when mixing handles is possible.
  Source text remains available if original files change or disappear. There is
  no API to attach old metadata to a separately edited AST/typed result.
  `into_typed()` deliberately discards metadata.

## Validation and budgets

Tracing is optional. Its failures latch while ordinary lowering and independent
IR verification continue; ordinary errors take precedence. A successful opt-in
result requires exact instance/signature agreement, actual source declaration
and call identities, complete request coverage and unique caller/call-site
edges. Signature checks include result-only specialization arguments. Source
call coverage is independently enumerated from the loader's validated structural
nodes; it is not inferred from the observed edge count alone. Generalized value
initializers cannot contain function calls in the accepted subset.

The observer permits at most 1,000,000 request edges and 8,000,000 cumulative
metadata work units. Work is charged for visits, retained call-path steps,
source-index/validation steps and concrete type materialization. A call-edge cap
counts repeated call sites even when their target instance already exists;
semantic instance deduplication and its existing 1024-instance limit are unchanged.
The current path is maintained with depth checkpoints rather than copying it at
every expression. Existing source nodes are indexed by borrowed paths while
building the result; published call-site handles address their owned snapshot.
These logical work units count structural visits, paths and type nodes rather
than bytes in source names or allocator overhead. This is not a byte-accurate
memory cap or a whole-compiler resource guarantee.

Malformed metadata returns `E-TYPED-SOURCE-MAP`; metadata exhaustion returns
`E-TYPED-SOURCE-MAP-LIMIT`, only from the opted-in API. No partial successful
index, guessed source location or parent-span repair is returned. These checks
establish this index's correspondence, not a proof of all lowering behavior.

## Explicit exclusions

Native declarations/values and fallback-only programs remain unsupported by
this API; it does not perform header validation or change backend selection.
Ordinary native/fallback compilation is unaffected. The index records Rill
functions, not builtin calls, constructors or generated C helper functions.
There is no compiler-generated typed function producer in this increment; a
function without an actual source declaration fails explicitly rather than
borrowing a source span. A future generated-function producer must add an
explicit reason/relationship before it can enter this index.

This index does not map individual typed nodes, local bindings/storage,
captures, shape materializations, layouts, cleanup, native calls or generated C
byte ranges. It makes no cross-edit, portable-cache, debugger, runtime-stack,
complete source-aware diagnostic or phase-completion claim. Broader mapping
requires separate construction-site traces and coverage gates.

The shared lowering helper returns the existing owned `Instances` table for
finalization; specialization is not repeated. Source signatures are borrowed
from that table for validation and public conversion. The final permutation
uses bounded integer slots, without extra `Type`/scheme clones. Public concrete
type nodes and validation-time substitutions charge the metadata work budget
before allocation. Source definitions and call nodes refer to the retained
source table, and no inference pointer or variable ID escapes.

See the [fresh verification record](verification/2026-11-03-typed-function-origins.md)
for executed checks, independent-review findings and the remaining integration
gates.
