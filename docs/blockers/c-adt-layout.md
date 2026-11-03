# C emission for algebraic values

The C backend emits closed programs with concrete scalar, static-string and
fixed byte-buffer value parameters, concrete finite Copy enums and records from a
provisional typed IR using per-type C locals. This includes nested/literal Copy
patterns and concrete instances of generic enum types and functions. Reachable
function instances are keyed by source identity and complete concrete parameter
and result types, including phantom arguments. Recursive instances deduplicate
before their bodies are expanded. The interpreter uses this same verified
representation for supported programs. Owner programs also use this path,
including concrete finite
enum instances with typed tagged unions. Owners never fall back to the copying AST
emitter. For other supported programs the backend falls back to
a uniform `rl_value`. Its `RL_CAP` is the largest inline value needed by any
reachable specialization, so even an integer temporary in that path carries
that program-wide capacity. String nodes
also carry a pointer and length; fixed byte buffers occupy nodes in this same
representation. That fallback does not allocate heap memory, but large
values increase copying and stack use for every value. The emitter rejects an
estimated frame over 1 MiB (`E-C-STACK`); this estimate is not a stack-safety
guarantee. See [stack safety](stack-safety.md).

The present analysis records inferred types against AST addresses and computes a
maximum across reachable function instantiations. The provisional typed IR
covers concrete scalar, static-string and fixed byte-buffer function instances,
hosted owners, finite enums, calls, bindings, mutation, branches and loops.
Copy matching evaluates one snapshot, tests patterns in source order and binds
fields only after every enclosing tag matches. Its independent verifier checks
pattern types, binding identities, scope and pattern-matrix exhaustiveness.
Consuming enum matching transfers owner payloads and inserts variant-aware
cleanup. Nullary enums have tag-only layouts; no empty C aggregates are emitted. Enum layouts and frame estimates are bounded before
C compilation; cleanup passes aggregates by address rather than copying them.
Finite Copy records use tag-free structs, with a dummy byte for empty records.
Construction retains written evaluation order while storage uses declaration
order. Field paths independently check root/nominal identities, indices, scope,
mutability and active root/field loans, retaining the source limit of 16 steps.
Assignment updates the original root after its right-hand side finishes,
preserving sibling or prefix replacements. Existing `u8x16` values use a compact
16-byte struct and independently checked operations on the same typed path,
including inside restricted owner programs. Foreign-value programs still use
the checked AST C backend; raw C values require native execution. Owner programs
reject foreign forms rather than falling back to copying execution.
Copy root and field loans use explicit typed shared/exclusive operands
and materialized read temporaries. The IR independently checks argument-order
conflicts and forwarding, including prefix overlap and disjoint siblings; C
passes typed pointers without `restrict` promises. Interpreter field handles
retain the original cell plus a composed path, never a Rust borrow across calls.
Generic functions over the supported types now share typed execution;
the worklist caps concrete source-function instances at 1024 and type nesting
at 128. Unused declarations remain source-checked but do not consume instance
or layout slots. Eligible immutable local schemes now use bounded capture-and-shape elaboration:
concrete leaves are evaluated once into private typed capture records and each
whole-value/field use materializes its exact closed type. Unused shape/phantom
parameters need no invented concrete layout. Capture layouts have explicit
compiler-generated origins and share no storage identities across bindings.
Unresolved real uses and weak monotypes still require a representation or an
explicit rejection. See the [local-value decision](../decisions/0004-local-value-shapes.md).
The typed pattern verifier caps nesting at 128 and matrix work at 100,000 units,
and reports a diagnostic rather than accepting unverified coverage. The source
parser's earlier nesting limits still apply. `interpreter::run_reference` keeps
the independent checked AST oracle available for Copy differential tests and
explicitly rejects owner programs. Neither
path yet provides the full checked MIR required by `PLAN.md` section 16.

## Proposed lowering

1. Introduce stable IDs for bindings, expressions, functions and types. Retain
   source locations and the instantiated type at each use, including the
   substitution chosen for a generic call or constructor. Keep parameter modes
   (`value`, `read`, `inout`) with function signatures and call operands.
   Do not use pointers into a particular AST as lasting IR identity.
