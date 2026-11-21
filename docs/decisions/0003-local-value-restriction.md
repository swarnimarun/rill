# Conservative local value restriction

Status: provisional, 2026-11-03. This implements the first conservative rule in
`PLAN.md` section 5.4 for the experimental subset. It intentionally restricts
previously inferred local schemes; it is not a repair for a demonstrated
mutable-polymorphism soundness defect and does not complete C03 or P1.

## Eligible immutable initializers

An immutable local may quantify variables only when its initializer is:

- An integer, boolean, unit or ordinary static-string literal.
- An immutable local value reference, excluding direct `read`/`inout` parameters
  and roots with active call loans. Foreign constants have fixed declared types
  and can also be referenced as values.
- A validated nullary constructor, written as a name or zero-argument call.
- A validated positional constructor applied only to eligible values.
- A nominal record literal whose fields are all eligible values.

Constructors come from checked declarations, including resolved module
identities, never capitalization. A bare name resolves to a local before a
constructor. Constructor calls retain the existing separate declaration
namespace. Record identity comes from checked construction metadata. No
expression is reordered or reevaluated by classification.

No quantified variable may be free in the current local/function environment,
which includes mutable storage and borrowed parameters. Existing raw-pointer
obligations remain monomorphic. The initializer's type must have no known owner
dependency: hosted owners, owners in inactive variants, and even owner-valued
phantom type arguments prevent local generalization. The check visits validated
declaration syntax without expanding substituted generic field trees. It does
not infer effects or closure captures. First-class closures are not supported;
future closure support must establish its capture/ownership conditions first.

Other source forms do not qualify: user, foreign or builtin calls (including
allocation), field reads, binary operations, moves, assignments, loops,
conditionals, matches and nested-let/sequencing expressions. A constant
condition or a function whose body returns `None` does not change that rule.
Mutable locals never generalize.

## Weak variables, snapshots and function boundaries

Before applying the restriction, inference independently compares level-based
candidates with the environment free-variable rule. A restricted initializer
lowers every result free variable to the surrounding let level and stores an
empty quantified set. Later aliases cannot generalize those existing variables.
Wrapping an alias in a constructor does not remove its environment dependency.

A value parameter remains monomorphic inside its function. An independent
constructor parameter may still generalize: in `Pair(x, None)`, the variable in
`x` stays tied to the parameter while the unrelated variable in `None` may
quantify. Direct constructor reads of a mutable or borrowed root are excluded
as a whole, including otherwise independent phantom variables.

An immutable snapshot alias is different from a direct storage read. In
`let alias = read_root; let marker = Mark(alias)`, the alias retains the root's
monotype, but an independent fresh phantom parameter introduced by `Mark` may
generalize. The rule does not preserve historical read/mutable provenance after
copying a value. Neither the root nor its existing type variables can acquire
incompatible instantiations through that snapshot. Loan checks distinguish
roots by local identity, so a newly shadowing immutable local is not the root
borrowed by an earlier argument.

Record projection instantiates an immutable generalized record scheme before
selecting a field. A monomorphic projection result must not lower or bind the
original record's quantified variable. Mutable and borrowed roots have empty
schemes and retain the same root type. That checker correction alone did not
supply concrete typed layouts; the later [local-value shape elaboration](0004-local-value-shapes.md)
adds bounded typed execution without changing this source rule.

Top-level functions and recursive components retain their existing rank-one
generalization after monomorphic body/group inference. A function can return a
locally monomorphic value and still have a generic result across independent
calls. Polymorphic recursion and generic owner programs remain unsupported.

## Migration

Given these declarations:

```rill
type Option<a> = None | Some(a)
fn integer(x: Option<i32>) { 40 }
fn boolean(x: Option<bool>) { true }
fn absent() { None }
```

This remains accepted by inference:

```rill
fn main() {
    let empty = None;
    let alias = empty;
    if boolean(alias) { integer(alias) + 2 } else { 0 }
}
```

A call-produced local instead has one type:

```rill
fn main() {
    let empty = absent();
    if boolean(empty) { integer(empty) + 2 } else { 0 }
}
```

The latter reports `E-TYPE` at the incompatible use. A subsequent alias does not
change the restriction. Conditional/match-produced `None` values behave likewise.
Use independent calls or bindings when independent instantiations are intended:

```rill
fn main() {
    let integers = absent();
    let booleans = absent();
    if boolean(booleans) { integer(integers) + 2 } else { 0 }
}
```

Use a direct `None` value instead when no computation is intended. Duplicating a
call can duplicate effects, allocation or traps; this is a deliberate source
change, never an automatic compiler rewrite. Ordinary one-type uses of call,
branch and match results remain accepted.

## Remaining boundary

Eligible immutable nonexpansive Copy locals now have bounded
[capture-and-shape elaboration](0004-local-value-shapes.md): concrete leaves are
snapshotted once and every whole-value or selected-field use has its own exact
concrete representation. Unused shapes need no invented phantom type. This does
not broaden the value restriction. Unconstrained uses and weak monotypes can
still lack a typed/C layout. Concrete restricted locals continue to use one type.
Kinds, rigid escape checks, closure/effect conditions, complete MIR coverage and
full P1 acceptance remain open. See the original
[value-restriction verification record](../verification/2026-11-03-value-restriction.md)
and the [new lowering evidence](../verification/2026-11-03-typed-local-values.md).
