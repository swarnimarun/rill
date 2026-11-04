# Rill Native: language, type-system, and compiler blueprint

**Status:** Proposed design, not an implemented language or a completed soundness proof.  
**Design date:** 2 November 2026.  
**Baseline:** The user-provided *Rill rebuild plan* (`markdown.md`).  
**Scope:** A native-first ML-family language for graphics, games, tools, and systems software. All Rill code below is proposed surface syntax or explicitly identified core notation, not code accepted by the current compiler.

## Implementation amendments, 3–4 November 2026

The implementation uses Rust and targets C11 first. The current supported
subset and open phase gates are recorded in [phase status](docs/phase-status.md).
The following user-requested choices supersede earlier surface-syntax proposals
where they differ:

- Functions and control flow use explicit `{}` blocks. Statements end in `;`,
  and a final expression supplies the block's value. Indentation is cosmetic.
  The experimental `let ... in` syntax remains accepted for existing examples.
- Native dependencies can be declared in source with `@include("header.h");`,
  `@link("library");`, and `@pkg_config("package");`. The CLI uses the selected
  C compiler and structured arguments for header checks, building and running.
- `@include` automatically imports referenced supported C functions and
  constants before module resolution, using the configured C frontend and
  target compiler. Header modules automatically export generated declarations.
  Explicit `extern` declarations remain accepted. Aggregate ABI shims,
  callbacks and checked foreign ownership/effect contracts remain later P5
  work. A header signature check does not establish those contracts.
- Native `build` and `run` use `-O3` by default, with explicit optimization
  overrides. `run --interpret` selects the checked evaluator. Optimizations
  retain arithmetic traps, allocation and cleanup behavior.

See the [current syntax and commands](README.md) and the
[direct SDL3 example](examples/sdl3-direct/README.md). These amendments do not
relax the acceptance gates in section 19 or claim the language is stable.

## Executive decision

Build an **affine, native ML with place-based borrowing, explicit storage, qualified type-and-effect inference, and a small trusted compiler core**. Preserve ML expression syntax, pattern matching, algebraic data types, inference, and composition. Make allocation, duplication, dynamic dispatch, asynchronous execution, and foreign-code obligations visible. Add search and transactional mutation as checked extensions rather than prerequisites for ordinary native programming.

The target is not “all features of F#, OCaml, Rust, Haxe, and Zig.” The target is a coherent language with measurable advantages: pleasant functional code around predictable, allocation-controlled native kernels; reliable C integration; excellent iteration and diagnostics; and enough type expressiveness to encode domain invariants without making every program a proof.

The uploaded plan already calls for checked places, explicit cleanup, a reference interpreter, verified MIR, and differential backends. Keep those commitments. It also identifies ownership/rollback/concurrency interactions as risks. This blueprint develops their contracts and changes the delivery priority: a useful native library and renderer integration should not depend on complete backtracking, transactions, GPU code generation, or self-hosting. [R0: §§4–7, 9–11, 13]

### Evidence and design boundary

Statements marked **Recommendation** or expressed as “Rill should” are proposals. Published research supplies precedents, not a proof that this combined design is sound. Affe demonstrates an ML-style affine/linear system with borrowing and principal inference; Oxide supplies a source-oriented account of borrow checking and provenance; Koka demonstrates effect-row inference; bidirectional typing provides a tractable route to annotated higher-rank polymorphism. The combined extensions below require their own metatheory. [S1–S4]

## 1. Product contract and deliberate exclusions

### 1.1 Required properties

| Property | Contract to implement | Evidence of completion |
|---|---|---|
| Native control | Inline aggregates, explicit allocation, raw-pointer boundary, target layouts | Object/layout inspection and ABI fixtures |
| Safe ordinary code | No use-after-free, invalid initialized reads, or unchecked overlapping mutable access in the safe subset | Formal core plus compile-fail and runtime tests |
| ML ergonomics | Local inference, ADTs, patterns, expressions, composition | A representative library with few local annotations |
| Predictable hot paths | No implicit GC, reference counting, boxing, or deep cloning | Allocation/dispatch reports and generated-code checks |
| Resource accountability | Ownership, cleanup effects, allocator provenance | Cleanup traces and exhaustion tests |
| C integration | Real preprocessing and target-aware ABI handling | Round-trip calls against a C compiler |
| Compiler flexibility | Shared checked IR, bounded compile-time evaluation, checked macro expansion | Same conformance corpus across consumers |
| Tool quality | Formatter, diagnostics, incremental checks, debugger mappings | Editor and malformed-input integration tests |

“No implicit allocation” means source semantics do not require allocation for a particular construct. It is not permission for an optimizer to allocate secretly. A small closure has a concrete environment value; boxing it is a different operation. A copy may still be expensive even when it is allocation-free.

### 1.2 Exclusions from the first production subset

Do not initially implement unrestricted dependent types, impredicative inference, arbitrary type-level programs, overlapping trait instances, subtype inheritance, transparent copy-on-write, mandatory tracing GC, arbitrary multi-shot continuations, or a full C++ ABI frontend. Do not make JavaScript semantics the least-common-denominator native memory model.

These are scope decisions, not claims that the features are intrinsically bad. Each increases the number of interacting rules and proof obligations. Advanced features should earn their place through real library use cases.

### 1.3 Profiles, not incompatible dialects

Use one semantic core with explicit target/profile requirements:

- **Freestanding:** no implicitly linked allocator, libc, operating system, executor, or unwinder. User supplies the entry point and required capabilities.
- **Hosted native:** explicit standard-library services and an optional default allocator provided by the entry-point environment.
- **Wasm:** the same supported core with an explicit host interface and linear-memory/ABI adaptation.
- **Shader:** a separately validated subset with address spaces, uniformity restrictions, bounded storage, and target layout rules.

A profile removes capabilities or rejects unsupported constructs; it must not quietly reinterpret ownership, overflow, strings, or errors. Start with one host ABI. Record additional target support only when the conformance suite actually runs there.

## 2. Changes relative to the uploaded Rill plan

| Baseline idea | Decision | Reason |
|---|---|---|
| Verified place/control-flow MIR | Retain and strengthen | The semantic center must precede backend choices |
| ML-shaped syntax and inference | Retain | This is the main ergonomic differentiator |
| Exclusive `inout` and projections | Retain | Express direct mutation without unrestricted aliases |
| Default read parameter convention | Change to by-value unless annotated | Generic identity and ordinary ML functions then have predictable ownership; `read`/`inout` calls still borrow ergonomically |
| No public lifetime parameters as a goal | Relax for stored/returned views | Most regions can be inferred, but API relationships must remain expressible |
| Failure, choice, rollback | Keep distinct | Failure is not evidence that external state is reversible |
| `option` with implicit transactional expectations | Specify a migration explicitly | Recommended new `option` handles failure; `attempt` or `transaction` requests rollback; do not silently change old programs |
| Computation builders | Retain as checked library expansion | No privileged safety bypass based on builder spelling |
| MLIR as intended native path | Retain as an optional later lowering route | A thin direct LLVM path is a reasonable first native backend; choose one native route initially |
| Broad concurrency forms | Define through task contracts first | Keyword count is not a concurrency model |
| Self-hosting | Defer | It is not a prerequisite for a useful, trustworthy toolchain |

The parameter-default and handler changes are **intentional proposed compatibility breaks**. They need ADRs, migration diagnostics, and corpus classification before adoption. Nothing in the uploaded document establishes them as approved. [R0: §§4, 10, 12]

## 3. Surface language and evaluation contract

Use strict, left-to-right evaluation. Bindings are immutable unless declared mutable. Mutation uses a distinct operator. ADT constructors, tuple construction, function arguments, and record fields are evaluated in source order. Patterns are exhaustive unless a construct explicitly returns failure. Guards do not make an otherwise incomplete match exhaustive.

```rill
type Vec2 = { x: f32; y: f32 }
type Particle = { position: Vec2; velocity: Vec2 }

let integrate (inout p: Particle) (read acceleration: Vec2) (dt: f32)
    : unit ! {write(p)} =
    p.velocity <- p.velocity + acceleration * dt
    p.position <- p.position + p.velocity * dt
```

Here `write(p)` abbreviates a write to the abstract region reachable through that parameter. The function does not promise strict floating-point reproducibility across targets; that is a separate profile and math-library contract. The example assumes the obvious arithmetic implementations for `Vec2` and a trivial `Copy` representation.

Saturated `f a b c` is one direct call when `f` has natural arity three. Partial application creates an explicitly typed closure environment and incurs construction effects now; effects of the body occur on invocation. Do not combine calls across user-visible effects merely because syntax appears curried. Returning a closure is different from returning a function result.

A recursive type must have finite layout. For example, `List<a> = Nil | Cons(a, Box<List<a>, A>)` includes an indirection and an allocator/ownership choice. An immutable linked list is not silently a GC-allocated or reference-counted list.

## 4. Type-system organization

Separate several related descriptions rather than encoding everything in one unstructured type object.

| Layer | Examples | Question answered |
|---|---|---|
| Value type | `i32`, `Option<a>`, `Vec<a,A>`, `Mat<f32,4,4>` | What value is this? |
| Parameter convention | value/sink, read, inout, set | How is a place accessed or transferred? |
| Borrow provenance | origins, regions, outlives constraints | Which storage makes the view valid? |
| Closure call capability | `Fn`, `FnMut`, `FnOnce` | How may the environment be invoked? |
| Effect row | `read(r)`, `write(r)`, `alloc(a)`, `io(c)` | Which observable actions may execute? |
| Predicates | `Copy<a>`, `Send<a>`, `Trait<a>`, `DropEff<a,e>` | Which generic obligations must hold? |
| Layout | size, alignment, fields, validity, ABI class | How is a concrete value represented? |

