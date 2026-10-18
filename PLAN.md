# Rill Systems Architecture & Implementation Plan
**A Zero-Host, Mutable-Value-Semantics & Verse-Effect Native Systems Language in Rust + MLIR (`melior`)**

---

## 1. Vision, Design Philosophy & Core Pillars

**Rill** is a statically typed, natively compiled systems programming language designed to unify the high-level declarative expressiveness of **Verse** and **OCaml/F#** with the predictable, bare-metal performance and safety of **Rust**, **Hylo (Val)**, and **Zig**.

Rather than forcing developers to choose between functional ergonomics (garbage collection, hidden boxing, pervasive currying overhead) and systems control (viral lifetime annotations `'a`, complex pin projections, rigid syntax), Rill establishes five foundational pillars:

1. **Verse-Inspired Failure, Choice & Transactional Effects (`<decides>`, `<transacts>`, `<suspends>`)**:
   First-class lenient logic-functional control flow (`|`, `option`, `one`, `all`, `sync`, `race`, `rush`, `branch`) tracked statically via a row-polymorphic effect system and compiled down to zero-cost state machines, stack-allocated undo trails, and LLVM coroutines.
2. **Lifetime-Free Borrow Checking via Hylo Mutable Value Semantics (MVS)**:
   Instead of annotating types with viral lifetime parameters (`&'a mut T`), Rill enforces the **Law of Exclusivity** through explicit parameter passing conventions (`let`, `inout`, `sink`, `set`), **Struct/Slice Splitting** (disjoint path borrowing), and **First-Class Projections** (yielding temporary views into data structures via stack-bound coroutines).
3. **OCaml/F# Type Inference (`HM(X)`) with Zero-Cost Arity Specialization**:
   Full Hindley-Milner type and effect inference augmented with trait constraints and row polymorphism. Functions read and compose like ML (`x . f y`), but the compiler performs whole-module arity analysis and uncurrying so native calls pass arguments directly in registers following the C/System-V ABI.
4. **F#-Style Computation Expressions & Opt-In Custom DSLs**:
   Rill's built-in brace forms (`option { ... }`, `all { ... }`) are generalized into a user-extensible Computation Builder protocol. Systems programmers can opt into specialized syntax blocks (`simd { ... }`, `gpu { ... }`, `arena { ... }`, `query { ... }`) that desugar at compile time with zero runtime abstraction penalty.
