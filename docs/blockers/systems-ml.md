# Systems ML follow-on gates

The runnable examples expose three compiler and runtime requirements.

| Evidence in this branch | Required next increment | Acceptance |
| --- | --- | --- |
| The [modal editor](../../examples/modal-editor/main.rill) shifts bytes within a fixed 256-byte value. A full document refuses insertion. The [engine](../../examples/scene-engine/scene.rill) uses fixed byte storage for 16 nodes. | Add a caller-selected allocator and a growable owned byte vector. Specify allocation failure and reallocation with live loans. Then implement UTF-8 validation and grapheme navigation above byte storage. | Fail allocation after a nonempty edit and retain the exact prior document. Reject a live slice across growth. Compare drop traces and bytes in the interpreter and optimized C. |
| The [database](../../examples/embedded-db/README.md) validates exact replay using a single-writer log. A file length inconsistent with the row format currently rejects at open. | Define transaction boundaries, crash recovery, index reconstruction, and snapshot/cursor compatibility before concurrent writers or background compaction. | Kill the writer after each write/flush boundary, reopen and compare to a committed prefix. Reapply a delta after recovery, force a divergent revision, and measure bounded-memory behavior. |
| The [type oracle](../type-models.md) enumerates a finite set of option payload uses and move/branch programs. The compiler has a wider language including recursive calls and foreign boundaries. | Extend the core semantics with a typed store, move paths, loan creation/end and drop effects. Define the relation from elaborated source into the proof calculus. | Prove preservation of store typing and non-overlap for each mutating step, with a compiler regression for each formal rule and counterexample. |
| The conditional pattern is lowered to an ordinary match. The IR has no general early return carrying a residual from a nested expression. | Before introducing postfix propagation, elaborate an early-exit edge for a single enclosing function and discharge its cleanup obligations. | Evaluate the operand once, skip succeeding statements on None/Err, reject a mismatched return carrier, and free each live owner exactly once at O0 and O3. |

The dependency order and broader acceptance evidence are in the
[implementation roadmap](../systems-ml-roadmap.md) and [PLAN section 19](../../PLAN.md#19-dependency-ordered-delivery-plan).

## GPU dispatch and Vulkan capabilities

The checked-in clamp harnesses allocate and synchronize their own buffers. Integrating dispatch into Rill requires checked range identity, alignment, target limits and ownership until completion. Add aliasing and device-loss tests before exposing asynchronous output to source programs. LLVM 22.1.8 also emits `Int8` for the Vulkan resource names. The unmodified binary ran on llvmpipe and skipped the physical WSL adapter that lacks `shaderInt8`. A fixture-specific packaging tool reproduces a validated Shader-only clamp. Gate the general portable profile on deriving required capabilities for each accepted kernel and a conformant hardware run. [Implementation steps and recorded experiments](../gpu-compilation.md)