OxCaml is a useful precedent for distinguishing what values are from how they are used, and for making layout information explicit. Rill should not copy its entire mode lattice or assume those modes have the same semantics as Rill ownership. [S8, S9]

### 4.1 Kinds

The internal kind system distinguishes `Type`, `Region`, `Effect`, `Nat`, `Measure`, and declared constructor kinds such as `Type -> Type`. In the first implementation, all ordinary type constructors have fixed arity; constructor-kinded variables are rejected until the restricted higher-kinded extension is implemented.

Do not unify a region variable with a type variable, or a row tail with a user-defined effect name. Distinguish inference metavariables from rigid quantified variables and skolems. Every metavariable has a level, kind, origin, and optional universe index.

### 4.2 Core grammar

The following is internal notation, not a proposed parser grammar:

```text
T ::= a | primitive | N<T...> | T * T | Array<T,n>
    | Ref<r,Shared,T> | Ref<r,Exclusive,T> | Ptr<address_space,T>
    | Closure<call_kind,Env,(mode T)...,effects,T>
    | Dyn<r,trait,associated_types,method_effect_bounds>

Scheme ::= forall (type/effect/region/const variables). Predicates => T

Effect ::= read(r) | write(r) | alloc(a) | free(a)
         | io(c) | atomic(r) | block | suspend | spawn
         | foreign | decides | trap

Row ::= {} | {Effect... | effect_variable}
```

The advanced explicitly annotated core additionally admits predicative `forall` and existential packages. The first implicit generalization system is rank one. A closure's ownership and copyability are properties of its concrete environment, not just its arrow type.

### 4.3 Data type policy

Use nominal ADTs and records initially. Support opaque newtypes, tuples, fixed arrays, explicitly owned buffers, borrowed slices, and transparent wrappers. Nominal identity prevents equal-looking resource types from becoming interchangeable. Record labels may use contextual disambiguation, but unresolved ambiguity is an error rather than a global search heuristic.

Structural record rows are a later, separate extension; they must not share the solver representation or semantics of effect rows by accident. Do not start with OCaml polymorphic variants, structural records, nominal inheritance, and traits all competing to describe the same interface.

## 5. Inference algorithm and annotation boundaries

### 5.1 What gets inferred

Infer local monotypes, eligible let-polymorphism, literal types, callback effects, closure captures, and local borrow lifetimes. Require annotations for exported interfaces, polymorphic recursion, higher-rank arguments, GADT-sensitive recursive definitions, and ambiguous returned-view origins.

Use a Hindley–Milner-style constrained inference engine for the ordinary subset and bidirectional checking for annotated advanced forms. A sound and complete algorithm for a different calculus is not automatically a theorem about this combined system. The research basis is Affe for resource-aware inference and bidirectional higher-rank work for checking richer annotations. [S1, S4]

### 5.2 Solver architecture

Maintain distinct solvers for type equality, effects, trait goals, regions/loans, and restricted constants. Connect them through typed obligations and source locations. Avoid a single “unify everything” routine.

```text
infer_module(module):
    resolve names and declaration dependencies
    kind-check declarations and signatures
    for each strongly connected declaration group:
        install monomorphic placeholders or checked annotated schemes
        infer/check expressions, recording types, effects, and obligations
        solve equality constraints with levels and occurs checks
        resolve trait goals using declared generic assumptions
        elaborate parameter conventions, captures, and candidate places
        construct structured control flow and ownership obligations
        check initialization, loans, escape, and move paths
        elaborate cleanup; contribute destructor effects
        solve/propagate effect constraints through the declaration group
        reject ambiguous or escaping metavariables/skolems
        generalize only eligible bindings
        check exported interface bounds
    emit a typed, checked module interface
```

This is a dependency graph, not necessarily one literal pass. Closure capture analysis, drop elaboration, and effect summaries interact. Local `let` boundaries must establish the information needed for safe generalization before their schemes are used. The implementation can conservatively delay generalization when unresolved capture/effect facts remain.

### 5.3 Type unification

Use interned immutable type nodes and union-find metavariables. Variable binding performs kind checking, an occurs check, level lowering, and a skolem-escape check. Nominal types unify only when their identities and arities agree. Arrays also generate length-equality obligations. Function comparison includes conventions, result type, permitted call capability, and effect compatibility; an `inout` parameter is not interchangeable with a consuming parameter.

Normalize transparent type aliases with cycle detection. Keep opaque aliases nominal outside their defining module. Never solve a generic function by trying arbitrary concrete instantiations until one happens to compile.

### 5.4 Generalization and mutation

Begin with a conservative value restriction. Generalize eligible immutable nonexpansive bindings whose quantified variables are not free in the environment, live store, loans, or captured ownership dependencies. Do not generalize a mutable cell, mutable binding, stateful closure environment, or an allocation result merely because its apparent result type is polymorphic.

OCaml documents why an unconstrained polymorphic mutable cell is unsound. Koka demonstrates a different state/effect-based treatment, but borrowing that idea requires adopting and proving its relevant conditions, not simply deleting the restriction. [S5, S3]

A closure capturing an affine resource can remain monomorphic in the first release. Later permit carefully specified polymorphism that cannot reinterpret the same capture across incompatible instantiations. This conservative limit is preferable to an incorrect “all lambdas generalize” rule.

```rill
let id x = x                 // forall a. (value a) -> a
let duplicate x = (x, x)    // forall a. Copy<a> => (value a) -> (a, a)
let mutable slot = None      // one monotype; not forall a. Option<a>
```

A mutable reference cell is not an ordinary unrestricted record; its trusted constructor and operations participate in the state/aliasing model.

### 5.5 Recursion and overloading

Infer a recursive strongly connected group under monomorphic assumptions, then generalize the group. Polymorphic recursion requires an explicit checked scheme. Reject infinitely expanding type equations and polymorphic recursion that produces unbounded specialization unless an explicitly supported erased representation exists.

Integer literals carry representability constraints. Default unresolved ordinary integer literals to `i32` and floating literals to `f64` at documented boundaries; require explicit types in ABI declarations. Do not use an arbitrary trait implementation as a default. Require constraints such as `Add<a>` for generic operators. Trait coherence must make dictionary selection unique.

### 5.6 Principal-type claims

Aim to establish principal constrained schemes for the specified rank-one fragment. Do not advertise principal inference for the whole language once trait defaulting, higher-rank checking, GADTs, constants, and effect subsumption are combined. Document where annotations are required and show the inferred scheme in diagnostics.

## 6. Effect rows, capabilities, and cleanup

### 6.1 Semantic meaning

Effects are upper bounds on operations that may execute, including implicit destruction. They do not grant authority. A file write requires both an appropriate I/O capability and an effect containing that operation. An allocator similarly supplies authority/storage while `alloc(a)` records that allocation may occur.

A function with an empty visible effect row need not terminate. Effects and termination are separate analyses. Allocation failure is normally an explicit `Result`; allocation remains an effect even when failure is handled.

Maintain a second, conservative **operational resource summary** `k` alongside the semantic row `e`. It records classes such as heap allocation, arena allocation, heap free, blocking, task creation, and unknown foreign behavior. Calls, captures, and cleanup compose these summaries by union; generic callbacks quantify their summary when necessary. An internal function contract is therefore `(parameters) -> result ! e @ k`. Most source signatures can display a combined presentation. This summary is an upper bound on kinds of operations, not a count, space bound, or time bound.

### 6.2 Row theory

Recommendation: use finite idempotent sets of effect atoms, open row variables, and explicit inclusion/union/lacks constraints. Koka's published system uses duplicate labels; this proposal deliberately differs and must not claim to inherit that row calculus's proof. [S3]

Implement closed rows first. For open rows retain constraints such as:

```text
e_body >= e_function_expression union e_arguments union e_callee
forall e. (Callback<T,U,e>, T) -> U ! e
```

Keep a canonical representative for each effect atom after region unification. When two region variables merge, their duplicate `write` atoms must coalesce. A handler residual must be computed in the constraint theory rather than by deleting a printed string. Region-sensitive labels and unresolved tails require real constraints.

At a closed interface, verify inferred effects are included in the allowed row. An open callback effect cannot pass a `noalloc` gate unless its quantified bound excludes the prohibited allocation effects. Do not silently instantiate an unresolved effect to the empty set.

### 6.3 Higher-order propagation

```text
apply : forall a,b,e.
        (Fn(value a) -> b ! e, value a) -> b ! e
```

Evaluating `make_closure()` includes environment construction and any allocation or reference-counting operation performed there. Calling the resulting value includes its latent body effects. Repeated invocation has the same may-effect set, not a count of executions. If operation counts matter, use a separate cost/budget analysis.

### 6.4 Destruction is not free or necessarily pure

For each type, compute or require a summary `DropEff<T,e>`. Generic cleanup contributes this effect row. Destroying a buffer may free allocator storage and execute element destructors. Destroying a file wrapper may perform I/O. Destroying a job guard may block. None of those can be hidden behind a supposedly pure generic helper.

```text
discard : forall a,e. DropEff<a,e> => value a -> unit ! e
identity : forall a. value a -> a ! {}
```

Automatic destructors cannot ordinarily fail or suspend in the first release. Fallible flushing and asynchronous closing must be explicit operations returning a result or task. An infallible destructor may still contain a declared I/O, free, or block effect; realtime scopes reject the prohibited ones.

### 6.5 Effect masking and local state

A local region's `read(r)` and `write(r)` may be hidden only when neither the result nor any escaping closure, object, capability, or externally observable store retains access to that region. Allocation provenance and finalizers must also be checked. Local mutation becoming externally pure is a justified encapsulation rule, not a printer convenience.

Distinguish semantic encapsulation from cost guarantees. A function allocating in a hidden arena can still violate `noalloc`; do not erase its cost-relevant allocation information merely because the region is private. Private-region abstraction must preserve the corresponding operational resource summary. The proof obligation is joint: semantic events are covered by the effect row modulo the specified region abstraction, and resource events are covered by `k`.

