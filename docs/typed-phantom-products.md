# Finite Copy products with opaque logical arguments

## Status and scope

K2 extends the [K1 nullary-phantom contract](typed-nullary-phantoms.md) from
tag-only roots to complete finite Copy products. This bounded increment passed
focused, independent and full aggregate acceptance: 762 default and 810
importer-enabled tests, both strict Clippy and Windows compile modes, plus
independent GCC/Clang/native checks. See [exact sources, checks and retained failures](verification/2026-11-03-phantom-products.md).
No P1, P2 or P5 phase gate is closed here.

There is no new syntax, inference rule, generalization permission, owner
feature, foreign contract or discarded-result optimization. Ordinary typed
lowering must prove the physical representation of every field of every
variant of a complete nominal instance. Logical arguments remain exact even
when they require no stored bytes. An unresolved argument never becomes an
arbitrary `unit`, `i32`, pointer or other witness.

## Candidate admission boundary

These source-accepted programs have finite physical layouts:

```rill
type Mark<a> = Mark(i32)
fn main() { match Mark(42) { Mark(n) => n } }
```

The expected result is `42`. Returning `Mark(42)` must instead preserve the
constructor value and its ordinary printed form `Mark(42)`.

```rill
type Stamp<a> = { n: i32 }
fn make() { record Stamp { n = 42 } }
fn main() { let weak = make(); weak.n }
```

The call result remains one weak monotype. Its complete physical record is
finite even though its logical argument is unresolved. Empty phantom records,
mixed nullary/fixed-field enum variants, static strings, fixed byte buffers,
vectors and already supported finite Copy descendants use the same rule.

Admission is per instance, not a list of permitted declaration names:

```rill
type Flag<a> = Off | On
type Link<a> = Link(a, Flag<a>)
fn tag(x) { match Link(x, Off) { Link(_, f) => f } }
fn main() { tag(Off) }
```

Here `Link<Flag<opaque>>` stores two different logical identities:
`Flag<opaque>` and `Flag<Flag<opaque>>`. Both have finite tag layouts. Equal
storage does not make those identities interchangeable.

```rill
type Flag<a> = Off | On
type Option<a> = None | Some(a)
fn choose(yes) { if yes { None } else { Some(On) } }
fn main() {
    match choose(true) { None => 42 | Some(On) => 42 | Some(Off) => 0 }
}
```

`Option<Flag<opaque>>` is in scope on **both** None and Some paths: every
variant has a proved finite Copy representation. `Option<opaque>` remains
unsupported because Some stores an actual unknown payload, even if execution
selects None. A field such as `Flag<Option<opaque>>` is different: Option's
payload occurs only in a logical argument of a tag-only value, not in storage.

Direct opaque fields and transitive wrappers that eventually store them stay
unsupported. The same static check applies to unused evaluated initializers
and dead expression branches. Bare unresolved results of nonreturning calls
remain unsupported. K2 does not infer termination or erase evaluated results.

## Independent representation proof

Two analyses have deliberately different jobs:

- Physical parameter demand records which declaration parameters' layouts
  are required by **all** declared fields and variants. A demanded argument
  can itself be a proved finite nominal type with opaque logical arguments.
  A directly demanded opaque leaf fails. Fixed owner-bearing physical fields
  fail the new Copy admission rule.
- Logical native/owner dependencies traverse every argument and full field
  schema, including inactive variants and physically erased arguments.
  A physical-demand mask cannot erase these dependencies.

An open source nominal entry keeps its canonical resolved definition and
complete logical argument tree in symbolic metadata. Each opaque identity
retains its compilation brand and binder scope. Closed source entries keep
their existing concrete argument representation. Compiler-generated capture
products retain their separate origin and exact field/argument identity rule.

Independent verification must reconstruct every source record/enum kind,
record field name/order, constructor identity/order and substituted field
type. Physical nominal children must precede their parents in the registry.
No physical unknown, fabricated primitive, omitted inactive variant, forward
reference, owner field or unproved open child is allowed.

The proof continues through closed physical descendants. Matching the name
`Fixed` is insufficient if its source schema is `Fixed(i32)` but a malformed
registry supplies `Fixed(bool)`. This complete descendant check is essential
even when the closed child's logical identity appears unchanged. Builder
caches and demand summaries are not independent verifier authority.

Source correspondence is still separately checked against the original checked
program. A consistently rewritten schema and all its uses can describe a
different structurally valid program. Source-free verification alone does not
prove that such a rewrite preserves the original source.

Full syntactic declaration-cycle rejection remains unchanged, including a
recursive occurrence beneath an otherwise erased phantom argument. Recursive
functions may still deduplicate exact full signatures; the existing 32-active-
call runtime guard, including main, is unchanged.

## Effects, places and source rules

Construction evaluates operands once in written order. Records must not reorder
effects into declaration-field order. Unused calls and selected trapping
operands still execute. Unselected branches do not execute, but still receive
ordinary static representation checking. Shape captures remain immutable
snapshots made at the original binding.

Existing Copy root/field operations consume the proved exact layouts. Shared
and exclusive loans retain the original slot and field path; sibling field
loans remain disjoint. A field assignment evaluates its right-hand side before
updating the original root, preserving a whole-record replacement performed
by that right-hand side. Computed read arguments remain once-evaluated
temporaries. These facts provide no foreign pointee independence or `restrict`
permission.