2. Lower checked bodies to a typed, structured control-flow IR. Give locals and
   compiler temporaries explicit types and distinguish reading/copying a value,
   initializing a local, replacing a local, taking a shared or exclusive loan,
   ending a loan, and calling a function. Calls and constructor fields must
   record evaluation order. Branches, loops, matches and their scopes must be
   explicit. Move and cleanup verification covers hosted owners and finite
   enums; general initialization, drop flags and destructor effects remain
   necessary before calling the full plan's MIR verified.
   Revalidate types, places, parameter modes and loan intervals at
   this boundary; reject unsupported operations rather than filling values in.
3. Starting from `main`, build a worklist of concrete function instances keyed
   by function identity and the full type substitution. Substitute throughout
   each body, discover further instances at calls, and deduplicate recursive
   instances. Keep the existing limits on specialization count and nesting
   until there is a replacement resource policy. A polymorphic local needs a
   distinct concrete instantiation at each use, or an explicit diagnostic if
   the current lowering cannot do that safely. Reject unresolved variables and
   unbounded specialization; do not guess a type. Foreign calls retain their
   declared C signatures instead of being specialized as Rill functions.
4. Intern each closed Rill type into a layout identity. Emit C declarations in
   dependency order: `int32_t` for `i32`, a specified boolean representation,
   a nonempty unit representation for value positions, and a pointer plus byte
   length for static UTF-8 strings. Emit `Bytes<N>` as a value containing `N`
   inline bytes, with a legal C representation for `N = 0`; updates produce a
   copy, bounds and byte-range errors trap. `cstr` remains a separate direct
   foreign-argument contract and is not an owned Rill string. No representation
   should silently allocate.
5. For each closed nominal ADT, emit a tag and a C union of named, typed
   variant payload structs. Nullary-only types need no empty C struct or union.
   Instantiate generic field types first, reject infinite inline layout, and
   keep nominal type identities distinct even when layouts happen to match.
   Construct fields in source order. Match code checks a tag before projecting
   a union member and materializes addressable bindings in the proper scope.
   Do not expose these private layouts as a stable C ABI; foreign aggregate
   shims and audited representations remain separate work.
6. Emit typed locals and calls from the concrete IR. Value parameters receive
   values; shared roots use `const T *` and exclusive roots use `T *`, with
   reborrows and shadowed locals retaining distinct binding identities. First
   evaluate arguments left to right into typed locals or addresses, then call
   C, whose argument evaluation order is otherwise unspecified. Retain checked
   integer operations, trap behavior, string byte lengths and the current
   function-depth policy. Never claim alias or overflow facts to C that the
   checked IR has not established.

## Incremental milestones and evidence

- **Typed boundary.** Extend the provisional primitive typed IR and verifier to
  the current subset, while the existing evaluator and C emitter remain the
  reference for constructs not yet lowered.
  Test binding identity under shadowing, types at polymorphic use sites, modes,
  loan intervals, and rejection of malformed IR. Record unsupported features.
- **Concrete instances.** The reachable-instance worklist now covers multiple
  instances, mutually recursive groups, monomorphic recursion, return-only and
  phantom variables, instance limits and generics inside ADTs. Its exact source
  instance set and unresolved-local failures are tested against the independent
  old layout analyzer. Eligible nonexpansive local value shapes now support independent concrete uses;
  unconstrained actual uses and other typed-path gaps remain. The uniform
  analyzer is still used for unsupported typed-path programs.
- **Typed C behind a switch.** Emit primitives, static strings, fixed buffers,
  ADTs and all existing control flow, including pattern bindings. Add tests
  for nested and nullary constructors, embedded NUL, UTF-8 bytes, buffer zero
  length and bounds, overflow, and left-to-right traps at `-O0` and `-O2`.
  Verify each compiled case against the reference evaluator and existing C
  output. Reject any unsupported typed operation; do not mix typed and uniform
  values in one function without a separately specified conversion.
- **Loans and ABI.** Cover repeated shared reads, exclusive conflicts,
  forwarding and reborrowing, shadowing, pattern-binding addresses and the
  scalar/C-string foreign ABI. Test linking against a C fixture on the target.
  Keep current conservative loan checks until IR-level checks replace them.
- **One checked path and transition.** Move the evaluator to the checked IR,
  then run the complete acceptance suite against both implementations. Make
  typed C the default only when every currently supported construct works and
  its failures match the documented contract. Remove the uniform backend and
  its capacity limit only after the transition is covered; retain a conservative
  stack policy until actual target frame and ABI costs have been measured and
  bounded. Typed layouts reduce the avoidable overhead but do not themselves
  prove stack safety, ownership soundness or C ABI compatibility.