### 6.6 Checked performance policies

`@noalloc` should mean no dynamic allocation anywhere in the checked transitive execution, including arenas, closure boxes, imported calls, and cleanup. A narrower `@no_heap_alloc` can allow a named bounded scratch allocator. `@realtime` is a documented policy excluding heap allocation/free, blocking, unbounded scheduler work, and other prohibited effects; it is not a worst-case execution-time theorem.

Opaque foreign calls fail such checks unless accompanied by a reviewed contract. A capability handler cannot discharge an allocation prohibition merely by supplying an allocator. Stack usage, recursion, and execution-time bounds need separate policies.

## 7. Ownership, places, and initialization

### 7.1 The ownership contract

Owned values are affine: ownership may be transferred at most once along an execution path. The compiler inserts destruction for live owned values at defined scope exits. `Copy` is a structural property permitting implicit bitwise duplication of values with no destructor and no exclusively owned resource. `Clone` is an explicit, potentially allocating or otherwise effectful operation. Neither immutability nor functional syntax implies copyability.

Default by-value parameters consume non-`Copy` values and copy `Copy` values. `sink` is an optional explicit spelling of consumption. `read` borrows for observation; `inout` borrows exclusively; `set` initializes an uninitialized output. Call sites can use ordinary arguments for `read`, but mutable and consuming access should have visible syntax or reliable editor annotations. Do not base source-level ownership on an optimizer's last-use decision.

| Convention | Entry | While active | Exit |
|---|---|---|---|
| value / sink | Initialized owned value | Callee owns the transferred value | Returned, transferred elsewhere, or destroyed |
| read | Initialized place | Shared observation; conflicting writes prohibited | Owner retains the value |
| inout | Initialized place | Exclusive access to overlapping storage | Place is initialized on every returning path |
| set | Uninitialized destination | Exclusive initialization authority | Initialized on success; cleaned and uninitialized on failure |

Initially, assignment to an initialized destination is a distinct replace operation: evaluate the replacement, destroy the old value with its effects, and initialize the destination. `set` does not silently mean both initialization and replacement.

### 7.2 Places and overlap

A place is a typed path rooted in owned storage or an established borrow:

```text
Place = Root Projection*
Projection = Field(id) | Index(operand) | Deref | Variant(id) | Slice(range)
```

Distinct owning roots, distinct nonoverlapping record fields, and proven disjoint slice partitions can be accessed exclusively at the same time. Distinct raw-pointer variables do not establish disjoint storage. Union fields overlap. Dynamic indices are potentially overlapping unless a trusted checked operation establishes otherwise.

```rill
swap (inout pair.left) (inout pair.right)   // accepted for disjoint fields
swap (inout values[i]) (inout values[j])   // rejected without a proof
let (left, right) = split_at_mut (inout values) midpoint
```

`split_at_mut` checks its boundary and returns loans derived from one exclusive parent loan. The parent is suspended while its children are live. The implementation belongs to the trusted library boundary and needs a verified contract; naming a function “split” does not establish disjointness.

Hylo's documented projections are a useful ergonomic precedent for exclusive access through a binding. Rill still needs its own specified overlap, lifetime, and cleanup rules. [S10]

### 7.3 Loan inference

Construct CFG program points before loan analysis. Each loan records its origin place, access mode, provenance, creation point, and required live points. Solve outlives/inclusion constraints across branches, loops, failure edges, and task boundaries. Use nonlexical loan endings based on liveness, but retain predictable lexical destruction unless an explicit consume/drop changes it.

A borrow cannot outlive the storage it depends on. A returned view may depend on caller-owned input, not a dead local. Most local annotations can be omitted; relationships crossing abstraction boundaries need an explicit or inferable contract.

```text
head : forall r,a. Slice<r,a> -> Option<Ref<r,Shared,a>>
choose : forall r,a. (bool, Ref<r,Shared,a>, Ref<r,Shared,a>)
         -> Ref<r,Shared,a>
```

For views with multiple independent origins, expose region parameters or an explicit `from` relation. Do not claim both unrestricted stored views and no lifetime relationships anywhere in the language. Owned buffers and generation-checked handles avoid many ordinary lifetime annotations; they do not eliminate the underlying reasoning.

Oxide's use of provenance in a substructural judgment is an appropriate reference for the formal treatment. Its result does not automatically cover Rill closures, transactions, or effects. [S2]

### 7.4 Definite initialization and partial moves

Track a tree of initialized subplaces. Moving a field does not move unrelated fields, but the whole aggregate is no longer fully initialized. Reinitializing one field cannot reinitialize its siblings. At a join, a place is definitely initialized only when it is initialized on every reachable incoming path. Maintain additional maybe-initialized information and drop flags where conditional cleanup needs it.

A later read of a possibly moved value is rejected. Pattern matching records which components are moved versus borrowed. For a type with a user destructor that depends on the whole value, initially prohibit arbitrary partial moves; require an explicit consuming decomposition operation with a valid destructor contract.

Moving a value out of an `inout` place temporarily creates a hole. Initially prohibit this except through checked replace/take operations that preserve the exit invariant. Do not allow `?`, cancellation, or a destructor to expose the hole.

### 7.5 Views and projections

A direct projection is a borrowed subplace. A computed projection is an acquire/yield/release protocol with possible temporary storage and writeback. The compiler represents all three phases explicitly. Initial release routines cannot fail or suspend. Their writes and cleanup effects are charged to the caller.

Release runs exactly once on normal completion and ordinary failure. Cancellation support is added only when those edges exist in the formal model. A view into a temporary cannot escape; its type includes its provenance even when the surface syntax hides that parameter.

### 7.6 Variance

Shared references may be covariant in the viewed type and borrow lifetime under the defined model. Exclusive references must be invariant in the stored type, although a borrow can be shortened through a valid reborrow. Owning containers are covariant only where their actual operations and representation justify it. Interior-mutable fields force appropriate invariance. Function arguments and results have the usual opposite variance, subject to conventions and effects.

Rust's reference gives a useful concrete comparison, particularly the invariance of the value type behind mutable references. Do not implement a blanket “all generic parameters covariant” rule. [S11]

### 7.7 Cleanup guarantees and limits

For normal returns, explicit error returns, and supported cooperative cancellation, every still-owned reachable resource is either transferred or destroyed once according to the language's scope rules. This is not a guarantee after process termination, abort, power loss, stack exhaustion, or a violated unsafe contract. The first profile uses aborting traps rather than stack unwinding; traps therefore do not promise resource cleanup.

Safety must not depend on arbitrary user code running after an abort. Device-resource retirement and asynchronous I/O must retain independent ownership until completion. Memory-safe leaks and protocol errors remain possible; a type-safety theorem does not prove that all business-level cleanup happened.

## 8. Closures, partial application, and dispatch

### 8.1 Closure construction and call capability

Each closure is elaborated to a nominal environment type plus a body. Capture modes are inferred from the body, with syntax available to override or clarify them. A read capture creates a shared loan; an inout capture creates an exclusive loan; a move capture transfers ownership; an explicit clone duplicates through `Clone`.

Classify invocation separately:

- `Fn` needs only shared access to its environment.
- `FnMut` needs exclusive access to update the environment.
- `FnOnce` may consume captured resources and is callable through a consuming operation.

Owning a noncopyable buffer does **not** automatically make a closure callable only once. A closure that only reads its owned buffer may be invoked repeatedly while remaining noncopyable. Conversely, a closure containing a mutable borrow cannot be duplicated merely because its body could execute repeatedly.

### 8.2 Representation policy

| Source category | Representation | Allocation requirement |
|---|---|---|
| Named function | Direct symbol or thin function pointer | None |
| Concrete closure | Environment fields stored inline | None merely for construction |
| Generic callback | Specialized concrete closure type | None required |
| Borrowed dynamic callback | Code/vtable plus borrowed environment pointer | None required; lifetime constrained |
| Owning dynamic callback | Explicit box/region-owned erased environment | Explicit storage operation |

An escaping closure is legal without heap allocation when it is returned as a concrete owned value and its captures are valid. Escape analysis may optimize storage; it must not be the only reason a program is safe. Recursive closures require an explicitly supported indirection or restricted recursive representation.

Partial application follows the same rules. Applying a function to a moved resource creates a closure owning that resource. Supplying an `inout` argument can retain a loan until the partial application is consumed or dropped; the compiler must diagnose that lifetime rather than silently shorten it.

### 8.3 Dynamic objects

Use explicit `dyn Trait` erasure. Fix associated types and method effect bounds at object construction. The vtable includes method slots and, for owning objects, destruction/layout information or a defined owning container protocol. Methods with unrestricted generic parameters are not object-safe in the first design. Borrowed and owning dynamic objects are different forms.

Do not make trait dispatch depend on inlining for correctness. Code must remain correct when the callee lives in another package or its implementation is selected dynamically.

## 9. Traits, modules, and advanced type features

### 9.1 Traits and coherence

Use named traits with associated types, declared method conventions/effects, and explicit generic assumptions. Adopt an orphan/coherence rule: an implementation must be owned by the trait's package or by an appropriately local nominal type, and applicable implementations must not overlap. No specialization or arbitrary negative reasoning in the first release.

Implement a canonicalized goal solver with memoization, cycle handling, and explicit ambiguity. A recursion limit is an implementation diagnostic, not permission to select an arbitrary candidate. Begin with a terminating restricted instance language. Generic bodies are checked under their declared predicates, independently of later monomorphization.

Treat safety-sensitive properties such as `Send`, `Sync`, `Copy`, and representation validity as compiler-checked structural properties or unsafe implementation contracts. Arbitrary safe code cannot assert a false one.

### 9.2 Modules