Calls, conditional/match results, mutable bindings, aliases of weak values and
borrowed-dependent bindings gain no polymorphism. Mixed nominal types,
incomplete matches, aggregate equality, overlapping/prefix loans and
read-to-inout forwarding remain ordinary source errors. Generic owner
programs, owner-bearing open products, owner/native mixing and existing
unsupported owned records retain their existing rejection boundaries. Closed
owner controls keep their prior behavior and separate cleanup/allocation tests.

## Native and optional-origin contracts

Existing unsigned, float and exact raw-pointer values can be fixed physical
Copy fields. Native dependencies also survive fields whose raw/owner types
occur only logically. Preserve the complete native manifest and all header
assertions, including unused signatures. Pointer spelling, pointee identity
and constness remain exact. A read pointer-slot loan const-qualifies the slot,
not a different pointee type. No new C ABI or foreign lifetime rule is added.

The evaluator still cannot invoke native code. Reached calls evaluate their
ordinary arguments before `E-INTERP-FFI`; raw operations and constant reads
retain their existing `E-FFI` precedence. Unused declarations and unselected
native branches do not prevent pure evaluation. Native C runs must be checked
against the actual fixture header/functions, not described as interpreter FFI
parity.

Both optional origin APIs stay closed. Ordinary source analysis, lowering and
verification run first. Successful ordinary lowering containing any opaque
metadata then produces the existing explicit mapping rejection:

- `E-TYPED-UNSUPPORTED: function origin index does not map opaque logical arguments`
- `E-TYPED-UNSUPPORTED: Copy node origin index does not map opaque logical arguments`

This includes body-only opaque products with a concrete integer result. No
partial successful index or invented concrete identity is permitted. Ordinary
semantic errors retain precedence; closed pure mappings remain supported and
closed native/owner exclusions remain intact.

## Proposed K2 resource policy

The design calls for cumulative 100,000-work-unit bounds for each new product
phase: lazy schema-catalog preparation, admission/substitution, registry
accumulation and independent representation verification.
They activate for non-nullary symbolic products, without silently redefining
closed-program or K1 counters. Exact implementation accounting and boundary
tests must be reconciled and recorded during K2 acceptance.

Charge new work before traversal, allocation or growth using checked arithmetic:
each retained/visited node, edge, variant, field and parameter, plus each copied
or hashed identifier byte. Known-length batches must be reserved first.
Repeated key requests pay for their actual comparisons. Reservations may be
conservative: registry keys reserve six complete scans/copies before lookup and
possible insertion; retained declaration indexes reserve both copied and hashed
name bytes; canonical metadata reserves separate validation, copying and source
comparison passes. Closed raw-pointer atoms copied after K2 activation also
reserve their pointee spelling. Shared demand/binder unions and replicated constructor tables are charged per operation/instance;
shared children do not make all transitive scans free. Substitution must bound
expanded size and height before duplicating argument subtrees. Verification
should compare borrowed substitution views rather than eagerly expand them.

Existing 1,024 function-instance/layout limits, 128-level nesting, per-variant
4,096-field limit, conservative 1 MiB inline-layout bound and local-shape/capture
limits still apply. New work exhaustion uses `E-TYPED-LOGICAL-COMPLEXITY`, not
unchecked fallback. These units bound this representation work, not all compiler
memory, C output or arbitrary host-stack usage. Failed construction must not
publish a partial registry/certificate.

## Acceptance inventory and remaining verification

[`tests/typed_phantom_products.rs`](../tests/typed_phantom_products.rs) contains
acceptance assertions, including the exact Mark, Stamp and Link fixtures
moved from K1's unsupported list. It compares direct ordinary typed lowering,
independent verification, typed/default/reference evaluation, byte-identical
default/typed C output and deterministic repeated lowering. Default C must
contain neither `rl_value` nor `RL_CAP`. The uniform-C oracle retains a narrower
open-layout boundary and is not substituted for direct typed admission.

The suite covers returns, weak results, nested logical arguments, both Option
variants, forward declarations, repeated/permuted parameters, effect order,
snapshots, loans, recursion, value restrictions, modules, origin precedence,
native fixed/logical dependencies and unused incompatible header assertions.
Its C helper invokes GCC and Clang explicitly at `-O0` and `-O2` with
`-std=c11 -pedantic-errors` when available. Missing compilers are reported as
missing cross-compiler evidence, without substituting cc for either compiler.
Executed focused, independent and aggregate results are recorded in the linked verification report.

Each later checkpoint must retain exact revisions, commands, compilers/flags,
results and skips for:

1. Focused parser/checker/lowering and internal schema/layout/binder tests,
   including malformed artifacts rejected by verify, evaluate and emit_c.
2. Exact and one-short budget reservations, all-variant reconstruction,
   closed-descendant forgeries, cross-compilation identity swaps and unchanged
   native/owner dependency enforcement.
3. Pure differential and native header/trace runs with explicit GCC and Clang
   at both optimization levels, plus accepted closed owner controls.
4. Independent review, final formatting, locked default/full importer-enabled
   tests and strict all-target Clippy in each supported configuration, retaining
   platform/native gates. A skip is not acceptance.

This work does not complete genuinely unknown payload/no-return lowering,
generalized owner products, full origin mapping, or the plan's phase gates.
