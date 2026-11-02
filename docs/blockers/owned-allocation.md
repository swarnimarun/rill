# Hosted owned allocation

Rill supports a restricted hosted `SystemBoxI32`, an owned allocation containing
one `i32`. `system_box_i32_abort(value)` explicitly selects the system allocator
and an aborting allocation-failure policy. A live box can be read with
`box_read(owner)` and updated with `box_write(inout owner, value)`. The latter
requires a mutable binding. `move owner` transfers ownership to a new local;
the old binding is then unavailable and cannot be reinitialized. Ordinary
copying and implicit owner moves are rejected.

`system_bytes_zeroed_abort(length)` creates a distinct fixed-length owned byte
buffer with the hosted system allocator. Its nonnegative runtime `i32` length
is checked for target-size representability; negative or unsupported sizes and
allocation failure trap. Every logical byte starts at zero. A zero-length
buffer is valid and still owns one allocation. `owned_bytes_len(owner)` and
`owned_bytes_get(owner, index)` read it; `owned_bytes_set(inout owner, index,
value)` writes it. Invalid indices or values outside 0 through 255 trap.
It follows the same move, borrow, scope and cleanup restrictions as the box.
It does not resize, expose a raw pointer, or provide a borrowed slice or string.

`std.alloc.box_i32(value)` and `std.alloc.bytes_zeroed(length)` add a
recoverable alternative. Import `std.alloc` and `std.result` explicitly; these
functions return ordinary affine `std.result.Result` values with a success
owner and `std.alloc.AllocError`. Errors are `InvalidLength`, `SizeOverflow`
and `OutOfMemory`. Invalid lengths and target-size overflow return before any
allocator attempt. Null allocation returns an error without creating an owner.
The argument evaluates once; zero-byte success still owns one allocation.
Consume results with `match move`, using the canonical bundled constructors.
The [library API](../../stdlib/README.md) shows the exact types and syntax.
Errors return normally, so live owners retain normal cleanup obligations;
the `_abort` operations and unrelated runtime traps still promise no recovery.

[The owned-box examples](../../examples/owned/README.md) run through both the
interpreter and generated C. Allocation and local-to-local moves must be
direct local initializers. An owner can also be transferred to an inferred
owner value parameter with a direct `move owner` argument; the callee takes
the cleanup obligation and may forward it to another function. All functions
in an owner program must be monomorphic and form an acyclic call graph. A
function can borrow an owner root with `read` or `inout`; borrowed non-owner
user-function parameters remain unsupported in owner programs. A function may
return an owned local or value parameter with a tail `move owner`, or forward
an owner-returning call as its tail expression. Returned owners can initialize
locals or enum fields, including through nested lexical lets and branches.
A bare allocation as a function body, an implicit
owner copy, return of a borrowed owner and an owner-valued `main` remain rejected.
Finite nominal enums may contain owners, including nested concrete generic
instances. Constructors consume explicit moves and keep their fields inline.
`match move value` consumes an enum and transfers its fields to arm bindings.
Unconsumed fields, including wildcards, are destroyed at normal arm exit in
reverse field order, after body locals. A whole-value binder receives the
entire enum; a whole-value wildcard destroys it at arm exit. Nested field
patterns require separate matches. See [the Result example](../../examples/owned/result.rill).

An `if` or `match` may return an owner. Its arms must agree about which
enclosing owners remain live; moving the same outer owner on every arm is
allowed. Loops and short-circuit operands must preserve enclosing ownership
state. Owners allocated within a branch or iteration may move between locals
or transfer to a consuming call. Normal cleanup needs no runtime drop flag
under these rules. Differing branch states, owner reassignment, owned records,
borrowed enum matching, foreign declarations and vectors remain unsupported.
See [borrowed owners](owner-borrows.md). This is not yet general affine
ownership or a stable allocator API.

A normal scope exit drops each still-live owner once, in reverse declaration
order. A callee's local owners are dropped before its owned parameters, and
live parameters are dropped in reverse parameter order. A returned owner is
excluded from callee cleanup; its caller receives the cleanup obligation.
Live callee locals are destroyed before the caller resumes. A move transfers the
drop obligation and the moved-from binding is not dropped. Allocation failure
and other traps terminate generated executables
with a nonzero status. The interpreter API returns a runtime error; the CLI
reports it and exits. Neither path promises Rill-level cleanup after a trap,
process termination, stack exhaustion or an unsafe foreign violation.

The named builtin chooses the system allocator and abort policy; there is no
separate allocator-capability configuration, general hosted/freestanding profile
or effect system yet. Do not infer capability isolation, a checked `noalloc`
contract or custom allocators from these operations. The interpreter models allocations internally; C output calls the
host system allocator. The explicit operation does not make heap use invisible.

## Implementation and evidence