Provide file modules, explicit exports, signatures, opaque types, and local packages early. Persist type schemes, region relationships, effect bounds, trait identities, layout requirements, and drop summaries in interfaces. An implementation edit may not silently widen a public effect contract.

Add first-order static module parameters/functors later, reusing generic instantiation rather than introducing an unrelated compile-time object system. Specify applicative versus generative type identity. Support explicit generative instances for branded resource domains; do not infer their identity from incidental compiler build order.

### 9.3 High-value type features

| Feature | Example/use | Implementation boundary |
|---|---|---|
| Opaque newtypes | `EntityId`, `TextureId` | Nominal identity, representation erasure where allowed |
| Phantom parameters | `Vec3<World>`, `Vec3<View>` | Checked at compile time; no stored phantom payload |
| Const generics | `Mat<f32,4,4>`, `Array<T,N>` | Concrete evaluation plus a restricted symbolic equality solver |
| Units of measure | velocity, time, angle | Canonical dimension expressions and checked arithmetic |
| Typestate | `CommandBuffer<Recording>` | Consuming transitions and private constructors |
| GADTs | Typed expression/command IR | Branch-local equalities, skolems, annotations |
| Higher-rank functions | Scoped allocator and borrow callbacks | Explicit predicative `forall`, bidirectional checking |
| Existentials | Erased plugins/iterators | Explicit packaging; ownership/layout/destruction included |
| Associated type families | Lending iterators | Restricted, annotated region-indexed families in a later phase |
| Higher-kinded abstraction | Generic traversals | Later fixed-kinded constructors; no higher-order unification |

F# demonstrates a practical units-of-measure feature. Rill can use dimension exponent maps for canonicalization and erase the dimensions from runtime representation. Unknown symbolic dimensions require a defined restricted solver; arbitrary nonlinear arithmetic is not part of inference. [S12]

### 9.4 Constant expressions and shapes

Initial type-level constants are literals, parameters, and a small normalized affine arithmetic fragment. Evaluate closed expressions with the checked compile-time interpreter. For symbolic multiplication or arbitrary user functions, require equal normalized syntax, an explicit supported equality witness, or a runtime check; never assume an unproven equality.

`Index<N>` can certify a fixed-array bound when constructed by a checked operation. Dynamic collection indices need a region/identity relationship and invalidation discipline; an index checked before reallocation or shrinkage is not automatically valid afterward.

### 9.5 GADTs and higher rank

A GADT match introduces equalities and existential skolems only inside the matched branch. Roll back local solver refinements between branches. Existentials cannot escape through a result, closure capture, effect, or trait dictionary unless packaged explicitly. Require enough annotations for branch checking and polymorphic recursion.

OCaml's GADT documentation explicitly describes branch-local equalities and nonescaping existential variables. This is the behavior to reproduce in the corresponding Rill extension, rather than adding arbitrary constructor constraints to ordinary HM unification. [S13]

Higher-rank checking skolemizes a required `forall`, checks the body against rigid variables, then verifies none escaped. Instantiation remains predicative. Annotated higher-rank expressiveness is an intentional trade for predictable implementation and errors. [S4]

## 10. A formal core and its proof obligations

### 10.1 Judgments

Use separate static contexts:

```text
K   kind context
G   immutable type schemes and declarations
D   owned places and initialization state
L   active loans and provenance
C   generic/effect/region obligations

K; G; D; L |- expression => T ! effects => exits ; C
```

`exits` maps control reasons to states, rather than pretending every expression returns normally. Reasons initially include `Normal` and explicit error/failure; later add cancellation and transaction abort. Branch joins operate on corresponding reachable reasons. Aborting traps have no returning successor and no cleanup guarantee.

Elaboration produces a fully annotated core term with explicit type instantiation, dictionaries, borrow operations, transfers, effect operations, and cleanup. Erasure and machine lowering happen only after checking that core representation.

### 10.2 Representative rules

**Move.** An initialized owned place with no conflicting live loan can be consumed. Its ownership moves to the result and its source subplace becomes uninitialized.

```text
D(p) = Init(T)       available_for_move(p,L)
---------------------------------------------------
G;D;L |- move p => T ! {} => D[p := Uninit]
```

**Copy.** Copying requires `Copy(T)` and readable initialized storage. The source remains initialized. Reading externally observable storage contributes the applicable read effect; a local temporary's read can be encapsulated under the specified rule.

**Shared borrow.** Borrow an initialized place when no overlapping exclusive access is active. Add a loan whose required lifetime lies within the provenance/storage lifetime.

**Exclusive borrow.** Borrow when no overlapping access conflicts. Suspend the parent authority while a derived loan exists; reborrowing does not manufacture a second independent owner.

**Call.** Instantiate the function scheme. Evaluate operands left-to-right. Check argument conventions and their mutual overlap. Transfer or loan their places. Join operand, invocation, and required cleanup effects. Apply the callee's normal/failure initialization postconditions to the corresponding successor states.

**Let.** Generalize only permitted metavariables at an eligible binding, excluding variables reachable from the environment, store, loans, captured owners, and unresolved constraints that would violate the restriction.

**Branch.** Check branches against an expected result type or unify ordinary inferred results. For each returning reason, intersect definite-initialization facts; retain loan/region constraints necessary for either possible execution. Insert path-appropriate cleanup, not unconditional double drops.

**Scope.** End loans at verified last-use points. At scope exit, destroy each still-owned initialized local in the specified order and add its destruction effects. No result may contain a borrow of destroyed storage.

These are representative schemas. A full specification must supply value typing, store typing, substitution, every term/exit rule, and a complete operational semantics before claiming a proof.

### 10.3 Operational state

Define an abstract machine with expression/core instruction, typed store, ownership/loan ghost state, continuation/cleanup stack, and an observable event trace. Later extensions add task state and per-execution transaction logs. Memory operations, foreign boundaries, allocation/free, and resource destruction have explicit events.

Do not use emitted C as the language definition. The interpreter operates on typed abstract storage and detects invalid core transitions. The optimized runtime may erase ghost state once its preservation is justified.

### 10.4 Theorems to establish in order

1. Substitution/instantiation preserve kinding and well-formed constraints.
2. Type inference is sound relative to declarative typing for the stated rank-one fragment; principal constrained schemes only where established.
3. Preservation: a well-typed machine step preserves store, ownership, and expression typing.
4. Progress: a well-typed closed configuration is a value, a defined failure/trap/blocked external operation, or can step; it is not stuck on an invalid typed access.
5. Loan safety: conflicting exclusive accesses cannot coexist in reachable safe states.
6. Effect/resource soundness: observed primitive events are permitted by the instantiated row under its specified region abstraction and by the operational resource summary, including cleanup.
7. Elaboration preserves source observations and ownership transitions.
8. Destruction is single-owner and single-execution on the modeled normal/error/cancellation paths.
9. Later: scoped concurrency cannot use parent storage after its lifetime; transactions restore the specified state projection on abort.

Memory safety remains conditional on trusted primitives and foreign contracts. Safe wrappers around unsafe code need semantic obligations, not just a safe-looking signature. RustBelt provides a strong methodological precedent for this distinction. [S6]

### 10.5 Mechanization and trust

Use one proof assistant for the small core and keep its executable examples synchronized with the compiler fixtures. Start with a sequential affine calculus, extend to borrow regions and explicit store operations, then effects, then separately transactions/concurrency. Choose the assistant based on team capability; do not maintain parallel proofs for publicity.

The trusted computing base includes the checker, elaborator, primitive runtime, imported contracts, code generator, linker/toolchain assumptions, and target memory model. A verifier after each pass catches malformed IR, not every semantics-changing optimization. Differential testing, translation validation where practical, and proof arguments are complementary rather than interchangeable.

## 11. Memory, allocators, and representation

### 11.1 Storage vocabulary

Provide `Box<T,A>`, `Vec<T,A>`, owned `String<A>`, borrowed `Slice<r,T>` and `Str<r>`, explicitly shared `Rc<T>`/`Arc<T>` in hosted libraries, `Arena<r>`, fixed-capacity vectors, and generation-checked handles. These are distinct ownership/storage choices, not alternate hidden implementations of one magically cheap value type.

A vector or box retains enough allocator authority to release its storage correctly. An allocator that is borrowed from a local region imposes that region's lifetime on its allocations. An owned allocator handle may be stored in a container if its storage and thread-safety contracts permit it. Two allocator values with the same implementation type need not identify the same arena; instance identity matters.

Zig's documentation is useful precedent for explicit allocator selection, including fixed-buffer, arena, and general-purpose choices. Rill adds static lifetime and ownership checks around those choices rather than treating allocator parameters as sufficient by themselves. [S7]

### 11.2 Fallibility and arenas

Allocation-capable constructors return a result or use a clearly selected aborting allocator. Size arithmetic is checked before allocation. Reallocation invalidates outstanding views and is rejected while they remain live. Capacity is separate from initialized element count, and partial construction has explicit cleanup.

Resetting an arena requires proof that no live owned value or borrow still depends on the region. Reclaiming bytes does not run file, device, or other external-resource destructors automatically. Either restrict the arena to appropriate trivially destructible payloads or maintain a specified destruction list whose effects are charged to reset.

A frame scratch arena is useful only when every dependent CPU operation has finished. GPU submission can extend a resource's lifetime beyond the CPU frame boundary; a frame number is not evidence that device execution completed.

### 11.3 Layout contract

Default records, tuples, and ADTs use inline layouts. Recursion requires explicit indirection. Store payloads of arrays densely without hidden per-element boxes. Native layout is compiler-controlled and versioned; stable interop layout requires `repr(C)` or another explicit representation contract.

Compute size, alignment, discriminant, niches, valid bit patterns, field offsets, and function ABI classification separately. Layout equality is not automatically function-call ABI compatibility. Rust's reference explicitly distinguishes these concepts and documents the limited guarantees of default representations. [S14]

