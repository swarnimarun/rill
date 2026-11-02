# Selected C import investigation

Status: preliminary design, paused before implementation. No importer command,
new Cargo dependency or unsafe libclang binding is part of `v1`.

The investigation recommends libclang's stable C API over parsing Clang's JSON
AST dump. The final decision and dependency version remain for the resumed
session. See [the continuation plan](../CONTINUATION_PLAN.md).

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