The checker rejects unsupported owner escapes and ordinary copies. Owner
programs lower to a restricted typed representation with explicit owner
initialization, move, read, write and cleanup metadata; the typed verifier
checks the ownership state before either interpreter execution or C emission.
The generic copying AST backend is not a fallback for owner programs. This
representation is provisional and is not the verified MIR in `PLAN.md`.

`tests/owned_box.rs` checks invalid use after move, implicit copies, immutable
writes and unsupported escapes, and compares interpreter/C results. It
intercepts the generated C allocator to check allocation and cleanup order and
inject failures at both allocation sites at `-O0` and `-O2`.
`tests/owner_calls.rs` checks parameter transfer, forwarding, drop order and
argument traps. `tests/returned_owner.rs` checks returned boxes and byte buffers,
callee-to-caller cleanup transfer, unused returned owners, zero-length buffers,
mutation after return and rejection of unsupported escapes and joins. Typed runtime tests also exercise injected interpreter
allocation failure and scoped cleanup. `tests/owned_bytes.rs` checks length,
bounds, zeroing even when the allocator returns poisoned memory, zero-length
allocation, OOM, moves, borrows and C parity.

`tests/owner_adt.rs` checks success/error variants, nested multi-field enums,
branch ownership joins, consuming patterns, unused payload cleanup and failure
traces through the interpreter and C at `-O0` and `-O2`.
`tests/owner_adt_module.rs` transfers an owned byte buffer inside the bundled
`std.result.Result` across a factory and forwarding module. Enums themselves
cause no allocator calls. C cleanup dispatches on the active variant; the
typed verifier checks field types, tags, ownership classifications and cleanup.

`tests/fallible_allocation.rs` replays failures at each allocation in a mixed
box/byte program through generated C at multiple optimization levels.
Interpreter tests check exact successful-owner and release traces, invalid
lengths, zero sizes, recovery followed by later allocation and canonical typed
contracts. `tests/fallible_allocation_module.rs` checks bundled nominal identity
and ownership across modules. This evidence covers the restricted subset.

An optimized build must still attempt the explicit allocation, because its
failure is observable. Apple clang 17 eliminated a direct `malloc`/`free` pair
and its failure branch at `-O2` in a simple owner program. Generated C now calls
through volatile allocator and release function-pointer slots so the compiler
cannot treat those calls as a known removable pair. This adds indirect calls and
is an implementation measure, not a portability proof for every C compiler.

`tests/owner_optimization.rs` compiles unchanged generated C at `-O0`, `-O2`
and `-O3`, and also `-O3 -flto` when the compiler supports LTO. It checks that
linked output retains `malloc` and `free`, then injects deterministic allocation
failure through the actual slots in a test harness without replacing those
names in the generated source. On a Darwin arm64 host with Apple clang 17.0.0
(`clang-1700.6.4.2`), all four configurations passed; inspected `-O2` assembly
and `-O3 -flto` executable code retained both indirect calls and a null branch.
The `gcc` command on this host invokes Apple clang, so independent GCC was not
tested. Other compiler versions, targets, link-time configurations and external
sanitizers need separate verification. Symbol retention alone does not prove
that calls execute; the runtime injection test covers that behavior for the
configurations it runs.

On the same host, Zig 0.17.0 `zig cc -std=c11 -O2` built the owned-box example;
the executable printed `42` and `nm -u` showed `_malloc` and `_free`. That
check did not include allocator failure injection or assembly inspection. A
`zig cc -O3 -flto` attempt failed with `LTO requires using LLD`, so Zig LTO
behavior has not been verified.

## Remaining work

- Define allocator authority as a capability distinct from `alloc`/`free`
  effects and operational resource summaries. Add explicit hosted and
  freestanding profile behavior, transitive effects and checked allocation
  policies; unknown foreign effects must not silently become pure.
- Add explicit allocator instance identity and provenance, growable buffers
  and owned strings beyond the current recoverable system allocator. Extend
  checked sizing, initialization and partial cleanup to those containers.
- Generalize `Copy`/move obligations to calls, generic values, records and ADTs.
  Add safe by-value transfer and borrowed access without treating user-visible
  resource duplication or reference counting as implicit. The existing
  record-field copy-in/writeback path is restricted to the Copy subset.
- Implement initialization trees, partial moves, assignment/replace, drop
  flags, normal error exits, branch joins and loop fixed points. Add explicit
  cleanup edges and checked destructor effects. Do not promise cleanup after an
  abort or across unsupported unwinding/cancellation.
- Extend the shared checked representation across the whole language and
  preserve ownership through optimization, modules and every backend. Verify
  allocator traces and leak/double-free behavior under supported target
  compilers and sanitizers; absence of one class of failure does not prove the
  full language sound.

These are P2/P3 prerequisites. One local box does not complete either phase.