Niche optimization is permitted only where value validity and foreign boundaries preserve the invalid patterns used as niches. Do not assume every `Option<T>` is ABI-compatible with `T`, and do not expose a private native enum layout as a stable plugin interface.

### 11.4 Numeric and failure semantics

Fixed-width integers have specified widths; `usize`/`isize` match the target address-size contract. Default integer overflow traps in every build mode. Wrapping, checked-result, and saturating operators are distinct explicit operations. Bounds-checked indexing either returns an explicit option/result or uses a specified trapping operation; an optimized build cannot remove safety checks without a proof.

Narrowing integer and floating-to-integer conversions have explicit checked, saturating, or unsafe forms. Define shifts for out-of-range counts rather than inheriting C undefined behavior. Default floating operations preserve the selected strict semantics; reassociation, contraction, reciprocal approximations, and finite-only assumptions require an explicit fast-math policy. Deterministic simulation additionally needs specified math functions, input ordering, and target capabilities.

Use UTF-8 owned strings and byte-indexed borrowed string views with boundary-checked Unicode operations. C strings are separate nul-terminated byte representations. Conversion must check embedded nul policy and encoding instead of treating every string pointer as interchangeable.

## 12. Failure, search, transactions, and builders

### 12.1 Three separate mechanisms

Ordinary recoverable errors use `Result<T,E>` and library composition. `Option<T>` represents absence. Neither automatically rewinds mutation. A `decides` operation is an explicit early-failure/control effect; a handler decides how to turn it into a value or select alternatives. Transactions are an additional capability/operation that controls which mutations are reversible.

Under the recommended new surface, `option { ... }` handles `decides`; it does not silently undo arbitrary writes. `attempt { ... }` or `transaction { ... }` establishes rollback. Existing Rill examples whose meaning relies on rollback under `option` must receive a documented migration or retain a compatibility form. Do not change the meaning under the same syntax without an explicit version decision. [R0: ADRs 01–06 and §7]

The first failure release can restrict `option`/choice bodies to operations whose behavior is already specified. Plain `Result` handling remains available in ordinary native code without transactional overhead.

### 12.2 Search contract

Use ordered depth-first enumeration with duplicates preserved, unless a separately named fair search handler is selected. Single-result selection returns the first success and commits to that selection. Full enumeration retains alternatives across subsequent guards.

```rill
all {
    let x = choose [1; 2]
    let y = choose [10; 20]
    require (x + y > 15)
    yield (x + y)
}
// [21; 22]
```

This reproduces the nested-guard requirement in the uploaded plan using proposed syntax. A rewrite that drops the second alternative because the first succeeds is invalid under `all`. [R0: ADR 01, §5 MIR requirements]

Implement search with explicit generator/state-machine frames first, not arbitrary duplicable continuations. An ordinary affine value cannot be copied into two resumptions. Restrict replayed captures to a checked replay-safe subset or require explicit snapshot/clone operations with their effects. Purity alone is not enough: a pure computation can still consume a unique owner.

### 12.3 Transaction contract

An initial transaction accepts exclusive access to declared participating storage and one checked scratch allocator. All mutations of participants go through transaction-aware operations. Storage addresses remain valid until commit/abort. Reject external I/O, atomics visible to other threads, opaque FFI, suspension, resource destruction, and unjournaled ownership transfers.

Start with scalar/trivially copyable writes. Reserve an undo entry before the corresponding mutation. On abort, reverse writes, clean branch-local resources in a defined order, and restore allocator identity and checkpoint. Inner success retains the log required by an outer abort. Log exhaustion has a defined error path before an unrecorded write occurs.

For owned values, bitwise restoration is insufficient. A future move journal must describe who owns old and new values at each point, defer irreversible destruction to a valid commit point, and preserve outer rollback. Until that protocol is proved and tested, reject those mutations.

The rollback theorem concerns the declared transactional projection of program state. It does not restore the network, operating system, device queue, random external state, or arbitrary foreign library globals. Collected search results must live outside discarded branch storage or be explicitly transferred to surviving ownership.

### 12.4 Computation builders

Resolve builders as ordinary symbols with a typed protocol. Methods such as `bind`, `yield`, `combine`, `delay`, `for_each`, and `return` have declared conventions, effects, and capture behavior. Expansion is hygienic and preserves evaluation order and source spans.

Implement a genuine library-defined sequence/result builder before adding special GPU, SIMD, query, and allocation forms. Built-in transaction/search boundaries may have dedicated core operations; a user macro or builder cannot acquire their privileges merely by sharing a name.

### 12.5 General effect handlers

Defer unrestricted user-defined algebraic handlers. When added, classify continuation use explicitly as one-shot or replayable and include captured ownership and cleanup in the rule. The safe initial default is one-shot. A multishot handler requires a supported duplicable continuation environment and cannot capture an uncopyable owner, exclusive loan, or unsafe resource protocol by accident.

## 13. Structured concurrency and the memory model

### 13.1 First concurrency subset

Start with a deterministic single-thread executor, explicit task frames, cooperative cancellation, and scoped task groups. A parent cannot release borrowed storage until all children have completed or acknowledged cancellation and finished required cleanup. A dropped handle is not evidence that the work stopped.

Specify separate operations for all-complete joins, first-completion races, and first-success races. For first-success races, failed tasks do not defeat a still-running candidate; total failure collects/returns a specified error. Tie-breaking is deterministic in the reference executor. Completion, failure, cancellation, and abandoned/unreachable work are not interchangeable states.

### 13.2 Transfers, sharing, and suspension

Derive `Send` for values that can transfer safely between threads and `Sync` for values whose shared access is safe. Device-context or window handles may deliberately lack either property. Scoped borrowed captures require the parent's lifetime to cover every child path. Detached tasks, where offered, must own their data and capabilities independently.

Initially forbid outstanding exclusive projections and open transactions across suspension. Permit owned frame values. Add borrowed suspension later using pinned/stable frames, explicit loan relationships, and cancellation proofs. Moving a frame containing a self-reference is not safe simply because the coroutine is compiler-generated.

Atomics, locks, and trusted interior-mutability cells are explicit library boundaries. Start with a specified sequentially consistent atomic subset; add weaker orderings only with a memory-model specification, library review, and concurrency tests. No data races are permitted in the safe subset.

### 13.3 Scheduling and resource costs

Task creation can allocate and must declare it. Offer preallocated task pools and scoped inline frames for bounded workloads. Scheduler queue exhaustion cannot silently drop work. Cancellation is observed at specified suspension/checkpoints, not at an arbitrary machine instruction. A noncooperative task can delay a join; structured concurrency is not a termination theorem.

Record scheduler/task ownership in MIR. Tail-call and coroutine transforms must preserve cleanup, capability binding, and resource validity. Executor implementation is a library/runtime concern after the compiler exposes the required verified primitives.

## 14. Game and graphics programming model

### 14.1 Data-oriented libraries, not engine keywords

Supply dense arrays, fixed-capacity vectors, sparse sets, generation-checked slot maps, structure-of-arrays containers, intrusive structures behind unsafe APIs, bitsets, spatial indices, and allocator-aware collections. Persistent collections should be opt-in with their sharing/clone costs explicit.

Generation handles include an overflow policy: retire a slot on generation exhaustion or use another policy that cannot recreate a stale valid handle. “The counter probably will not wrap” is not a semantic safety argument. Validated references into a collection remain scoped so structural mutation cannot invalidate them.

A SoA container exposes field slices or a computed element projection. It cannot promise that an entire element is a single contiguous address. Computed projections must therefore obey the release/writeback rules rather than masquerading as a normal pointer.

### 14.2 ECS query safety

Represent queries with read/write component sets and scoped access tokens. Reject contradictory accesses within a query. A structural edit of the world is unavailable while query loans remain live; changes can be queued into a command buffer.

For jobs A and B, a sufficient conflict-free condition is:

```text
W_A intersection (R_B union W_B) = empty
W_B intersection (R_A union W_A) = empty
```

Component IDs, archetype membership, runtime filters, and entity partitions can be dynamic. A scheduler may check disjointness at runtime and issue scoped tokens to validated jobs. The type system protects token use; it does not magically know all future entity sets. Trusted query/scheduler internals require audited unsafe implementations and adversarial alias tests.

### 14.3 Render resources

Use types to distinguish resource categories, formats, address spaces, pipeline phases, and command-buffer states. Use a render graph plus runtime identity/range tracking for actual hazards, aliasing, queue ownership, and barriers. A `Texture<Readable>` phantom type does not prove that two differently named handles reference different memory.

GPU completion is distinct from CPU scope exit. Submitted work must keep required buffers, descriptors, pipelines, and callback contexts alive until the corresponding completion/fence protocol permits retirement. Resource wrappers may enqueue deferred destruction rather than immediately calling the device destructor; queue capacity and destruction effects must be explicit.

Khronos documents synchronization as an application responsibility in Vulkan. Rill should help express and validate this contract, not claim host ownership alone eliminates device synchronization errors. [S17]

### 14.4 Shaders

Initially integrate external shader compilers and generate checked host layouts/bindings. A later Rill shader subset can share ADTs, arithmetic, constants, and generic utilities, but must reject host pointers, unsupported recursion, allocation, open effects, and unsized dynamic dispatch.

Add distinct memory address spaces, workgroup/dispatch storage rules, barrier/derivative uniformity checking, and explicit uniform/storage-buffer layout standards. `repr(C)` is not a universal shader layout. Keep CPU and shader compilers differential against shared scalar tests where semantics actually coincide.

### 14.5 Practical development loop

Build an example windowed renderer, an asset loader, a scene/ECS update, a CPU software-rendering or image-processing kernel, and a freestanding numeric library. Supply profiler markers, source-level debugging, allocator diagnostics, and build caching. These projects should shape library design before a broad new syntax catalog does.

