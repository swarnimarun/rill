# Selected C import investigation

Status: the historical checkpoint investigation is retained below; the resumed
implementation decision appears at the end. Selected import now exists behind
the optional `c-import` feature.

The investigation recommends libclang's stable C API over parsing Clang's JSON
AST dump. The resumed session chose this design and pinned the dependency
version below. See [the continuation plan](../CONTINUATION_PLAN.md).

## Candidate designs

Clang's JSON subprocess would isolate crashes and allow process timeouts. It
would need a reviewed JSON parser dependency and tested Clang version range.
The local dump exposed a function's return type inside a declarator string,
which would require additional semantic probes or a C declarator parser.
There was no cached JSON dependency available during the investigation.

libclang exposes structural function and type queries, including canonical
types, arguments, return type, calling convention and target size. Its API
avoids interpreting a function signature string. Prefer a reviewed runtime
loader such as `clang-sys` over handwritten platform loading and ABI layouts.
That dependency was not cached either. Library discovery, diagnostics,
resource headers and FFI cleanup need explicit implementation and tests.
See the [official tutorial](https://clang.llvm.org/docs/LibClang.html) and
[type-query reference](https://clang.llvm.org/doxygen/group__CINDEX__TYPES.html).

## Verified feasibility probe

A temporary C fixture used an unsigned-long typedef, a typedef for an opaque
record, a pointer-returning function and an object macro `(1u << 3)`. On the
checkpoint's Apple Clang 17 installation, a libclang probe preserved the opaque
typedef spelling, exposed the canonical unsigned-long type and eight-byte size,
reported the ordinary C calling convention, and resolved a synthetic
`__typeof__(FLAG)` variable to four-byte unsigned int. This is local feasibility
evidence; it does not establish portability or a supported importer API.

## Constraints for implementation

Generate ordinary exported extern modules from explicitly requested symbols.
Use the existing target C compiler checks as the final validation step. Preserve
preprocessing flags and target configuration; importing on the host and building
for a different ABI must not silently succeed.

The current pointer syntax can represent a record typedef such as
`ptr<SDL_Window>`, but not a struct-tag-only name or nested pointer. Reject those
forms until they have a deliberate representation. Preserve opaque identity and
constness. Size equality alone cannot establish scalar call compatibility.

Object macros need semantic type and constant-expression probes after
preprocessing. Function-like macros are not foreign functions. Current integer
constant checks use representability rather than exact C integer type, so the
importer must define whether it strengthens that contract. Do not claim generated
bindings freeze a target ABI merely because a constant fits.

Keep compiler handles scoped to their translation unit and dispose resources
before unloading the library. Rust callbacks must not unwind through C. An
in-process libclang call cannot promise a safely enforceable timeout; use worker
process isolation if a hard execution limit is required.

Validate every selection before publishing output. Reject unsupported or
ambiguous declarations with the requested symbol and C source location. Test
fixture imports independently of SDL, then generate the selected SDL3 binding
module and run the existing real-library integration test.

## Resumed implementation decision (3 November 2026)

The first increment uses the stable libclang C API through `clang-sys` 1.8.1
with runtime discovery, behind the optional `c-import` Cargo feature. A cloud
probe found libclang 19 and GCC; the Clang executable and Rust toolchain were
provisioned separately. Default builds do not load or link libclang.
Clang JSON was rejected because its prototype strings would require a second
C declarator parser. No handwritten ABI declarations or dynamic loader are
introduced. Importing requires libclang 12 or newer and a compatible C11
compiler; resources remain scoped to their translation unit.

The bounded command selects named functions and integer/bool constants and
emits an ordinary exported binding module. It compares canonical frontend
types with target-configured stdint probes, not just widths. Pointer support
preserves scalar or record-typedef identity and pointee constness. Unsupported
signatures fail with the selection and source location. Object macros are
preprocessed and typed by Clang; only frontend-foldable integer/bool constants
are admitted, never function-like macros or runtime variables. The final
selected C compiler checks exact constant types and values as well as the existing
function ABI assertions before any output is published.

Package flags and explicit C arguments are passed, in order, to both frontends.
Generated modules do not persist arbitrary compiler flags or freeze an ABI:
subsequent builds must use the same target, sysroot, defines and include paths.
This is the existing native-module build contract, not an importer cache or a
cross-target package format. The first increment's libclang API is in-process
without a hard timeout. The subsequent CLI worker boundary below isolates it;
full import cache identities remain later work.

## CLI isolation continuation (3 November 2026)

Keep structural libclang extraction and exact target checks unchanged, but run
the complete CLI generation operation in a private same-executable subprocess.
The library `generate` API remains explicitly in-process. Supervise package
resolution and target C validation as well as libclang, so moving only parsing
out of process does not leave the same CLI hanging in another tool.

Use a small versioned, length-prefixed binary protocol with 1 MiB request/module
bounds and raw Unix bytes for paths. It carries explicit selections, ordered
flags, the selected compiler and resource path. No shell command construction,
handwritten libclang ABI/loader, JSON AST or additional parser dependency is
needed. The already-locked `libc` crate supplies Unix process/pipe interfaces as
an optional direct dependency; default builds still require neither it nor
libclang. The parent rechecks framing, UTF-8, expected dependencies/symbols and
the shallow generated declaration shape before normal Rill module validation.
Reject arbitrary functions/types before checking, because the general checker
still has unbounded declaration-dependency traversals.

Nonblocking pipes bound work per supervision turn as well as retained bytes.
The parent polls an unreaped group leader with `waitid(WNOWAIT)`, kills the
owned process group before reaping, and cleans a private temporary directory.
The leader's reserved PID avoids a reused-group race. A signal guard handles
SIGINT/TERM cancellation and clears inherited SIGCHLD auto-reaping while the
worker is live. Malformed responses or complete output followed by failure do
not authorize publication. The worker is the current executable, never a PATH
or environment-selected replacement. Fault-injection workers exist only in
internal tests.

The boundary currently requires Unix. It is not a memory or security sandbox,
and deliberately escaping descendants are not contained. A non-Unix CLI must
fail explicitly until equivalent process-tree supervision is implemented.
See [the precise limits](../c-interop.md#cli-worker-isolation-and-limits) and
[verification evidence](../verification/2026-11-03-import-worker.md).
