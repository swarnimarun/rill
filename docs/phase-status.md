# Delivery status

The broad compiler and language status below is a review of committed source
`96d3c98` on 5 November 2026. The bounded Copy-wire/stateful reload additions
are described separately from a 13 November working-tree verification record;
that later snapshot does not refresh acceptance evidence for unrelated features.
Rill remains an experimental sequential subset implemented in Rust with C11
output. No PLAN phase is marked complete here. [PLAN.md section 19](../PLAN.md#19-dependency-ordered-delivery-plan)
defines completion; the [active roadmap](CONTINUATION_PLAN.md) orders work.
The [feature matrix](language-features.md) describes user-visible capabilities.

## Phase gates

| Phase | Current state | Evidence still needed to close the gate |
| --- | --- | --- |
| P0: contract and baseline | Baseline, provisional decisions and normative examples recorded | Review remaining contract decisions; no completed soundness proof is claimed |
| P1: typed sequential kernel | Substantial bounded subset; typed execution plus checked Copy fallback | Complete semantic representation for the accepted subset, remaining inference/layout/source-mapping and stack-safety work, reproducible aggregate acceptance |
| P2: owned places | Call-scoped loans, restricted boxes/bytes and owner enums, explicit moves and cleanup | Owned records, broader initialization/move joins, control-flow loan analysis and general ownership verification |
| P3: effects and owned library | Fallible system allocation and fixed-buffer utilities | Closed effects/drop summaries, allocator provenance, growable containers and borrowed slices |
| P4: constraints and modules | Local file modules, exports, bundled library, rank-one Copy generics/callables | Traits, open effects, affine closure call kinds, packages and canonical public interfaces |
| P5: C integration | Target-checked scalar/raw-pointer imports, scalar exports and bounded native generation API | Aggregate ABI adapters, callback contracts, foreign lifetime/effect/resource checking and broader target evidence |
| P6: useful native release | C11 applications, renderer examples, editor/project tooling and alpha SDK recipe | Required P3–P5 contracts, debugging/source maps, allocation reports and reproducible acceptance of the supported native library/application scope |
| P7: advanced static APIs | Bounded phantom-layout support only | Const shapes, units, GADTs, higher rank and staging contracts with rejection evidence |
| P8: scoped concurrency | Not implemented | Executor, task/borrow lifetime and cancellation contracts plus tests |
| P9: transactions and search | Not implemented | Participant/undo/replay/collector semantics and restoration tests |
| P10: optimized graphics targets | Explicit u8x16/opt-in NEON and external shader tools exercise parts | Cross-target conformance, GPU layout/hazard contracts and representative benchmark distributions |
| P11: stabilization | Versioned alpha packaging and installer prepared | Compatibility policy, library audit and reproducible supported-target releases |

A later-phase experiment does not close earlier dependencies. In particular,
alpha packaging is not P6/P11 acceptance and external shaders are not a Rill
shader-language implementation.

## Current compiler and ownership boundary

The checker supports rank-one inference with union-find equality and levels,
conservative local value restriction, nominal records/enums, exhaustive matches,
closed function signatures and deferred nominal projections. Kinds, rigid
escape checking and general annotated interfaces remain open.

Production interpretation and C emission prefer independently checked typed
lowering. Finite Copy layouts, concrete function instances, field/root loans,
local capture/shape elaboration and bounded logical-only/discarded results use
that path. Unsupported Copy representations may use the checked uniform
fallback. Owner programs cannot fall back. The verifier remains provisional,
not the complete ownership/effect proof promised by PLAN section 16.

Owners include hosted i32 boxes, fixed-length owned bytes and finite concrete
owner-bearing enums with consuming matches and scoped cleanup. The owner path
rejects recursion, quantified functions, borrowed owner enums, owned records
and foreign declarations, including incompatible unused declarations. These
restrictions limit composition of otherwise available library modules.

See [ownership](blockers/ownership.md), [owned allocation](blockers/owned-allocation.md),
[layout](blockers/c-adt-layout.md), [function resources](blockers/function-resources.md)
and [stack safety](blockers/stack-safety.md). Source, module, function-instance
and pure Copy-body origin APIs exist; native operations, owner cleanup and
generated-C byte mappings still need broader coverage.

## Deferred nominal projections

Closed result/parameter constraints are installed across a recursive group
before its bodies. Ordinary equalities within that group can resolve field
reads, assignment and loans. Unresolved nominal identities still reject;
there is no structural label search or general order-independence promise.
See [recursive signatures](recursive-signatures.md) and
[deferred projections](deferred-projections.md) for contracts and acceptance.

## Local compiler and editor integration

Braced syntax, Copy functions/closures, boolean negation, unit-valued omitted
`else`, source formatting, LSP and editor packages are implemented. Optional
source mapping remains narrower than editor functionality. See
[source tooling](source-format.md), [functions](functions.md) and
[editor support](editor-support.md).

## Project tooling, CLI tokens and reload hooks

Project commands support strict manifests, imported-source watching and explicit
version-2 native/shader recipes. Failed builds retain the running process;
successful watch builds restart it. `std.cli` parses supplied static-string
tokens; runtime argv and general I/O remain open. Interpreter `reload::Session`
supports closed integer hooks with host state between calls. See
[projects](projects.md) and [reload workflows](hot-reload.md).

## Searchable documentation command

The docs command builds offline HTML from embedded guides, project Markdown and
parsed API declarations. It does not execute project code or native tools.
Source-level signatures are not inferred stable interfaces. The prepared Pages
workflow and local generation are distinct from an actual remote deployment.
See [docs command](docs-command.md) and [maintenance](docs-maintenance.md).

## Experimental C export ABIs

`abi::emit_i32_library` emits a checked C11 library, header and versioned manifest
for its closed i32 subset. The separate `abi::copy::emit_copy_library` emits a
bounded version-1 byte-wire ABI for closed Copy schemas, including records and
enums; it does not expose native aggregate layouts. Both reject native
foreign values, owners and unsupported roots. See [C exports](c-exports.md)
for the separate contracts and [C interop](c-interop.md).

## Standard library foundations

The compiler bundles 19 modules, including Option/Result, checked and saturating
i32 helpers, static text, owned-buffer/binary/JSON utilities, allocation,
supplied CLI-token parsing, reload context and graphics bindings. Generic Copy
helpers remain separate from owner-compatible monomorphic utilities. There are
no general vectors, owned strings, slices or OS services. See the
[catalogue](../stdlib/README.md) and [library guide](standard-library.md).

## Incremental builds, native generations and migration help

Project build/run/watch reuse checked generated C for exact unchanged eligible
pure graphs on Unix. Raw commands opt in. Changed graphs recheck; C compilation
and linking always run. Per-module checks, native objects and shader compilation
are not persistently cached. [Build performance](build-performance.md) owns the
current measurements and future performance acceptance criteria.

The scalar native generation API replaces closed i32 libraries between
exclusive synchronous host calls on macOS/Linux and retains the old generation
on failure. `CopySession` supports the separate bounded Copy-wire ABI.
`live_reload::LiveSession` adds validated Copy state schemas, explicit host
migration and selective root/helper-cohort replacement; checkpoint restore and
publication reject without replacing active state on failure. These remain
experimental host APIs, not process restart. Owned-state, callback, foreign
resource and GPU resource migration remain unsupported. See
[native generations](native-hot-reload.md) and the
[stateful reload example](../examples/reload-renderer/README.md).

Migration guides, rejection hints and spelling suggestions describe alternatives
without accepting Rust syntax. `str == str` and `!=` compare exact bytes,
including embedded NUL. Aggregate equality and abrupt exits remain unsupported.
See [search and equality](search-and-equality.md) and the
[language proposals](proposals/next-language-increments.md).

## Graphics tooling and alpha distribution setup

Explicit binary32 operations run through both evaluators and C backends. Float
literals and overloaded operators remain open. The SDL3 engine uses Rill
scene/camera/math logic, external Slang shaders and a C11 adapter for layouts,
uploads and cleanup. The Raylib bridge supplies a separate OpenGL path. These
bindings do not prove foreign or GPU resource lifetimes.

Version-2 recipes build native/shader inputs and the alpha SDK recipe bundles
SDL3, Raylib, Slang, validation tools and bridges. Recent local execution and
relocation evidence covers macOS arm64. The prepared Intel macOS/Linux package
matrix does not establish runtime acceptance there; Windows packages are not
configured. See the [graphics audit](graphics-feature-audit.md),
[shader guide](shaders.md) and [distribution guide](distribution.md).

## Evidence and continuing verification

The 5 November baseline claims above refer to their named committed source;
they are not a fresh full-suite run of later changes. The 13 November entry
below covers only the bounded Copy-wire/stateful reload and renderer-test slice
in its recorded working-tree snapshot. Do not sum overlapping totals or treat a
passing test return containing a skip as capability evidence.

| Evidence | Scope |
| --- | --- |
| [Baseline](baseline.md) and [normative examples](normative-examples.md) | Initial contract and fixtures |
| [Deferred projections](verification/2026-11-04-deferred-projections.md) | Sequential solver/layout integration; links to earlier bounded acceptance |
| [Compiler/editor integration](verification/2026-11-04-ernie-integration.md) | Combined cloud/local compiler, editor and native checks |
| [Tooling and migration](verification/2026-11-05-tooling.md) | Cache, scalar generations, migration, equality; full-run failures and focused repairs stated separately |
| [Graphics and distribution](verification/2026-11-05-graphics-distribution.md) | Full sweep followed by focused revisions, required local Metal execution, SDK relocation, Raylib, installer and docs checks |
| [Documentation normalization](verification/2026-11-05-project-direction.md) | Source review, planning changes and documentation-only validation |
| [Stateful Copy reload](verification/2026-11-13-stateful-copy-reload.md) | Copy-wire ABI, selective stateful reload and renderer protocol tests on the recorded working-tree snapshot; GPU outcome is reported separately |

Older [verification records](verification/) retain detailed typed-layout,
source-origin, ownership, importer and signature evidence. Feature defaults have
changed: current default builds include `c-import`; historical “default” and
“importer-enabled” counts must be read with their original configuration.

The repository currently runs ordinary documentation PR checks and compiler
checks in the tag/manual release workflow. A general compiler PR gate is future
work in the active roadmap. Use [verification commands](../AGENTS.md#verification) locally,
record optional dependencies/hardware separately, and run full acceptance
before claiming a phase is complete.