Hot reload goes through an explicitly versioned plugin ABI and state-migration protocol. Reload requires quiescing active jobs/callbacks and preserving or migrating live state. Do not promise that arbitrary suspended frames, vtables, or pointers into old code survive unloading.

## 15. C interoperability and native attachment

### 15.1 Import architecture

Use a real C frontend with the selected target, sysroot, include paths, defines, language mode, and compiler flags. Start with libclang where its stable interface suffices; use a small pinned Clang LibTooling helper for richer AST/ABI/preprocessor needs. The helper can emit a versioned serialized declaration graph to the Rust compiler. Clang documents the stability/full-AST tradeoff between these interfaces. [S15]

The importer handles declarations and layouts, not inferred ownership. Preserve `c_int`, `c_long`, `c_size_t`, signedness, alignment, nullable pointers, calling conventions, enum constants, unions, bitfields, and opaque types. Unsupported forms get diagnostics and suggested C shims, not approximate bindings.

### 15.2 Example attachment

Consider this **example C library that the project controls**, rather than a claim about an existing external API:

```c
/* particles.h */
#include <stddef.h>
typedef struct Particle {
    float px, py, vx, vy;
} Particle;

/* Synchronous; writes p[0..n); retains no pointer; no allocation/callbacks. */
void integrate_particles(Particle *p, size_t n, float dt);
```

A proposed build declaration attaches `particles.c`, its include directory, and target-specific options using structured process arguments. A proposed import `cimport "particles.h" as native` generates the raw declarations and target layout. Use the imported `Particle`, or a separately verified compatible representation, rather than duplicating the C declaration by hand.

```rill
let integrate_native (inout ps: SliceMut<native.Particle>) (dt: f32)
    : unit ! {write(ps)} =
    if not (ps.is_empty ()) then
        unsafe {
            native.integrate_particles(ps.raw_mut_ptr (), ps.len (), dt)
        }
```

This wrapper is safe **only if** the reviewed C implementation obeys the documented lifetime, bounds, aliasing, synchronization, and effect contract. The wrapper's exclusive slice proves valid access for the call; the C header alone does not prove no retention, no callbacks, or no hidden allocation. A checked/audited contract supplies the raw call's effect summary; an unknown import remains conservatively opaque.

### 15.3 Imported contracts

Support explicit metadata describing nullability, initialized/output ranges, ownership transfer, allocator/deallocator pairs, retained pointers, callback lifetime and threads, aliasing requirements, blocking, and effects. Mark whether a fact comes from syntax, generated ABI data, a recognized attribute, or an audited external assertion.

`const T*` does not guarantee that the function retains no pointer or that no other alias mutates the object. `restrict` does not make overlapping arguments safe. Null-terminated strings, fixed buffers, flexible-array members, and variable-length outputs need dedicated wrappers.

Unknown foreign enums should initially be imported as raw integer-backed wrappers with checked conversion to a closed Rill enum. Foreign bytes are not automatically a valid Rill boolean, nonnull reference, or discriminated union.

### 15.4 ABI lowering

Implement scalar calls first. For by-value aggregates, ABI classification is target-specific: registers, extension rules, hidden result pointers, padding, variadic promotions, and calling conventions matter beyond size/alignment. Initially generate C shims for difficult signatures and let the target C compiler perform classification; later implement direct lowering with cross-compiler ABI fixtures.

Callbacks use an explicit function pointer plus context pointer where the API supports it. A stack context is legal only for a synchronous nonretaining callback. Retained callbacks require owned stable context and an unregister/join protocol proving no callback remains in flight before release. Captureless function-pointer coercions do not imply that arbitrary closures are C ABI-compatible.

No Rill unwinding, C++ exception, or `longjmp` may cross a boundary that would skip required Rill cleanup/loan termination unless a separately specified interop mode supports it. Start with C++ wrappers exposing a C ABI; do not promise transparent C++ templates, exceptions, and object lifetimes.

### 15.5 Export, linkage, and caches

Export a deliberately narrow C ABI with generated headers, stable representation types, explicit error conventions, and explicit ownership of returned memory. Native Rill generics, enums, closures, and vtables are not a stable third-party ABI by default.

Include compiler version, target, flags, imported declarations, transitive headers, sysroot/toolchain identity, resolved libraries, and declared build inputs in cache keys. Invoke tools without shell interpolation. Handle spaces, non-UTF-8 paths where the platform supports them, interrupted builds, and concurrent cache publication.

## 16. Compiler architecture and extensibility

### 16.1 One checked semantic path

```text
Source and package graph
  -> lossless CST, spans, recovery
  -> surface AST and hygienic expansion
  -> resolved HIR
  -> typed HIR with explicit schemes, conventions, obligations, provisional effects
  -> structured place/control-flow MIR
  -> initialization, moves, loans, escape, capability checks
  -> cleanup elaboration and final effect/interface verification
  -> verified MIR
       -> reference interpreter
       -> readable C bootstrap/differential backend
       -> chosen first native backend
            -> direct LLVM, OR a deliberately small MLIR-to-LLVM path
       -> later validated shader/other target subset
```

Do not implement both direct LLVM and a broad custom MLIR stack in parallel before one is useful. The recommended default is interpreter plus C for validation, then direct LLVM for native development. Preserve a well-defined verified-MIR boundary so MLIR can later support multi-level vector/tensor work when justified by workloads.

MLIR's conversion framework supplies legality, rewrite, and type-conversion infrastructure. It does not define Rill ownership, transactions, ABI, or cancellation automatically. Each conversion must specify the invariant it preserves. [S19]

### 16.2 Semantic MIR operations

Represent `Init`, `Read`, `Copy`, `Move`, `Assign`, `BorrowShared`, `BorrowExclusive`, `EndLoan`, `Drop`, and calls separately. A call includes conventions, effects, ABI category, and exit successors. Preserve explicit lexical cleanup regions, failure handlers, projection acquire/release, and later task/transaction boundaries.

Values may use SSA while addressable places remain explicit. Do not erase semantic scopes into arbitrary jumps before proving cleanup and ownership. Generic semantic checking precedes machine layout. Layout-aware lowering then chooses registers, stack slots, aggregate returns, frame storage, and target instructions.

### 16.3 Implementation data structures

These are illustrative Rust-like data structures, not a complete implementation:

```rust
struct Scheme {
    binders: Vec<Binder>,
    predicates: Vec<Predicate>,
    body: TypeId,
}

struct FnSig {
    params: Vec<Param>,          // TypeId plus value/read/inout/set
    result: TypeId,
    invocation_effects: RowId,
    resource_summary: ResourceSummaryId,
    return_origins: OriginRelation,
    abi: Abi,
}

struct ClosureInfo {
    signature: FnSig,
    environment: TypeId,
    captures: Vec<Capture>,
    call_kind: CallKind,         // Fn, FnMut, FnOnce
    construction_effects: RowId,
    destruction_effects: RowId,
    construction_resources: ResourceSummaryId,
    destruction_resources: ResourceSummaryId,
}

struct Loan {
    place: PlaceId,
    kind: LoanKind,
    provenance: OriginSetId,
    required_points: PointSetId,
    parent: Option<LoanId>,
}

struct TypeVariable {
    kind: Kind,
    level: u32,
    universe: u32,
    binding: Option<TypeId>,
    origin: Span,
}
```

Use typed IDs and interned nodes. Keep type inference's union-find state out of finalized module interfaces. Canonicalize exported schemes and trait goals so incremental caching does not depend on arbitrary internal variable IDs.

### 16.4 Logical modules

Start with a small number of Rust crates and clear module boundaries: source/diagnostics; syntax/expansion; names/modules; types/constraints; MIR/verification; ownership/cleanup; interpreter; backend/ABI; driver/query service. Split more crates only when build and ownership boundaries stabilize.

The LSP must call the same semantic query service as the compiler. Cache keys include macro expansions and imported C dependencies. Error recovery may produce explicit error nodes, but successful code generation cannot accept unresolved error types, placeholder zeros, or unknown effect summaries.

### 16.5 Compile-time evaluation and macros

Use a typed, deterministic interpreter for constants, layout queries, generated bindings, and bounded specialization. There is no implicit filesystem, network, clock, process, or environment access. Declared build capabilities are separate and their inputs enter cache identities. Budget exhaustion is a compiler diagnostic, not a source runtime behavior.

Separate typechecking from staging. A generic body must check under its declared bounds, rather than depending on whichever instantiation happens to be evaluated first. Target-dependent constants use the target's arithmetic/layout rules, not the host's pointer width or floating-point shortcuts.

Expose hygienic AST builders and checked type/metadata queries. Macro-generated code re-enters ordinary resolution, typing, ownership, and effect checking. A macro cannot manufacture an unchecked proof token or mark an allocating function `noalloc`. Source/expansion spans remain available in errors.

Haxe documents AST-based macros at defined compilation stages and target-feature defines. The useful lesson is structured compiler extension and feature-aware compilation, not unrestricted mutation of an unversioned internal compiler graph. [S16, S18]

### 16.6 Plugin trust

Read-only analysis plugins and checked source-generating macros can use a narrow versioned API. Optimizer/backend plugins are a stronger trust boundary: a structurally valid transformation can still change semantics, and emitted machine code can violate safety. Mark them as trusted toolchain components and include versions in build identities. A MIR verifier is not a sandbox for an arbitrary backend.

## 17. Optimization program

### 17.1 Required source-level cost contracts

An ordinary record, enum, or tuple does not allocate. Constructing an unboxed concrete closure does not allocate merely because it is a closure. Generic calls use static dispatch by default. Dynamic dispatch, reference counting, allocation, and deep cloning have explicit operations/types. Containers report allocator effects and bounds behavior. These are language/library contracts, not promises that every such operation executes in zero instructions.

