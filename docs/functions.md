# Functions and application

These forms are implemented in the current experimental compiler. Function
types use the existing rank-one inference engine, including its value
restriction. Parameters, results, captures and callback signatures are inferred.

## Function values and closures

```rill
fn identity(value) { value }
fn apply(f, value) { f(value) }
fn make_adder(amount) { fn(value) { amount + value } }

fn main() {
    let id = identity;
    let add_twenty = make_adder(20);
    if id(true) { apply(add_twenty, 22) } else { 0 }
}
```

This returns `42`. Named Rill functions are values. `fn(value) { ... }` is an
anonymous function, and `fn() { ... }` has zero parameters. A closure can escape
the function that creates it. Its immutable Copy captures are stored inline by
value, retaining their original lexical bindings even after later shadowing.
Construction does not implicitly allocate a heap box or add reference counting.

The compiler keeps each concrete environment's identity in the callable type.
Generic callbacks specialize for that environment. It then converts checked
closures into ordinary capture records and direct calls, rechecks the resulting
program and uses the existing typed/C machinery. The source reference evaluator
executes closures independently of that conversion.

## Currying and application

```rill
fn add(x, y) { x + y }
fn main() {
    let add_twenty = add(20);
    add_twenty(22)
}
```

All of these applications return `42`:

```rill
add(20, 22)
add(20)(22)
add 20 22
```

Supplying fewer arguments creates a closure containing the supplied values.
The callee and supplied arguments execute once, when that partial application
is constructed. Saturated calls retain the function's natural arity and invoke
one body. When a function returns another function, its body executes before
arguments for the returned function are evaluated. An unused function binding
still evaluates its initializer and its effects.

Parentheses invoke arbitrary function expressions, including lambda results
and record fields. Whitespace application binds more tightly than binary
operators. Within its argument list, `f x(y)` means `f(x(y))`, while `f x (y)`
means `f(x, y)`. Standalone legacy `f (x)` remains a call. Use `f(-1)` for a
negative argument; `f - 1` remains subtraction.

Zero-argument functions require `()`. Reading their name obtains the function
value without invoking it. Lambda values and named function aliases may
generalize under the value restriction; call/conditional results remain one
monotype. No arbitrary default type is chosen merely to lay out an unused
callable whose environment is already concrete.

## Application and composition operators

| Form | Meaning |
|---|---|
| `value \|> f` | Apply `f` to `value` |
| `f <\| value` | Apply `f` to `value` |
| `f >> g` | Construct a function that invokes `f`, then `g` |
| `g << f` | Construct a function that invokes `f`, then `g` |

Both operands evaluate in source order. Composition evaluates its two function
operands once during construction, then calls them when invoked.

```rill
fn increment(x) { x + 1 }
fn double(x) { x * 2 }
fn main() { 20 |> increment >> double }
```

This returns `42`. From lowest to highest precedence: pipelines, `||`, `&&`,
composition, equality, comparisons, addition/subtraction, multiplication/division
and application. `<|` associates to the right; the other new operators associate
to the left. Nested generic type delimiters such as `Outer<Inner<i32>>` remain
ordinary type syntax.

## Current boundaries

- Captures must be immutable Copy values. Mutable, borrowed and owned captures
  produce `E-FUNCTION-CAPTURE`. Capturing a polymorphic local requires a
  monomorphic binding; no hidden duplication or type erasure is introduced.
- First-class functions currently use value parameters. Direct `read`/`inout`
  calls retain their existing behavior. Borrow-mode function values and lambdas
  report `E-FUNCTION-MODE`.
- Builtins and foreign functions require direct calls. They are not guessed
  into a generic callable ABI. Ordinary Rill wrappers may call them directly.
- Different concrete closure environments do not unify just because their
  argument and result types match. A conditional joining those representations
  reports a type error; implicit dynamic erasure is not implemented.
- Functional programs containing checked Rill owners report
  `E-FUNCTION-OWNED`. They never fall back to the copying reference evaluator.
  See the [resource-capture blocker](blockers/function-resources.md).
- `main` cannot return a function or an aggregate containing one. This avoids
  exposing generated capture records as source-level printed values.

- Callable conversion currently requires concrete stored capture types and
  function instance signatures. A captured unresolved phantom argument or a
  nonreturning callable result may report `E-FUNCTION-LAYOUT`. Ordinary programs
  without callable syntax retain the upstream checked phantom, discarded and
  nonreturning paths; no default logical type is chosen.
- The uniform source-C oracle rejects unconverted callables. The independent
  source evaluator executes them directly; default C emission uses checked conversion.
- Optional typed function/body origin indexes reject callable programs until
  conversion has an explicit source mapping. Source AST/module origins remain
  available and include lambda parameters, bodies and application operands.

These limits preserve the current ownership and representation contracts.
They do not complete the broader closure/effect gates in PLAN.md section 19.
See the runnable [example](../examples/functions/README.md) and the
[verification record](verification/2026-11-04-ernie-functions.md).