5. **Zig-Style `comptime`, Explicit `<alloc>` Capabilities & Zero-Host Native Compilation**:
   Built in **Rust** using **`melior` (MLIR)** with minimal dependencies, Rill compiles directly to static, freestanding native binaries (and shaped tensor/SIMD kernels via MLIR's `linalg`/`vector` dialects) without requiring a runtime, garbage collector, or `libc`.

---

## 2. Expanded Surface Grammar & Syntax Specification

This grammar extends the baseline Rill specification to incorporate Hylo-style value conventions (`inout`, `sink`, `project`), Zig-style `comptime`, F#-style custom brace builders, and explicit memory/slice splitting while preserving Rill's clean, layout-independent syntax.

### 2.1 Lexical Grammar

```ebnf
document      = lexeme* EOF .
lexeme        = whitespace | comment | token .
whitespace    = " " | "\t" | "\n" | "\r" | Unicode_Space .
comment       = ( "#" | "//" ) character* newline
              | "/*" ( character | comment )* "*/" . (* nestable block comments *)

name          = ( letter | "_" ) ( letter | digit | "_" )* .
number        = "0x" hex_digit+ ( "_" hex_digit+ )* suffix?
              | "0b" bin_digit+ ( "_" bin_digit+ )* suffix?
              | digit+ ( "_" digit+ )* ( "." digit+ ( "_" digit+ )* )? suffix? .
suffix        = "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
              | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
              | "f16" | "f32" | "f64" .
text          = '"' text_char* '"' .
raw_text      = 'r#"' character* '"#' .
char          = "'" char_char "'" .
```

#### Reserved Keywords
```text
type   trait  impl   dyn    where    is       match   record  when   if     else
true   false  as     size   shapeof  reshape  let     use     for    var    set
inout  sink   view   project yield   comptime builder extern  unsafe deinit
```

*Contextual Identifiers*: `and`, `or`, `not`, `option`, `one`, `all`, `sync`, `race`, `rush`, `branch`, `spawn`, `computes`, `transacts`, `decides`, `suspends`, `alloc`, `reads`, `writes`, `io`, `fs`, `net`, `time`, `random` remain ordinary names in the lexer, interpreted by context in parser or type-resolution phases.

### 2.2 Module & Declaration Grammar

```ebnf
module         = statement* .

statement      = use | signature | let_decl | project_decl | type_decl
               | trait_decl | impl_decl | builder_decl | extern_decl
               | comptime_stmt | definition | propagate | var_stmt | set_stmt
               | yield_stmt | expression .

use            = "use" ( name ( "::" name )* ( "::" "{" name ( "," name )* "}" )? | text ) ";" .

signature      = name ":" type constraints? effects? ";" .

let_decl       = "let" name parameters? ( "->" convention? type )? constraints? effects? "=" expression ";" .

project_decl   = "project" name parameters ( "->" convention? type )? constraints? effects? "=" expression ";" .

parameters     = "(" ( parameter ( "," parameter )* )? ")" .
parameter      = ( "comptime" )? name ( ":" convention? type )? .
convention     = "let" | "inout" | "sink" | "set" .
```

* **Parameter Conventions**:
  * `let` (default if omitted): Immutable borrow/view; callee cannot mutate or consume the value.Zero-copy pass-by-value for register-sized types, pass-by-const-pointer for large aggregates.
  * `inout`: Exclusive mutable projection. Callee mutates the caller's place in-place; requires `&mut` exclusivity at the call site without lifetime syntax.
  * `sink`: Owned consumption (move semantics). Callee takes ownership and is responsible for destruction or forwarding.
  * `set`: Uninitialized out-parameter. Callee *must* initialize the binding before returning; enables guaranteed RVO (Return Value Optimization) and in-place construction.
* **`project` Declarations**:
  * Define a coroutine-backed projection (similar to Hylo subscripts) that `yield`s a `let` or `inout` view into an aggregate and resumes cleanup/invariants after the caller finishes using the projected value.

### 2.3 Types, Effects & Constraints

```ebnf
type           = type_atom ( "->" convention? type_atom )* ( "->" convention? type effects? )? .
type_atom      = ( "dyn" | "view" )? name ( "::" name )*
               | type_atom type_atom+
               | "[" type ( ";" comptime_expr )? "]"       (* fixed-size / shaped tensor type *)
               | "&" ( "raw" | "raw" "mut" ) type_atom     (* unsafe FFI raw pointers only *)
               | "(" type ")"
               | "(" type ( "," type )+ ")" .

effects        = "<" effect_row ">" .
effect_row     = effect_item ( "|" effect_item )* .
effect_item    = name ( type_atom )* .                     (* e.g. io, decides, alloc, State s, e *)

constraints    = "where" constraint ( "|" constraint )* .
constraint     = type "is" name ( type_atom )*
               | name ":" effect_row .
```

### 2.4 Algebraic Data Types, Records & Linear Deinit

```ebnf
type_decl      = "type" name type_param* "=" type_body ";" .
type_param     = name | "(" "comptime" name ":" type ")" .
type_body      = record_def | variants | type .

record_def     = "record" "{" field* deinit_clause? "}" .
field          = ( "var" )? name ":" type ";" .            (* "var" allows field-level splitting mutation *)
deinit_clause  = "deinit" "(" "sink" name ")" block .      (* deterministic RAII destructor *)

variants       = "|"? variant ( "|" variant )* .
variant        = name type_atom* .
```

### 2.5 Computation Builders (F#-Style DSLs) & Expressions

```ebnf
builder_decl   = "builder" name type_param* "=" "{" builder_member* "}" ";" .
builder_member = let_decl .

brace_form     = name "{" statement* expression? "}" .     (* Built-in or custom builder DSL *)
concurrent     = ( "sync" | "race" | "rush" | "branch" ) "{" expression ( ";" expression )* ";"? "}" .
comptime_expr  = "comptime" block .
yield_stmt     = "yield" ( "inout" )? expression ";" .
```

#### Operator Precedence Table (Lowest to Highest)

| Level | Operator(s) | Associativity | Semantics |
| ---: | --- | --- | --- |
| 1 | `\|` | Left | Backtracking choice (`<decides>`) |
| 2 | `..`, `..<` | Non-assoc | Inclusive (`..`) and half-open (`..<`) ranges |
| 3 | `or` | Left | Short-circuit logical OR (desugars to `if`) |
| 4 | `and` | Left | Short-circuit logical AND (desugars to `if`) |
| 5 | `=`, `==`, `!=` | Non-assoc | Structural equality / unification test |
| 6 | `<`, `>`, `<=`, `>=` | Non-assoc | Ordering comparison |
| 7 | `.` | Left | Uniform Function Call / Flow (`x . f y` $\equiv$ `f x y`) & Field Projection |
| 8 | `++` | Right | Array / Slice / Text concatenation |
| 9 | `+`, `-`, `^`, `\|&` | Left | Additive & bitwise XOR/OR |
| 10 | `*`, `/`, `%`, `<<`, `>>`, `&` | Left | Multiplicative & bitwise shift/AND |
| 11 | `as`, `is` | Left | Type cast / static layout reinterpretation |
| 12 | Postfix `f[x]`, `xs[i]`, `e?`, `f(x)` | Left | Adjacent failable call, indexing, option unwrap |
| 13 | Application `f x y` | Left | Space-separated curried/uncurried call |

---

## 3. Formal Type System, Ownership & HM(X) Inference Engine

Rill combines **Hindley-Milner inference with constraints ($\text{HM}(X)$)**, **Rémy-style Row Polymorphism** for algebraic effects, and **Substructural Place Conventions** for memory safety.

### 3.1 Core Type Algebra

The internal type representation $\tau \in \text{Type}$ in `crates/rill-ty` is stratified into:

$$
\begin{aligned}
\tau &::= \alpha && \text{(Unification Type Variable)} \\
     &\mid B && \text{(Base Primitives: } \text{i8}, \dots, \text{i128}, \text{f32}, \text{f64}, \text{Bool}, \text{Unit}\text{)} \\
     &\mid T(\bar{\tau}; \bar{k}) && \text{(Nominal ADT / Record / Tensor with shape constants } \bar{k}\text{)} \\
     &\mid (\kappa_1 \tau_1, \dots, \kappa_n \tau_n) \xrightarrow{\varepsilon} \kappa_r \tau_r && \text{(Multi-arity Function Type with conventions } \kappa_i \text{ and effect row } \varepsilon\text{)} \\
     &\mid \text{Proj}(\kappa, \tau, \pi) && \text{(Projected View of convention } \kappa \text{ tied to provenance set } \pi\text{)} \\
     &\mid \text{Comptime}(\tau) && \text{(Compile-time known meta-type)}
\end{aligned}
$$

Where Passing Conventions $\kappa \in \{\text{Let}, \text{Inout}, \text{Sink}, \text{Set}\}$ and Effect Rows $\varepsilon$ are defined as:

$$
\varepsilon ::= \langle \rangle \mid \langle e \mid \varepsilon \rangle \mid \rho
$$

Here, $\rho$ is an **Effect Row Unification Variable** allowing functions to be implicitly polymorphic over effects without verbose annotations.

### 3.2 Provenance-Free Signatures vs. Internal Provenance Sets ($\pi$)

A revolutionary aspect of Hylo/Rill compared to Rust is that **types in user-facing signatures do not carry lifetime parameters `'a`**.
* **Rule 1 (Parameter Exclusivity)**: In any function call `f(a, b)`, if either parameter has convention `inout` or `sink`, the underlying memory places of `a` and `b` must be **disjoint** ($ \text{Place}(a) \perp \text{Place}(b) $).
* **Rule 2 (Projection Tieback)**: A `project` function may return a $\text{Proj}(\text{Let} \mid \text{Inout}, \tau, \pi)$. Its provenance $\pi$ is automatically computed as the union of the input parameters marked `let` or `inout`. Because projections cannot be stored in heap structures or returned past their lexical borrow scope, $\pi$ never escapes into nominal struct definitions!

### 3.3 Formal Typing & Effect Judgments

Let $\Gamma$ be the immutable lexical typing environment, and $\Delta$ be the linear state environment mapping local places $p$ to their initialization and borrow state $S \in \{\text{Uninit}, \text{Owned}(\tau), \text{BorrowedLet}(n), \text{BorrowedInout}, \text{Moved}\}$.

A typing judgment has the form:

$$
\Gamma; \Delta \vdash e : \tau \ \langle \varepsilon \rangle \dashv \Delta'
$$

("Under static environment $\Gamma$ and pre-state $\Delta$, expression $e$ has type $\tau$, performs effects $\varepsilon$, and transitions local places to post-state $\Delta'$.")

#### 1. Failable Adjacent Call (`f[x]`)
In Rill, calling a function with `<decides>` uses adjacent brackets `f[x]` (or occurs inside a failable condition):

$$
\frac{
  \Gamma; \Delta \vdash f : (\text{Let } \tau_1) \xrightarrow{\langle \text{decides} \mid \varepsilon \rangle} \tau_2 \dashv \Delta_1 \quad
  \Gamma; \Delta_1 \vdash x : \tau_1 \ \langle \varepsilon \rangle \dashv \Delta_2
}{
  \Gamma; \Delta \vdash f[x] : \tau_2 \ \langle \text{decides} \mid \varepsilon \rangle \dashv \Delta_2
} \quad \text{[T-FailCall]}
$$

#### 2. Backtracking Choice (`e1 | e2`) with Transactional State Rollback
When `e1 | e2` is evaluated, if `e1` fails (`<decides>`), any mutations to outer state via `<writes>` or `<alloc>` inside `e1` must be rolled back to $\Delta$ before trying `e2`:

$$
\frac{
  \Gamma; \Delta \vdash e_1 : \tau \ \langle \text{decides} \mid \varepsilon_1 \rangle \dashv \Delta_1 \quad
  \Gamma; \Delta \vdash e_2 : \tau \ \langle \varepsilon_2 \rangle \dashv \Delta_2 \quad
  \varepsilon = (\varepsilon_1 \setminus \{\text{decides}\}) \cup \varepsilon_2
}{
  \Gamma; \Delta \vdash (e_1 \mid e_2) : \tau \ \langle \varepsilon \rangle \dashv \Delta_1 \sqcup \Delta_2
} \quad \text{[T-Choice]}
$$

Notice that if $e_2$ does **not** have the `decides` effect (i.e., $e_2$ is guaranteed to succeed as a fallback), then the combined expression `e1 | e2` **eliminates** the `decides` effect! For example:
```rill
let safe_head (xs: Array i32) -> i32 <computes> = xs[0] | 0;
```
Here `xs[0]` has effect `<decides>`, `0` has effect `<computes>` (empty row), so `xs[0] | 0` is pure (`<computes>`).

#### 3. Handler Elimination (`option { e }` and `all { e }`)

$$
\frac{
  \Gamma; \Delta \vdash e : \tau \ \langle \text{decides} \mid \varepsilon \rangle \dashv \Delta'
}{
  \Gamma; \Delta \vdash \text{option } \{ e \} : \text{Option } \tau \ \langle \varepsilon \rangle \dashv \Delta
} \quad \text{[T-OptionHandler]}
$$

$$
\frac{
  \Gamma; \Delta \vdash e : \tau \ \langle \text{decides} \mid \varepsilon \rangle \dashv \Delta'
}{
  \Gamma; \Delta \vdash \text{all } \{ e \} : \text{Array } \tau \ \langle \text{alloc} \mid \varepsilon \rangle \dashv \Delta
} \quad \text{[T-AllHandler]}
$$

---

## 4. Borrow Checking Without Lifetimes: Projections, Views & Splitting

Rill eliminates Rust's hardest ergonomic hurdle—explicit lifetime annotations (`'a`, `'b: 'a`, HRTBs `for<'a>`)—by adopting **Place-Path Splitting** and **Coroutine Projections**.

### 4.1 The Place Hierarchy & Disjoint Splitting

A **Place** $p$ is a path rooted at a local binding (`let`, `var`, `inout`, `sink`):

$$
p ::= x \mid p.\text{field} \mid p[i] \mid p[l..u]
$$

Two places $p_1$ and $p_2$ are **statically disjoint** ($p_1 \perp p_2$) if:
1. They have distinct root variables ($x \neq y$).
2. They share a root prefix $p$, and diverge at distinct struct fields ($p.f_1 \perp p.f_2$ where $f_1 \neq f_2$).
3. They share a root array/slice prefix $p$, and diverge via a verified `split` projection or compile-time disjoint index bounds ($p[0..k] \perp p[k..n]$).

```rill
type Transform = record {
    var position: Vec3;
    var rotation: Quat;
    var scale: Vec3;
};

let update_transform (t: inout Transform) <computes> = {
    # Both `t.position` and `t.rotation` are borrowed simultaneously!
    # Because `position` != `rotation`, the Place Checker proves disjointness:
    # Place(t.position) ⊥ Place(t.rotation)
     rotate_point (inout t.position, t.rotation);
};
```

### 4.2 First-Class Projections (`project` and `yield`)

In Rust, returning a reference from a getter requires tying `'a` from `&'a mut self` to `&'a mut Field`, and breaks completely if the value needs to be computed on the fly and written back on drop (like a bitpacked field or Swizzled SIMD vector).

In Rill, a **Projection** is a stack-allocated, inlineable micro-coroutine:

```rill
# Yielding an in-place mutable view into a dynamic array with bounds checking
project at (xs: inout Array a, idx: usize) -> inout a <decides> = {
    idx < size xs; # Failable guard (<decides>)
    ptr := unsafe { array_raw_ptr_mut (inout xs) + idx };
    yield inout (*ptr);
    # Code here runs AFTER the caller finishes mutating the projected element!
};

# Virtual / Computed Projection (impossible with Rust references!)
type PackedColor = record { var rgba: u32; };

project redChannel (c: inout PackedColor) -> inout u8 <computes> = {
    var r: u8 = ((c.rgba >> 24) & 0xFF) as u8;
    yield inout r;
    # Automatically writes back `r` into `c.rgba` when caller's borrow of `r` ends!
    set c.rgba = (c.rgba & 0x00FFFFFF) | ((r as u32) << 24);
};
```

When a user writes:
```rill
var color = PackedColor 0x10203040u32;
set (color . redChannel) = 0xFFu8;
```
The compiler lowers `set (color . redChannel) = 0xFFu8` by inlining the `redChannel` projection around the mutation:
1. Prolog: `var r: u8 = ((color.rgba >> 24) & 0xFF) as u8;`
2. Body (at `yield inout r`): `set r = 0xFFu8;`
3. Epilog: `set color.rgba = (color.rgba & 0x00FFFFFF) | ((r as u32) << 24);`

Even if the projection cannot be inlined (e.g., across a dynamic trait `dyn`), MLIR lowers it to a two-pointer continuation `(value_ptr, resume_fn_ptr)` allocated purely on the stack!

### 4.3 View Types & Slice Splitting

For scenarios where a struct needs to bundle multiple borrows temporarily (e.g., an iterator or window over a matrix), Rill provides `view` types:

```rill
type MatrixView a = view {
    data: Slice a;
    rows: usize;
    cols: usize;
};

# Built-in slice splitting projection
project split_at (s: inout Slice a, mid: usize) -> inout (Slice a, Slice a) <decides> = {
    mid <= size s;
    var pair = unsafe { raw_slice_split (inout s, mid) };
    yield inout pair;
};
```

Rules enforced on `view` types by `crates/rill-borrow`:
1. A `view` type can only reside on the stack (in `let` or `var` bindings with scoped provenance $\pi$).
2. A `view` type cannot be stored inside a nominal `record` or heap container (`Array`, `Box`).
3. When a `view` is created (`v := MatrixView buf r c`), the borrow checker records that `v` holds a `BorrowedLet` (or `BorrowedInout`) lock on `Place(buf)` until the last lexical use of `v` (Non-Lexical Lifetimes / Polonius-lite liveness analysis).

---

## 5. The Verse-Inspired Effect, Failure & Transactional Engine

Rill takes Verse's most celebrated ideas—failable expressions, logic variables/choice, transactional rollback, and structured concurrency—and grounds them in a zero-cost static compilation model.

### 5.1 `<decides>` and Nondeterministic Choice (`|`)

Unlike exceptions or monadic `Result<T, E>` clutter, `<decides>` treats failure as lightweight structured branching.

#### Compilation Strategy (Zero-Heap Failure Continuation Passing)
In MLIR (`rill_mir`), any expression or function with `<decides>` is compiled with TWO control-flow exits:
1. **Success Edge (`bb_success(val, next_choice_token)`)**
2. **Failure Edge (`bb_fail()`)**

Case breakdown:
* **Case A: Single-Result Failable Context (`if (x := f[y])`, `option { ... }`, `one { ... }`)**:
  When a `<decides>` expression does not contain the multi-branch choice operator `|` (i.e., it is *semideterministic*—at most one result), it lowers to a simple conditional branch (`cf.cond_br`) or an `{ i1, T }` return register pair! This has **identical assembly output** to checking a boolean return code in C.
* **Case B: Multi-Result Nondeterministic Choice (`all { x := (1 | 2 | 3); y := (10 | 20); x + y }`)**:
  When `|` or range generators (`1..n`) are driven by `all` or a backtracking condition, the compiler applies **Inline Stream Fusion (Deforestation)**:
  - `x := 1..3` lowers directly to a `scf.for` loop.
  - `x := (a | b)` lowers to duplicating or sequencing the continuation block for `a` and `b`.
  - If `|` appears inside a non-inlined recursive function, it lowers to a stackless generator state machine (`CoroutineFrame`) passed by `inout` pointer from the caller!

### 5.2 `<transacts>`: Speculative Execution & Undo Trails

In Verse, if a failable block mutates state (`set x = 10`) and subsequently fails (`false?`), the mutation is rolled back as if it never happened.

In Rill, this is governed statically by the effect lattice:

$$
\text{transacts} \equiv \text{reads} \mid \text{writes} \mid \text{alloc}
$$

When an expression combining `<decides>` and `<writes | alloc>` executes inside a speculative handler (`if (...)`, `option { ... }`, `e1 | e2`), the compiler inserts a **Speculative Transaction Scope**:

1. **Local Scalar Promotion (Zero Overhead)**:
   If the mutated `var` is local to the function and fits in registers, MLIR's `mem2reg` (SSA construction) turns the speculative mutation into SSA $\phi$-nodes (block arguments). On the `bb_fail` edge, control simply jumps to the `else` block using the original SSA value! **Cost: 0 instructions.**
2. **Thread-Local Undo Trail (For `inout` Heap/Struct Mutations)**:
   If a speculative block mutates an external `inout` place (`set player.health = 0`) before a potential failure point:
   - Before the write, the compiler emits `rill.txn_record_write(&player.health, sizeof(i32))`.
   - This pushes `(address, old_bytes)` onto a fast, pre-allocated thread-local bump stack (`UndoTrail`).
   - Any `<alloc>` inside the speculative block allocates from a temporary `ScratchArena` watermark.
   - If the block succeeds (`rill.txn_commit`), the `UndoTrail` watermark is simply popped (`trail.len = saved_len`).
   - If the block fails (`rill.txn_abort`), the runtime walks the trail backward from `current_len` to `saved_len`, `memcpy`ing original bytes back and resetting the arena pointer!

### 5.3 `<suspends>` & Structured Concurrency (`sync`, `race`, `rush`, `branch`, `spawn`)

Rill implements Verse's full quartet of structured concurrency primitives directly over stackless MLIR coroutines:

| Form | Completion Rule | Failure (`<decides>`) Rule | Cancellation Behavior |
| --- | --- | --- | --- |
| `sync { A; B }` | Waits for **all** arms (`A` and `B`) to finish; returns tuple `(valA, valB)`. | If any arm fails, cancels remaining arms and propagates failure. | All arms cancelled if parent task cancels. |
| `race { A; B }` | Completes as soon as the **first** arm succeeds; returns winner's value. | Fails only if **all** arms fail. | Immediately cancels losing arms at their next suspension point. |
| `rush { A; B }` | Completes as soon as the **first** arm finishes; returns winner's value. | Fails if winner fails. | Losing arms **continue running** in the background until enclosing function exits. |
| `branch { A; B }` | Spawns background work tied to current scope's lifetime without awaiting immediately. | N/A (`Unit` result). | Automatically cancelled when the enclosing lexical scope exits. |

### 5.4 `<alloc>` Capability & Scoped Memory Management

Every heap allocation in Rill (`Array::push`, `Box::new`, string formatting) requires the `<alloc>` effect:

```rill
let build_list (n: i32) -> Array i32 <alloc> = {
    var out = Array::with_capacity n;
    for (i := 0..n) out . push i;
    out
};
```

How is `<alloc>` satisfied?
1. **Default Global Allocator**: In `main () <io | alloc>`, `<alloc>` defaults to the platform's fast mimalloc/system allocator (or a page allocator in freestanding targets).
2. **Custom Scoped Arena Handler (`with_allocator`)**:
   Because `<alloc>` is an effect, any caller can intercept it and satisfy `<alloc>` purely on the stack or in a fixed buffer—eliminating the `<alloc>` effect from the caller's signature!

```rill
let process_packet (buf: inout [u8; 4096]) -> i32 <computes> = {
    # `StackArena::run` handles `<alloc>` using `buf` and discharges the `<alloc>` effect!
    StackArena::run (inout buf) {
        items := build_list 100; # Uses `buf` via implicit effect context pointer!
        items . fold 0 { acc, x -> acc + x }
    }
};
```

---

## 6. F#-Style Computation Expressions & Opt-In Custom DSLs

To keep the core compiler small while allowing extreme domain-specific ergonomics and performance, Rill generalizes its brace blocks (`name { ... }`) into **Computation Builders** (inspired by F# Computation Expressions, upgraded with `comptime` specialization and `inout` value semantics).

### 6.1 The Desugaring Rules for `builder_name { ... }`

When the parser encounters `b { stmts...; expr }` where `b` resolves to a `builder` or a value whose type implements a builder schema, the AST desugars the block statements structurally:

| Surface Syntax inside `b { ... }` | Desugared Builder Method Call |
| --- | --- |
| `x := expr; rest` | `b . bind (expr) { x -> [[rest]] }` |
| `let? x = expr; rest` | `b . try_bind (expr) { x -> [[rest]] }` |
| `yield expr;` | `b . yield_val (expr)` |
| `for (x : xs) body` | `b . for_each (xs) { x -> [[body]] }` |
| `if (cond) { t } else { f }` | `if (cond) { [[t]] } else { [[f]] }` |
| `stmt1; stmt2` | `b . combine ([[stmt1]]) (b . delay { [[stmt2]] })` |
| Final `expr` | `b . ret (expr)` |

### 6.2 Example 1: Zero-Cost Iterator / Sequence Builder (`seq { ... }`)

```rill
builder seq = {
    let yield_val (x: a) -> Step a <computes> = Step::Yield x;
    let combine (s1: Step a, s2: () -> Step a) -> Step a <computes> = Step::Chain s1 s2;
    let ret (u: ()) -> Step a <computes> = Step::Done;
};

# Usage:
let evens (n: i32) = seq {
    for (i : 0..n) {
        if (i % 2 = 0) { yield i } else { () }
    }
};
```

Because `combine` and `delay` are monomorphized and inlined by MLIR's `canonicalize` + `sroa` passes, `seq { ... }` compiles into a flat loop with zero heap closures.

### 6.3 Example 2: Opt-In SIMD / Tensor Kernel DSL (`simd { ... }`)

Systems programmers can opt into specialized hardware dialects using a `simd` or `gpu` builder that maps directly to MLIR's `vector` and `linalg` dialects via `comptime` intrinsics:

```rill
let saxpy (a: f32, x: Slice f32, y: inout Slice f32) <computes> = {
    simd {
        # Desugars to MLIR `affine.parallel` + `vector.fma`
        lane := lanes 8;
        idx := range (0 ..< size x) lane;
        vx := load x[idx];
        vy := load y[idx];
        store (inout y[idx]) (fma a vx vy)
    }
};
```

---

## 7. Compiler Architecture in Rust + `melior` (MLIR)

We design the compiler as a fast, modular, zero-bloat Rust workspace (`rillc`).

### 7.1 Workspace Crate Topology

```text
rill/
├── Cargo.toml
└── crates/
    ├── rill-span/       # Interned strings (Symbol), BytePos, Span, SourceMap (0 deps)
    ├── rill-diagnostics/# Ariadne-style rich terminal error rendering with hints
    ├── rill-lexer/      # Hand-written SIMD-friendly cursor lexer (implements §1)
    ├── rill-syntax/     # Lossless Concrete Syntax Tree (CST) & operator precedence table
    ├── rill-parser/     # Resilient Pratt + recursive descent parser (produces CST/AST)
    ├── rill-ast/        # Typed AST nodes & Computation Builder desugaring pass
    ├── rill-hir/        # Name resolution, module graph, desugared High-Level IR
    ├── rill-ty/         # HM(X) bidirectional type + effect row inference engine
    ├── rill-mir/        # Mid-Level IR (CFG with Explicit Places, Projections & Effect Scopes)
    ├── rill-borrow/     # Hylo-style Exclusivity, Place-Splitting & Projection Checker
    ├── rill-comptime/   # Fast bytecode/MIR interpreter for `comptime` & type evaluation
    ├── rill-mlir/       # `melior` backend: Custom MLIR dialects -> LLVM IR lowering
    └── rill-driver/     # CLI, incremental query cache, `lld` static binary linker driver
```

#### Strict Dependency Policy
* Only essential crates: `melior` (and `mlir-sys`), `rustc-hash` (for fast `FxHashMap`), `smallvec` (stack-allocated vectors for AST/IR children).
* **No heavy macro frameworks or parser generators**: The lexer and parser are hand-written in <2,500 lines of clean Rust, making future self-hosting into Rill straightforward.

### 7.2 Pipeline Dataflow Diagram

```text
[Source .rill]
      │
      ▼ (rill-lexer + rill-parser)
[Surface AST] ──(Builder Desugar: `seq {..}`, `and`/`or`)──► [Core HIR]
                                                                  │
      ┌───────────────────────────────────────────────────────────┘
      ▼ (rill-ty: Bidirectional HM(X) + Row Unification)
[Typed HIR] ──(rill-comptime: Evaluate `comptime` & Tensor Shapes)
      │
      ▼ (Arity Uncurrying + Monomorphization + Lowering)
[Rill MIR (SSA + Place Projections)]
      │
      ├──► (rill-borrow: Law of Exclusivity, Struct Splitting, Projection Lifetimes)
      │
      ▼ (rill-mlir via `melior`)
[High-Level MLIR: `rill` + `linalg` + `scf` + `async` dialects]
      │
      ▼ (MLIR Conversion Passes: Choice/Fail CPS, Transaction Trails, Coroutines)
[Low-Level MLIR: `llvm` + `cf` + `arith` + `vector` dialects]
      │
      ▼ (MLIR -> LLVM IR Translation + Target Machine Opts O3)
[Static Native Object (.o)] ──(Embedded lld / system linker)──► [Zero-Host Static Binary]
```

### 7.3 The Custom `rill` MLIR Dialect Operations

Rather than lowering directly from AST to ugly LLVM IR, we leverage `melior` to construct a structured MLIR representation that preserves high-level semantics for optimization:

1. **`rill.choice` & `rill.fail`**:
   - Represents `<decides>` control flow.
   - Optimization pattern (`CanonicalizeChoice`):
     - `rill.choice(rill.fail, %x)` $\longrightarrow$ `%x`
     - `rill.choice(%pure_const, %y)` $\longrightarrow$ `%pure_const` (Dead branch elimination!)
2. **`rill.txn_region` / `rill.record_write`**:
   - Encapsulates a `<transacts>` + `<decides>` region.
   - Optimization pattern (`PromoteLocalTxnToSSA`):
     - If a `memref.alloca` inside or outside `rill.txn_region` does not escape to an external call, eliminate `rill.record_write` and convert the memory slot directly to SSA block arguments across the success/failure edges.
3. **`rill.project` & `rill.yield_inout`**:
   - Represents Hylo-style projections.
   - Optimization pattern (`InlineProjection`):
     - Splices the callee block inside the projection directly at the `rill.yield_inout` point, turning virtual getters/setters into zero-cost register operations.

---

## 8. Concrete Rust + `melior` Implementation Blueprints

Below are production-ready architectural implementations for the three most critical subsystems of the Rill compiler:
1. **The Type & Row-Polymorphic Effect Unification Engine (`crates/rill-ty`)**
2. **The Hylo-Style Place-Splitting & Exclusivity Borrow Checker (`crates/rill-borrow`)**
3. **The `melior` MLIR Codegen Backend for `<decides>`, `<transacts>`, and `inout` Projections (`crates/rill-mlir`)**

### 8.1 Blueprint 1: HM(X) Type & Effect Row Unifier (`crates/rill-ty/src/infer.rs`)

```rust
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TyVar(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RowVar(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Convention {
    Let,
    Inout,
    Sink,
    Set,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum BuiltinEffect {
    Io,
    Fs,
    Net,
    Time,
    Random,
    Alloc,
    Decides,
    Reads,
    Writes,
    Suspends,
    Custom(String),
}

/// Rémy-style Effect Row: `< e1 | e2 | ... | rho >`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectRow {
    pub effects: SmallVec<[BuiltinEffect; 4]>,
    pub tail: Option<RowVar>,
}

impl EffectRow {
    pub fn computes() -> Self {
        Self {
            effects: SmallVec::new(),
            tail: None,
        }
    }

    pub fn transacts() -> Self {
        let mut effects = SmallVec::new();
        effects.push(BuiltinEffect::Reads);
        effects.push(BuiltinEffect::Writes);
        effects.push(BuiltinEffect::Alloc);
        Self { effects, tail: None }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuncParam {
    pub convention: Convention,
    pub ty: Ty,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    Var(TyVar),
    Int(u8, bool), // (bits, signed)
    Float(u8),     // 16, 32, 64
    Bool,
    Unit,
    Adt(u32, Vec<Ty>),
    Tensor {
        elem: Box<Ty>,
        shape: SmallVec<[usize; 4]>,
    },
    Func {
        params: SmallVec<[FuncParam; 4]>,
        ret_conv: Convention,
        ret: Box<Ty>,
        effects: EffectRow,
    },
}

pub struct InferCtx {
    ty_subst: FxHashMap<TyVar, Ty>,
    row_subst: FxHashMap<RowVar, EffectRow>,
    next_ty_var: u32,
    next_row_var: u32,
}

impl InferCtx {
    pub fn new() -> Self {
        Self {
            ty_subst: FxHashMap::default(),
            row_subst: FxHashMap::default(),
            next_ty_var: 0,
            next_row_var: 0,
        }
    }

    pub fn fresh_ty_var(&mut self) -> Ty {
        let id = self.next_ty_var;
        self.next_ty_var += 1;
        Ty::Var(TyVar(id))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let id = self.next_row_var;
        self.next_row_var += 1;
        RowVar(id)
    }

    pub fn normalize_ty(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(v) => {
                if let Some(resolved) = self.ty_subst.get(v) {
                    self.normalize_ty(resolved)
                } else {
                    ty.clone()
                }
            }
            Ty::Func { params, ret_conv, ret, effects } => Ty::Func {
                params: params
                    .iter()
                    .map(|p| FuncParam {
                        convention: p.convention,
                        ty: self.normalize_ty(&p.ty),
                    })
                    .collect(),
                ret_conv: *ret_conv,
                ret: Box::new(self.normalize_ty(ret)),
                effects: self.normalize_row(effects),
            },
            Ty::Adt(id, args) => {
                Ty::Adt(*id, args.iter().map(|a| self.normalize_ty(a)).collect())
            }
            Ty::Tensor { elem, shape } => Ty::Tensor {
                elem: Box::new(self.normalize_ty(elem)),
                shape: shape.clone(),
            },
            _ => ty.clone(),
        }
    }

    pub fn normalize_row(&self, row: &EffectRow) -> EffectRow {
        let mut flat_effects = row.effects.clone();
        let mut curr_tail = row.tail;

        while let Some(rv) = curr_tail {
            if let Some(sub) = self.row_subst.get(&rv) {
                for eff in &sub.effects {
                    if !flat_effects.contains(eff) {
                        flat_effects.push(eff.clone());
                    }
                }
                curr_tail = sub.tail;
            } else {
                break;
            }
        }
        // Canonical sort for deterministic row comparison
        flat_effects.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
        flat_effects.dedup();
        EffectRow {
            effects: flat_effects,
            tail: curr_tail,
        }
    }

    /// Unifies two effect rows `< E1 | r1 >` and `< E2 | r2 >` using Rémy row rewriting
    pub fn unify_rows(&mut self, r1: &EffectRow, r2: &EffectRow) -> Result<(), String> {
        let r1 = self.normalize_row(r1);
        let r2 = self.normalize_row(r2);

        // Compute set differences: (E1 \ E2) and (E2 \ E1)
        let only_in_1: SmallVec<[BuiltinEffect; 4]> = r1
            .effects
            .iter()
            .filter(|e| !r2.effects.contains(e))
            .cloned()
            .collect();

        let only_in_2: SmallVec<[BuiltinEffect; 4]> = r2
            .effects
            .iter()
            .filter(|e| !r1.effects.contains(e))
            .cloned()
            .collect();

        match (r1.tail, r2.tail) {
            (None, None) => {
                if only_in_1.is_empty() && only_in_2.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Effect mismatch: missing {:?} vs {:?}",
                        only_in_1, only_in_2
                    ))
                }
            }
            (Some(tail1), None) => {
                if !only_in_1.is_empty() {
                    return Err(format!("Unexpected effects {:?} in closed row", only_in_1));
                }
                self.row_subst.insert(
                    tail1,
                    EffectRow {
                        effects: only_in_2,
                        tail: None,
                    },
                );
                Ok(())
            }
            (None, Some(tail2)) => {
                if !only_in_2.is_empty() {
                    return Err(format!("Unexpected effects {:?} in closed row", only_in_2));
                }
                self.row_subst.insert(
                    tail2,
                    EffectRow {
                        effects: only_in_1,
                        tail: None,
                    },
                );
                Ok(())
            }
            (Some(tail1), Some(tail2)) if tail1 == tail2 => {
                if only_in_1.is_empty() && only_in_2.is_empty() {
                    Ok(())
                } else {
                    Err("Cannot unify identical row tails with distinct effect prefixes".into())
                }
            }
            (Some(tail1), Some(tail2)) => {
                let common_tail = self.fresh_row_var();
                self.row_subst.insert(
                    tail1,
                    EffectRow {
                        effects: only_in_2,
                        tail: Some(common_tail),
                    },
                );
                self.row_subst.insert(
                    tail2,
                    EffectRow {
                        effects: only_in_1,
                        tail: Some(common_tail),
                    },
                );
                Ok(())
            }
        }
    }
}
```

---

### 8.2 Blueprint 2: Hylo-Style Place Splitting & Exclusivity Checker (`crates/rill-borrow/src/lib.rs`)

This module enforces memory safety **without lifetime annotations**, proving that simultaneous `inout` / `let` borrows operate on disjoint struct fields or split array slices.

```rust
use smallvec::SmallVec;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FieldIdx(pub u32);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ProjectionElem {
    Field(FieldIdx),
    ConstIndex(usize),
    /// Created by `split_at(mid)`: DisjointLeft = [0..mid], DisjointRight = [mid..len]
    SliceSplitLeft { split_id: u32 },
    SliceSplitRight { split_id: u32 },
}

/// Represents a concrete memory path, e.g., `player.transform.position` or `xs.left_half`
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Place {
    pub root: LocalId,
    pub path: SmallVec<[ProjectionElem; 4]>,
}

impl Place {
    /// Returns true if `self` and `other` are guaranteed to point to non-overlapping memory.
    pub fn is_disjoint_from(&self, other: &Place) -> bool {
        if self.root != other.root {
            return true;
        }
        // Walk common prefix
        for (elem_a, elem_b) in self.path.iter().zip(other.path.iter()) {
            if elem_a != elem_b {
                return match (elem_a, elem_b) {
                    // Distinct struct fields are always disjoint in memory!
                    (ProjectionElem::Field(f1), ProjectionElem::Field(f2)) => f1 != f2,
                    // Distinct constant indices are disjoint!
                    (ProjectionElem::ConstIndex(i1), ProjectionElem::ConstIndex(i2)) => i1 != i2,
                    // Complementary halves of the same slice split are disjoint!
                    (
                        ProjectionElem::SliceSplitLeft { split_id: s1 },
                        ProjectionElem::SliceSplitRight { split_id: s2 },
                    )
                    | (
                        ProjectionElem::SliceSplitRight { split_id: s1 },
                        ProjectionElem::SliceSplitLeft { split_id: s2 },
                    ) => s1 == s2,
                    _ => false,
                };
            }
        }
        // If one path is a prefix of the other (e.g. `x` and `x.field`), they OVERLAP!
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Let,   // Shared immutable read
    Inout, // Exclusive mutable read/write
    Sink,  // Ownership consumption (move)
    Set,   // Initialization write
}

#[derive(Clone, Debug)]
pub struct ActiveLoan {
    pub loan_id: u32,
    pub place: Place,
    pub kind: AccessKind,
}

pub struct ExclusivityChecker {
    active_loans: Vec<ActiveLoan>,
    moved_places: Vec<Place>,
    next_loan_id: u32,
}

impl ExclusivityChecker {
    pub fn new() -> Self {
        Self {
            active_loans: Vec::new(),
            moved_places: Vec::new(),
            next_loan_id: 0,
        }
    }

    /// Validates whether `place` can be accessed with `kind` under the Law of Exclusivity.
    pub fn check_and_acquire_loan(
        &mut self,
        place: Place,
        kind: AccessKind,
    ) -> Result<u32, String> {
        // 1. Check if any overlapping place has been moved (`sink`) and not re-initialized (`set`)
        if kind != AccessKind::Set {
            for moved in &self.moved_places {
                if !place.is_disjoint_from(moved) {
                    return Err(format!(
                        "Use of moved or partially moved value at {:?} (moved at {:?})",
                        place, moved
                    ));
                }
            }
        }

        // 2. Check against all currently active loans (Law of Exclusivity)
        for loan in &self.active_loans {
            if !place.is_disjoint_from(&loan.place) {
                match (loan.kind, kind) {
                    // Multiple `let` views of overlapping places are completely legal!
                    (AccessKind::Let, AccessKind::Let) => continue,
                    _ => {
                        return Err(format!(
                            "Law of Exclusivity violation: cannot acquire {:?} access to {:?} \
                             while {:?} access to {:?} is still active",
                            kind, place, loan.kind, loan.place
                        ));
                    }
                }
            }
        }

        // 3. State transition for `Sink` (move) or `Set` (re-initialization)
        match kind {
            AccessKind::Sink => {
                self.moved_places.push(place.clone());
            }
            AccessKind::Set => {
                // Re-initializing `place` clears any prior moved state covered by `place`
                self.moved_places.retain(|m| place.is_disjoint_from(m));
            }
            _ => {}
        }

        let id = self.next_loan_id;
        self.next_loan_id += 1;
        self.active_loans.push(ActiveLoan {
            loan_id: id,
            place,
            kind,
        });
        Ok(id)
    }

    pub fn release_loan(&mut self, loan_id: u32) {
        self.active_loans.retain(|l| l.loan_id != loan_id);
    }
}
```

---

### 8.3 Blueprint 3: `melior` MLIR Lowering for `<decides>`, `<transacts>` & `inout` (`crates/rill-mlir/src/codegen.rs`)

Below is the concrete `melior` code showing how Rill initializes the MLIR context, lowers `<decides>` failable functions into dual-status or branching blocks, and compiles `option { e1 | e2 }` with zero-cost SSA rollback.

```rust
use melior::{
    Context,
    dialect::{DialectRegistry, arith, cf, func, llvm, memref, scf},
    ir::{
        Attribute, Block, Identifier, Location, Module, Region, Type, Value,
        attribute::{IntegerAttribute, StringAttribute, TypeAttribute},
        operation::OperationBuilder,
        r#type::{FunctionType, IntegerType},
    },
    pass::{PassManager, conversion, transform},
    utility::{register_all_dialects, register_all_llvm_translations},
};

pub struct RillMlirBackend {
    context: Context,
}

impl RillMlirBackend {
    pub fn new() -> Self {
        let context = Context::new();
        let registry = DialectRegistry::new();
        register_all_dialects(&registry);
        context.append_dialect_registry(&registry);
        context.load_all_available_dialects();
        register_all_llvm_translations(&context);

        Self { context }
    }

    /// Lowers a failable `<decides>` function `first_positive(x: i64, y: i64) -> i64 <decides>`
    /// Body: `(if (x > 0) { x } else { fail }) | (if (y > 0) { y } else { fail })`
    ///
    /// ABI for semideterministic `<decides>` functions:
    /// Returns two values in registers: `(i1 %succeeded, T %value)`
    pub fn build_failable_choice_module(&self) -> Module<'_> {
        let location = Location::unknown(&self.context);
        let module = Module::new(location);

        let i1_ty: Type = IntegerType::new(&self.context, 1).into();
        let i64_ty: Type = IntegerType::new(&self.context, 64).into();

        // Function signature: `(i64, i64) -> (i1, i64)`
        let fn_type = FunctionType::new(&self.context, &[i64_ty, i64_ty], &[i1_ty, i64_ty]);

        let region = Region::new();
        let entry_block = Block::new(&[(i64_ty, location), (i64_ty, location)]);
        let try_right_block = Block::new(&[]);
        let success_block = Block::new(&[(i64_ty, location)]);
        let fail_block = Block::new(&[]);

        // --- Entry Block: Evaluate Left Arm of `|` (`x > 0`) ---
        let x_val: Value = entry_block.argument(0).unwrap().into();
        let y_val: Value = entry_block.argument(1).unwrap().into();

        let zero = entry_block
            .append_operation(arith::constant(
                &self.context,
                IntegerAttribute::new(i64_ty, 0).into(),
                location,
            ))
            .result(0)
            .unwrap()
            .into();

        let x_gt_zero = entry_block
            .append_operation(arith::cmpi(
                &self.context,
                arith::CmpiPredicate::Sgt,
                x_val,
                zero,
                location,
            ))
            .result(0)
            .unwrap()
            .into();

        // If `x > 0` succeeds, jump to `success_block(x_val)`.
        // If `x > 0` fails (`<decides>`), backtrack to `try_right_block`!
        entry_block.append_operation(cf::cond_br(
            &self.context,
            x_gt_zero,
            &success_block,
            &try_right_block,
            &[x_val],
            &[],
            location,
        ));

        // --- Right Arm Block: Evaluate `y > 0` ---
        let y_gt_zero = try_right_block
            .append_operation(arith::cmpi(
                &self.context,
                arith::CmpiPredicate::Sgt,
                y_val,
                zero,
                location,
            ))
            .result(0)
            .unwrap()
            .into();

        try_right_block.append_operation(cf::cond_br(
            &self.context,
            y_gt_zero,
            &success_block,
            &fail_block,
            &[y_val],
            &[],
            location,
        ));

        // --- Success Block: Return `(true, result)` ---
        let result_val: Value = success_block.argument(0).unwrap().into();
        let true_val = success_block
            .append_operation(arith::constant(
                &self.context,
                IntegerAttribute::new(i1_ty, 1).into(),
                location,
            ))
            .result(0)
            .unwrap()
            .into();
        success_block.append_operation(func::r#return(&[true_val, result_val], location));

        // --- Fail Block: Return `(false, undef/0)` ---
        let false_val = fail_block
            .append_operation(arith::constant(
                &self.context,
                IntegerAttribute::new(i1_ty, 0).into(),
                location,
            ))
            .result(0)
            .unwrap()
            .into();
        fail_block.append_operation(func::r#return(&[false_val, zero], location));

        region.append_block(entry_block);
        region.append_block(try_right_block);
        region.append_block(success_block);
        region.append_block(fail_block);

        let func_op = func::func(
            &self.context,
            StringAttribute::new(&self.context, "first_positive"),
            TypeAttribute::new(fn_type.into()),
            region,
            &[],
            location,
        );

        module.body().append_operation(func_op);
        module
    }

    /// Runs the optimization and LLVM lowering pass pipeline on the generated MLIR module
    pub fn optimize_and_lower_to_llvm(&self, module: &mut Module<'_>) -> Result<(), String> {
        let pm = PassManager::new(&self.context);
        pm.enable_verifier(true);

        // 1. High-level canonicalization, CSE, and SROA (eliminates local transactional allocas)
        pm.add_pass(transform::create_canonicalizer());
        pm.add_pass(transform::create_cse());
        pm.add_pass(transform::create_sroa());

        // 2. Lower structured control flow (`scf`) to branch control flow (`cf`)
        pm.add_pass(conversion::create_scf_to_control_flow());

        // 3. Convert Arith, ControlFlow, MemRef, and Func to LLVM dialect
        pm.add_pass(conversion::create_arith_to_llvm());
        pm.add_pass(conversion::create_control_flow_to_llvm());
        pm.add_pass(conversion::create_finalize_mem_ref_to_llvm());
        pm.add_pass(conversion::create_func_to_llvm());
        pm.add_pass(conversion::create_reconcile_unrealized_casts());

        pm.run(module)
            .map_err(|e| format!("MLIR PassManager failed: {:?}", e))
    }
}
```

---

## 9. Zero-Cost Arity Specialization & Calling Convention ABI

Because Rill supports ML-style space-separated application (`f a b`) and flow pipes (`x . f y`), a naive compiler would allocate heap closures for every intermediate curried step. Rill avoids this completely via **Three-Tier Arity Lowering**:

1. **Syntactic Arity Grouping**:
   In `let add (x: i32, y: i32) = x + y;` or `let add (x: i32) (y: i32) = x + y;`, the HIR records the **natural arity** $N = 2$.
2. **Direct Call Fast-Path (99% of calls)**:
   At any call site `add 10 20` or `10 . add 20`, the parser and HIR flatten the application spine into `Call(add, [10, 20])`. Because the argument count matches the natural arity $N = 2$, the MLIR backend emits a direct multi-argument C-ABI call `call @add(10, 20)`—passing arguments in CPU registers (`rdi`, `rsi`) with **zero heap allocation**.
3. **Partial Application Stack Promotion**:
   If a function is partially applied (`inc := add 1; Map::map xs inc`), `inc` is lowered into a stack-allocated anonymous struct `struct Closure_add_1 { i32 captured_x; }` and `Map::map` is monomorphized over that struct type (identical to Rust's `impl Fn(i32) -> i32`).
   A heap allocation (`<alloc>`) is **only** required if the user explicitly boxes a closure into a dynamic trait object (`(add 1) as dyn (i32 -> i32)`).

---

## 10. Standard Library ("Zero-Host Core") & Self-Hosting Roadmap

Rill's standard library is stratified into two layers inspired by Rust and Zig, with almost all high-level features built in user-space Rill code using traits, projections, effects, and computation builders:

### 10.1 `core` (Zero-Host, Freestanding, No `<io>` or OS dependencies)
* **`core::mem`**: `size`, `alignof`, `swap (a: inout t, b: inout t)`, `replace`, `MaybeUninit`.
* **`core::slice`**: `Slice a` view type, `split_at` projection, SIMD chunk iterators.
* **`core::option` & `core::result`**: `Option a`, `Result v e`, `Try` trait backing `let?` and `e?`.
* **`core::ops`**: `Arith`, `Eq`, `Ord`, `Index` (using `project`), `Range`.
* **`core::alloc`**: `Allocator` trait and the `<alloc>` effect handler interface (`StackArena`, `ScratchArena`, `PageAllocator`).
* **`core::simd` & `core::tensor`**: Shaped array views (`shapeof`, `reshape`) backed by MLIR `linalg` / `vector` intrinsics.

### 10.2 `std` (OS Capabilities via Explicit Effects)
* **`std::io`, `std::fs`, `std::net`, `std::time`**:
  Implemented via direct Linux `io_uring` / syscalls (or platform APIs on macOS/Windows) without requiring `libc` on Linux targets.
* **`std::task`**:
  Work-stealing, stackless coroutine executor implementing `sync`, `race`, `rush`, `branch`, and `spawn` for the `<suspends>` effect.
* **`std::txn`**:
  Thread-local `UndoTrail` and `ScratchArena` implementation backing the `<transacts>` effect.

---

### 10.3 Phased Implementation Roadmap (Session Execution Plan)

| Milestone | Scope & Deliverables | Key Verification Test |
| --- | --- | --- |
| **M0: Foundation & Parser** | `rill-span`, `rill-lexer`, `rill-syntax`, `rill-parser`, `rill-ast`. Complete EBNF parser with adjacency rule (`xs[i]` vs `xs [i]`), `.` flow operator, and builder desugaring. | Parse all §8 Rill grammar examples into lossless CST & desugared AST with 100% round-trip accuracy. |
| **M1: HM(X) & Effect Rows** | `rill-hir`, `rill-ty`. Bidirectional type inference, Rémy effect row unification (`<io \| decides \| e>`), trait constraints (`where a is Show`), and arity uncurrying. | Infer principal types and effect rows for polymorphic `Map::map`, `option { ... }`, and `e1 \| e2`. |
| **M2: Hylo Borrow Checker** | `rill-mir`, `rill-borrow`. Lower Typed HIR to Place-CFG MIR. Implement `ExclusivityChecker`, struct field splitting, `split_at` slice proofs, and `project`/`yield inout` checking. | Reject overlapping `inout` borrows; accept simultaneous field mutations and virtual `PackedColor` projections. |
| **M3: `melior` Core & `<decides>`** | `rill-mlir` + `rill-driver`. Lower functions, records, ADTs, and `<decides>` / `\|` / `option` / `all` to MLIR (`func`, `cf`, `scf`, `llvm`) and emit native static binaries via `lld`. | Compile and run native freestanding binary computing backtracking N-Queens & matrix kernels at C speed. |
| **M4: `<transacts>` & `<alloc>`** | Implement SSA promotion for local speculative `var`s, thread-local `UndoTrail` for `inout` heap rollbacks, and scoped `<alloc>` arena handlers. | Speculative mutation inside `if (mutate[inout state], fail)` cleanly restores `state` on the `else` branch. |
| **M5: `<suspends>` & Builders** | Lower `sync`, `race`, `rush`, `branch`, `spawn` to MLIR `async`/LLVM coroutines. Add `simd { ... }` and `seq { ... }` computation builders in `core`. | Run structured concurrency test suite and verify SIMD auto-vectorization assembly output. |
| **M6: Self-Hosting Bootstrap** | Port `rill-lexer`, `rill-parser`, `rill-ty`, and `rill-borrow` into Rill itself, using pattern matching, `<decides>` parser combinators, and `<alloc>` arenas. | `rillc-stage1` compiles `rillc-stage2` into an identical static native binary. |

---

## 11. End-to-End Showcase: Systems Rill in Action

The following self-contained program demonstrates how Verse's `<decides>` and `<transacts>`, Hylo's `inout` projections and slice splitting, F#'s computation builders, and C-level zero-allocation performance come together seamlessly in Rill:

```rill
use core::slice;
use core::alloc::StackArena;

# 1. Record with mutable fields and deterministic RAII deinit
type Account = record {
    id: u64;
    var balance: i64;
};

# 2. First-class `inout` Projection (no `'a` lifetimes needed!)
project balance_mut (acc: inout Account) -> inout i64 <computes> = {
    yield inout acc.balance;
};

# 3. Transactional & Failable Systems Function (`<decides | transacts>`)
# If `amount > from.balance` or `to.balance` overflows, `<decides>` fails
# and `<transacts>` automatically rolls back `from.balance`!
let transfer (from: inout Account, to: inout Account, amount: i64) -> () <decides | transacts> = {
    amount > 0;
    set (from . balance_mut) = from.balance - amount;
    # If `from.balance` went negative, fail!
    # The prior mutation to `from.balance` is rolled back automatically!
    from.balance >= 0;
    set (to . balance_mut) = to.balance + amount;
};

# 4. In-place Quicksort using Hylo-style disjoint slice splitting (`<computes>`)
let quicksort (xs: inout Slice i64) -> () <computes> = {
    if (size xs > 1) {
        pivot_idx := partition (inout xs);
        if (pair := xs . split_at[pivot_idx]) {
            # `pair.0` and `pair.1` are statically proven disjoint by `rill-borrow`!
            quicksort (inout pair.0);
            quicksort (inout pair.1);
        } else { () }
    } else { () }
};

# 5. Main entrypoint combining Arena allocation, Speculative Choice, and Structured Concurrency
let main () -> i32 <io | suspends> = {
    var alice = Account 1u64 500i64;
    var bob   = Account 2u64 100i64;

    # Try transferring 800 (fails & rolls back!), fallback via `|` to transferring 200 (succeeds!)
    _ := option {
        transfer[inout alice, inout bob, 800]
        | transfer[inout alice, inout bob, 200]
    };

    # `alice.balance` is now 300, `bob.balance` is 300.
    io::println ("Alice: " ++ (alice.balance as String));

    # Stack-allocated 16KB arena discharges `<alloc>` completely on the stack!
    var scratch: [u8; 16384] = comptime { zeroed () };
    StackArena::run (inout scratch) {
        # Nondeterministic comprehension (`all`) allocated inside `scratch` arena
        pairs := all {
            x := 1..5;
            y := 1..x;
            when (x + y = 5);
            (x, y)
        };
        io::println ("Valid pairs count: " ++ (size pairs as String))
    };

    0i32
};
```
---

## 12. Implementation notes recorded during development

Status and decisions taken while building the M0–M4 slices, so later work starts
from measured reality rather than the original sketch.

### Implemented (Rust compiler, C bootstrap backend)

Lossless CST + resilient parser; name resolution and HIR with `Res`/`LocalId`/
`ExprId` tables; HM-style type inference with deferred numeric literal types;
records, enums (tagged unions), views, tuples, fixed arrays, module-level
statics; `for`, `while`/`break`, `if`/`else`, `match` with guards and payload
binding; non-capturing closures; `inout` as C pointers; `<alloc>` growable byte
buffers with the effect enforced (`<computes>` is the empty row and cannot
allocate); text/byte primitives; `io::println`, `io::read_file`,
`io::map_file`, `io::file_size`. Backend: typed HIR -> C -> `cc -O2` (native) or
`zig cc -target <arch>-linux-musl -static` (static ELF). `rillc build`,
`--dump=cst|ast|hir`, `--emit-c`.

Benchmarks (`bench/`) run the *same algorithm* in Rill, Rust and Zig and assert
identical checksums: v1 byte scan 0.12/0.13/0.13s, v2 structured parse 0.03/0.03/
0.02s, v3 577 KB with a 262 k-node pool 0.50/0.51/0.50s.

### Known blocker

Traits: members parse, lower, resolve as callable names, impls are emitted under
mangled names and a dispatch table routes trait-method calls by argument type.
Typing an impl member's parameter (`show (p: Point)`) still reports
`unknown type Point` even though the lookup that would resolve it succeeds
(debug output: `named_items=[3] nominals=[3]`), so the failing check is inside
the same fallback arm in `rill_ty::Checker::path_ty`. Next diagnostic: print the
reference's span alongside `type_ids` in that arm to find which occurrence fails.

### Design note: currying / partial application (next item)

`add 1` where `add (x: i32, y: i32)` must become a closure, and the clean way to
do it in the current backend is:

1. **Represent callables as pairs.** Today a function-typed value lowers to a
   bare C function pointer (lambdas rely on this). Closures need `{ fn, env }`,
   so the representation of `Ty::Fn` values has to become a struct
   `{ ret (*fn)(args..., void *env); void *env; }` — or a per-shape struct —
   which means updating: lambda emission, local/param declarations of function
   type (`c_decl`), and every call site (a call through a value becomes
   `v.fn(args, v.env)`).
2. **Partial application at a call site.** When fewer arguments are supplied
   than the callee's parameter count, emit a static trampoline per (callee,
   arity) pair and a stack-allocated environment struct holding the supplied
   arguments; the value is `{ trampoline, &env }`.
3. **Checker.** `apply` already has both counts: fewer arguments must unify the
   supplied ones with the leading parameters and return the *remaining* arity
   as the result type, instead of today's arity error. More arguments stay an
   error.
4. **Escape rule.** A partial application may not outlive its frame; the borrow
   pass (M2) owns that check, so until then the environment is stack-allocated
   and escaping closures are reported rather than miscompiled.

Coupled work: the same pair representation is what `dyn` fat pointers need
(`{ data, vtable }`), so doing currying first means `dyn` reuses the call shape
rather than inventing a second one.

### Directives for the remaining work (in priority order)

1. **FFI — done.** `extern "c"` declarations keep their declared C names (no
   prefix), `&raw`/`&raw mut T` are real pointer types in the checker, and
   `rillc build --link <lib>` passes `-l<lib>` to the toolchain.
   `examples/ffi.rill` calls libc `puts`, `strlen`, `malloc` and `free` (the
   latter two through `&raw mut u8`) and is verified by a driver test.
2. **Arenas — done (first cut).** `arena_new`, `arena_mark`, `arena_reset`,
   `arena_used`, `arena_alloc` on a builtin `Arena` nominal: a bump allocator
   allocated once at the top scope, with marks as the layers (mark -> allocate
   -> reset frees the layer; 16-byte aligned blocks; NULL when full). Every call
   carries `<alloc>`, so `<computes>` code cannot allocate.
   `examples/arena.rill` runs two layers and is verified by a driver test.
   Still to come for the full directive: a `StackArena::run`-style *scope*
   handler that binds the arena for the lexical context implicitly.
3. **Projections — first form done.** A `project` whose body is exactly
   `{ yield inout x.field }` is a *place projection*: the checker types the body
   by the yielded place and the codegen inlines uses as that field, so
   `x . proj` reads it and `set (x . proj) = v` writes it, with no getter/setter
   pair and no copy. `examples/projections.rill` deposits twice through
   `balance_mut` and prints `balance 142`; a driver test asserts stdout and the
   exit code. Still to come: computed bodies (`yield` of a temporary that is
   written back on release) and `.`-method calls taking parameters.
4. **`<decides>` / `<transacts>`** — success/fail edges, `|` with undo-trail
   rollback, then `option`/`one`/`all` handlers.
5. **Currying / partial application — first cut done.** `add 1` fixes the
   leading parameter and yields a function of the rest. With constant captures
   the result is an ordinary function pointer (generated per (callee, arity)),
   so it can be stored in a local, passed to another function and called through
   that parameter. `examples/currying.rill` verifies both, plus passing the
   result to a function. Capturing a *local* is reported with a note pointing at
   the closure representation (see the design note above); that and escaping
   closures are the remaining half.
6. **SIMD** — `[T; N]` lane operations and a `simd { }` builder lowered to loops
   that clang vectorises; shaped tensors later.
7. **`dyn`** — trait objects as fat pointers `(data, vtable)` with dispatch; the
   current `dyn` -> `Text` placeholder is to be deleted.
8. **`<suspends>` / structured concurrency** — the language provides only the
   scaffolding: stackless coroutines/generators that lower to state machines,
   plus green threads. Executors, thread pools, and the whole `Io` layer live in
   the standard library (or external libraries), not in the compiler.
   `sync`/`race`/`rush`/`branch`/`spawn` are the surface; cancellation and
   deadlines belong to the runtime. The Io/async context is *bound at the top
   scope* and reaches the entire lexical context in layers — the same shape as
   `StackArena::run` for allocation — and is deliberately Zig-inspired while
   keeping Rill's explicit, effect-tracked style.

MLIR (`rill-mlir`) and self-hosting remain deferred; the C backend is the
bootstrap that the typed HIR is lowered to meanwhile.