### 17.2 Transformations and legality conditions

| Transformation | Required fact | Failure mode to test |
|---|---|---|
| Monomorphization | Concrete type/layout and satisfied predicates | Explosion or nonterminating specialization |
| Arity specialization | Preserved argument order and partial-application timing | Effects run too early or more than once |
| Closure elimination | Environment/call behavior preserved | Dropped capture or escaped borrowed storage |
| Scalar replacement | No observable escaping identity or alias | Stale interior reference |
| Devirtualization | Proven unique implementation and matching ABI/effects | Wrong vtable/method convention |
| Bounds-check elimination | Dominating range/length proof survives mutations | Check invalidated by resize |
| Loop fusion | Same effects, failure, and drop order | Changed callback or destructor sequencing |
| Vectorization | Proven alias, alignment, arithmetic, and target conditions | Overlapping slices or invalid fast math |
| Tail-call conversion | No pending cleanup/borrow ABI obligation | Resource release skipped |
| Transaction-log elimination | Write unobservable outside provable local scope | Outer abort cannot restore inner writes |
| Search pruning | Correct handler/cardinality context | `all` loses valid alternatives |
| Coroutine-frame shrinking | Live values and cancellation cleanup retained | Use-after-free on resume/drop |

Emit noalias, nonnull, dereferenceable, range, and overflow metadata only when source semantics establish it for the required extent. A borrowed place in source is not a blanket license to attach every LLVM attribute. Unsafe and FFI boundaries can narrow the available proofs.

### 17.3 Compile-time and code-size controls

Monomorphize by default for native performance, but track each specialization's origin and cost. Deduplicate equivalent instantiations. Apply inlining budgets, shared cold paths, and optional explicit dictionary/erased APIs where code-size or compile-time tradeoffs justify them. Erasure must preserve layout and destruction semantics; it cannot be an unannounced runtime allocation.

Support incremental code generation, cached generic instantiations keyed by implementation/interface/toolchain identity, and link-time optimization as an opt-in build dimension. Whole-program optimization is not required for basic semantic correctness or allocation transparency.

### 17.4 Benchmarks

Use identical work and inputs for Rill, Rust, and C/C++ baselines. Pin compiler/toolchain settings and floating-point policy. Verify output checksums, mutated state, cleanup events, and task results before comparing times. Existing Rill benchmark lessons already show why apparently faster code can be semantically wrong. [R0: §3 and §11]

The workload set should include: dense particle updates; AoS/SoA traversal; matrix transforms; collision broadphase; ECS queries and structural edits; image decoding or asset parsing; closure/iterator pipelines; dynamic dispatch; allocator-heavy loading; scoped jobs; cancellation; C call/callback boundaries; and, separately, rollback/search.

Measure median and tail frame times, hardware-counter trends where available, allocations/bytes, peak working set, code size, compiler memory, clean build, and one-file incremental latency. Label confidence/variance. A kernel's result is not a whole-language performance claim.

**Acceptance targets:** zero prohibited allocations in a declared hot-path policy; no unexplained semantic discrepancy across backends/optimization levels; documented performance/code-size regressions against a frozen baseline. Numeric speed targets are project goals to set after the first correct implementations, not results established by this design.

## 18. Diagnostics, tools, and usability

Use stable error codes and structured diagnostics. A borrow error identifies the original place, conflicting access, lifetime requirement, and the operation keeping the loan alive. An effect error shows the transitive source of the prohibited action, including a destructor or callback. An FFI warning distinguishes imported syntax from an asserted safety contract.

Examples of desired diagnostics:

```text
E-LOAN-OVERLAP: two exclusive accesses may overlap
  first access:  values[i]
  second access: values[j]
  i and j are runtime values with no disjointness witness
  use a checked split/get-two operation, or restructure the access

E-EFFECT-ALLOC: @noalloc contract is not satisfied
  frame_update -> boxed_callback -> allocate environment
  construction allocates; callback invocation itself does not

E-DROP-BLOCK: leaving this scope may block
  local job_guard has destruction effect {block}
  explicitly join outside the realtime scope or use a supported nonblocking policy
```

Offer `check`, `build`, `run/exec`, `test`, `fmt`, MIR/LLVM dumps, C-binding inspection, layout inspection, and a compiler “cost explanation” report. Do not make formatting change parsing; preserve the legacy adjacency distinctions or version their removal. Keep formatting lossless for comments and idempotent. [R0: §9]

Inference errors should expose a compact principal/required type and the operation that introduced each constraint. Hide irrelevant internal region IDs by default, with an expert explanation mode. Friendly inference is not merely reducing annotations; it includes understandable rejection when a program is unsafe or ambiguous.

## 19. Dependency-ordered delivery plan

These are completion gates, not calendar estimates. A phase is complete only when its tests run in a reproducible required environment and documentation matches the accepted subset.

| Phase | Depends on | Concrete work | Required evidence |
|---|---|---|---|
| P0: contract and baseline | none | Archive source; execute baseline tests; classify examples; record numeric, ownership, effects, and migration ADRs | Baseline report and first normative examples; explicit known defects |
| P1: typed sequential kernel | P0 | Lexer/CST/parser; resolver; integers/bools/unit/functions; HM equality/levels; ADTs; interpreter; small C emitter | Same results across interpreter/C; malformed input diagnostics; exhaustiveness |
| P2: owned places | P1 | Value/read/inout/set conventions; field paths; moves; initialization; drop flags; lexical borrows then CFG regions | Reject aliasing, uninitialized reads, partial-move errors and escapes; cleanup traces |
| P3: effects and owned library | P2 | Closed effects; destructor summaries; allocator interface; box/vector/string/slices; ordinary Result | No hidden allocation; exhaustion cleanup; old text-storage regressions eliminated |
| P4: constraints and modules | P3 | Restricted traits; coherent goals; safe generalization; open effect rows; closure layouts/call kinds; package interfaces | Generic callbacks retain effects; mutable polymorphism rejected; cross-module behavior stable |
| P5: C integration | P2–P4 as needed | Clang import; scalar ABI; generated aggregate shims; callbacks; exports; cache identities | Bidirectional C/Rill layout/call tests on the first target; contracts audited |
| P6: useful native release | P3–P5 | First native backend; source debugging; allocation reports; math/data libraries; frame arena; simple renderer/asset pipeline | Working native app and reusable library; optimized/debug agreement; measured hot paths |
| P7: advanced static APIs | P4/P6 | Const shapes; units; phantoms; GADT checking; annotated rank-two APIs; bounded comptime; checked builders | Real typed graphics/asset APIs; skolem/constant/staging rejection tests |
| P8: scoped concurrency | P3/P4/P6 | Deterministic executor; frames; joins; cancellation; task-local capabilities; later Send/Sync and workers | No lost work; terminal states; parent storage retained; cancellation cleanup validated |
| P9: transactions and search | P3/P4/P7 | Declared participants; trivial-write undo; nested logs; replay-safe generator search; collector ownership | Nested abort restores state; capacity failure safe; alternatives and results preserved |
| P10: optimized graphics targets | P6–P8 | SIMD; profile-guided/code-size work; optional MLIR; shader subset or external shader integration; more native ABIs | Cross-backend conformance; validated layouts/barriers; benchmark distributions |
| P11: stabilization | useful phases | Compatibility policy; interface/ABI versions; standard-library audit; reproducible releases; optional self-hosted component experiment | Reproducible toolchain, stable user workflows, maintenance justification |

P5 can begin with manually written extern declarations and hand-authored verified MIR before all of P4 is complete. Advanced ABI, modules, and generic wrappers still depend on their actual prerequisites. P8 and P9 may share cleanup infrastructure but must not independently invent contradictory exit semantics.

A first useful language release is P6, not completion of the entire table. It need not support every advanced feature. Mark later syntax unsupported rather than parsing it and lowering a fabricated value. The phase numbering is intentionally different from the uploaded plan's M0–M11 sequence.

## 20. First implementation backlog

Each item is a reviewable work package with a bounded acceptance condition. Some run in parallel, but semantic contracts precede consumers.

| ID | Work package | Acceptance / rejection tests |
|---|---|---|
| C01 | Specify primitive values, order, integer operations, and traps | Same arithmetic behavior in interpreter and generated C at all tested optimizations |
| C02 | Implement type terms, kinds, union-find levels, occurs check | `a = List<a>` rejected; rigid variable escape rejected; substitution property tests |
| C03 | Implement rank-one instantiate/generalize with conservative restriction | Polymorphic identity accepted; polymorphic mutable slot rejected |
| C04 | Implement ADT constructors, patterns, exhaustiveness | Missing case rejected; impossible typed pattern diagnosed |
| C05 | Define places, roots, field projections, init tree | Field move does not initialize/move unrelated fields |
| C06 | Implement conventions and lexical loan baseline | Read/inout overlap rejected; distinct fields accepted |
| C07 | Add CFG liveness, loop joins, and inferred loan regions | Loans end at valid last use; loop/failure escapes rejected |
| C08 | Add cleanup IR and drop flags | Exactly one drop on each supported path; partially initialized aggregates cleaned |
| C09 | Closed effects including drop and unknown-FFI bounds | Pure/noalloc annotations rejected when cleanup violates them |
| C10 | Allocator API and owned buffers | Overflow/OOM produce specified outcomes; reallocation with live views rejected |
| C11 | Closure environment elaboration | Read-only owner capture callable repeatedly but not duplicated; once closure cannot be called twice |
| C12 | Qualified trait schemes/coherence | Generic code checks before instantiation; duplicate overlapping instances rejected |
| C13 | Open effect rows with constrained joins | Callback effects propagate; unresolved row never defaulted to empty |
| C14 | Module interfaces and canonical serialization | Changing an internal variable ID does not invalidate semantic caches; widening public effects is caught |
| C15 | C import helper and scalar calls | Target C integer widths/signs and header configuration match fixtures |
| C16 | Aggregate ABI via shims and callback lifetimes | C calls Rill and Rill calls C; retained stack context rejected |
| C17 | Native lowering and metadata audit | Noalias/overflow claims justified; generated code matches interpreter/C |
| C18 | Layout/allocation/dispatch explanation tooling | User can identify boxing, dynamic calls, copies, and implicit cleanup |
| C19 | Native vertical slice | Window + triangle/mesh + asset load + update loop with explicit frame storage |
| C20 | Formal core and regression linkage | Every mechanized example has a matching compiler test; theorem scope published |

Do not assign all twenty to “compiler work” in one issue. Each needs an owner, dependency IDs, accepted semantics, negative cases, and review artifacts. Release planning should be based on observed team throughput after these packages exist.

## 21. Adversarial conformance matrix

| Area | Must accept | Must reject or handle explicitly |
|---|---|---|
| HM polymorphism | Identity at two independent types | Same mutable slot instantiated inconsistently |
| Types/kinds | Well-kinded fixed-arity ADT applications | Type variable unified with a region/row/natural |
| Recursion | Monomorphic recursive group | Unannotated polymorphic recursion or infinite type |
| Effects | Effect-polymorphic callback | Hidden allocation in capture construction or drop |
| Row handling | Declared residual effects | Unknown tail silently treated as empty |
| Ownership | Move to a return value | Use after move; implicit deep clone |
| Copy | Structural trivial scalar aggregate | Copy of destructor-bearing/unique resource |
| Borrowing | Disjoint field loans | Unknown dynamic-index overlap |
| Reborrowing | Child loan inside parent authority | Parent accessed while child exclusive loan remains live |
| Initialization | Complete branch-wise initialization | Whole aggregate read after partial move |
| Drop | Path-specific initialized resources | Double destruction or omission on returning failure path |
| Closures | Noncopyable read-only closure called twice | Duplicated unique environment or expired borrowed capture |
| Arenas | Values destroyed before reset | Escaping borrowed allocation or reset with live dependents |
| Projections | Exactly-once writeback | Escaping temporary or failed/suspending release in unsupported subset |
| Traits | Coherent constrained implementation | Ambiguous/overlapping selection |
| Higher rank | Checked fresh-region callback | Skolem escapes through result, capture, or dictionary |
| GADTs | Local branch equality refinement | One branch's equality leaking into another |
| Constants | Supported normalized shape equality | Guessed nonlinear equality or host-width-dependent layout |
| ABI | Verified scalar/aggregate round trips | Treating same-size types as automatically ABI-compatible |
| FFI validity | Checked conversion of foreign bytes | Invalid bool/enum/reference manufactured in safe code |
| Callbacks | Owned retained context with quiescence | Context destroyed before in-flight callbacks finish |
| Concurrency | Joined scoped children | Parent returns while a child holds borrowed storage |
| Cancellation | One terminal state and one cleanup | Yielding task forgotten, queue overflow silently ignored |
| Transactions | Nested success then outer abort | Undo pointer to relocated storage or destroyed owning value |
| Search | Ordered nested alternatives and guards | Optimizer removes a result under enumeration |
| GPU lifetime | Deferred retirement after completion | CPU frame arena reset while device still uses it |
| GPU hazards | Validated disjoint resource/range schedule | Assuming differently named handles cannot alias |
| Tooling | Stable errors and idempotent formatting | Formatting changes parsing or stale LSP facts become codegen inputs |
| Caches | Reuse for identical complete inputs | Changed transitive header ignored |
| Target profiles | Explicit missing capability errors | Silent fallback to host allocation/runtime |

Property-test substitution, kinding, effect-row algebra, overlap symmetry, move-state joins, destruction counts, and transaction restoration. Fuzz syntax, imports, diagnostics, and small well-typed programs. Differential tests compare outputs, store projections, events, and cleanup, not just process exit codes. Generated C and runtime tests use memory/undefined-behavior instrumentation where available; thread testing begins when workers exist.

## 22. Decisions that remain genuinely open

The product needs a fixed native strategy before implementation: confirm the default move-versus-read convention using small representative programs; choose the precise finite-set row theory and prove its inference fragment; select the first host ABI; decide which traits/effect summaries are allowed in stable public interfaces; choose a proof assistant supported by the team; and choose the direct-LLVM versus minimal-MLIR native route.

The recommendations in this document make default choices, but these are the highest-risk areas for an implementation spike. Do not delay the whole project for every advanced question. Freeze the small accepted core, reject unsupported cases, and keep a decision ledger for extensions.

## 23. Final recommendation

The defensible language identity is **ML-level compositionality with explicit native resource semantics**. The differentiator is not a longer feature list. It is making the interaction of inference, ownership, effects, allocation, foreign code, and optimization understandable and testable.

Preserve Rill's ambitious search/projection/builder ideas, but make an ordinary allocation-controlled native program excellent first. Demonstrate the language with a renderer and a reusable C-facing library. Earn stronger soundness and performance claims through formal results and measured implementations; neither is established by the blueprint alone.

## References and evidence ledger

The external sources below were checked for this design on 2 November 2026. Some are evolving manuals; pin the exact document/toolchain revisions when implementation begins. URLs are provided as literal source identifiers. Each source supports the limited precedent attributed to it, not every new Rill proposal.

**[R0] User-provided *Rill rebuild plan*.** Source attachment `markdown.md`, 692 lines. Key anchors: purpose lines 5–20; review caveat 32–43; decisions 104–188; architecture 190–247; ownership/effects 249–275; rollback 277–312; backends/interop 342–381; milestones 383–517; validation 519–562. Its repository review and runtime observations are treated as that document's reported evidence, not independently reproduced here.

**[S1] Gabriel Radanne, Hannes Saffrich, Peter Thiemann, *Kindly Bent to Free Us* (Affe), ICFP 2020 / arXiv version.** ML-style affinity/linearity, borrowing, and inference precedent.  
`https://arxiv.org/abs/1908.09681`

**[S2] Aaron Weiss, Olek Gierczak, Daniel Patterson, Amal Ahmed, *Oxide: The Essence of Rust*.** Borrow checking, provenance, and progress/preservation for its specified core.  
`https://arxiv.org/abs/1903.00982`

**[S3] Daan Leijen, *Koka: Programming with Row Polymorphic Effect Types*.** Row-polymorphic effects and their semantic/inference treatment; the paper uses duplicate labels.  
`https://arxiv.org/abs/1406.2061`

**[S4] Jana Dunfield and Neelakantan R. Krishnaswami, *Complete and Easy Bidirectional Typechecking for Higher-Rank Polymorphism*, ICFP 2013.** Annotated predicative higher-rank checking precedent.  
`https://research.cs.queensu.ca/home/jana/papers/bidir/`

**[S5] OCaml manual, *Polymorphism and its limitations*.** Mutable polymorphism, value restriction, polymorphic recursion, and annotation boundaries.  
`https://ocaml.org/manual/5.5/polymorphism.html`

**[S6] Ralf Jung, Jacques-Henri Jourdan, Robbert Krebbers, Derek Dreyer, *RustBelt: Securing the Foundations of the Rust Programming Language*, POPL 2018.** Semantic verification obligations for safe interfaces over unsafe implementations.  
`https://plv.mpi-sws.org/rustbelt/popl18/`

**[S7] Zig Language Reference, Memory and comptime sections.** Explicit allocation and staging precedents, not a claim about identical safety semantics.  
`https://ziglang.org/documentation/master/`

**[S8] OxCaml documentation, *Introduction to the Mode System*.** Distinguishing value types from usage modes.  
`https://oxcaml.org/documentation/modes/intro/`

**[S9] OxCaml documentation, *Unboxed Types: Introduction*.** Layout-aware types and unboxed products.  
`https://oxcaml.org/documentation/unboxed-types/intro/`

**[S10] Hylo language tour, *Bindings*.** Mutable bindings and exclusive projected access.  
`https://hylo-lang.org/docs/user/language-tour/bindings/`

**[S11] Rust Reference, *Subtyping and Variance*.** Reference variance and mutable-type invariance comparison.  
`https://doc.rust-lang.org/reference/subtyping.html`

**[S12] Microsoft F# language reference, *Units of Measure*.** Compile-time dimensional checking.  
`https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure`

**[S13] OCaml manual, *Generalized Algebraic Datatypes*.** Branch-local equations and existential scope.  
`https://ocaml.org/manual/5.5/gadts-tutorial.html`

**[S14] Rust Reference, *Type Layout*.** Layout/ABI distinction and explicit representation guarantees.  
`https://doc.rust-lang.org/reference/type-layout.html`

**[S15] Clang documentation, *Choosing the Right Interface for Your Application*.** libclang versus LibTooling interfaces.  
`https://clang.llvm.org/docs/Tooling.html`

**[S16] Haxe Manual, *Macros*.** AST transformations at specified compiler stages.  
`https://haxe.org/manual/macro.html`

**[S17] Khronos Vulkan Guide, *Synchronization*.** Application-managed GPU synchronization.  
`https://docs.vulkan.org/guide/latest/synchronization.html`

**[S18] Haxe Manual, *Target Defines*.** Feature-aware target configuration.  
`https://haxe.org/manual/lf-target-defines.html`

**[S19] MLIR documentation, *Dialect Conversion*.** Conversion targets, legality, rewrite patterns, and type conversion.  
`https://mlir.llvm.org/docs/DialectConversion/`

**[S20] Rust Reference, *Behavior Considered Undefined*.** Useful contrast for unsafe-contract responsibilities; not adopted wholesale as Rill semantics.  
`https://doc.rust-lang.org/reference/behavior-considered-undefined.html`
