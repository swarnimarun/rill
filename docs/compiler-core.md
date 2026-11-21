# Follow a program through the compiler

Open **Shape a signal** in the playground, then change the output view while keeping the same source. Track `sample * gain` from text to an integer result. For the initial input the product is 42. With `amplify(25, 3)` the product is 75 and the enclosing `if` returns 60. Run this experiment before editing the compiler so you have an observable contract. [Workbench and commands](learning-workbench.md)

## Source, tokens and trees

Look at the tokens around `let level = sample * gain;`. A local identifier, the `*` operator and a statement terminator each occupy their own source range. `src/source.rs` records lexemes and positions, including whitespace and comments. `src/lexer.rs` exposes the token stream consumed by the parser. The token view lets you distinguish an unexpected token from a later disagreement about its meaning.

The handwritten parser in `src/parser.rs` builds declarations, expressions and patterns from those tokens. The AST types are in `src/ast.rs`. In the amplifier, the initializer of `level` is a multiplication with two argument names, and the continuation is an `if` whose condition compares `level` with `60`. A missing operand fails here, before any type is inferred.

The concrete syntax tree in `src/syntax.rs` preserves source spelling, delimiters and comments. It supports editor recovery on an unfinished document. Remove the semicolon in a local binding immediately before another `let`, then inspect diagnostics. The source tree can identify the insertion point and continue reporting later syntax errors. Compilation requires a complete parse. Likewise, the formatter in `src/formatter.rs` adjusts whitespace and re-parses its output. The AST view shows the semantic expression structure, while formatting works from the original text. [Source formatting](source-format.md)

## Names, inference and ownership

A file-based command first loads the module graph. `src/modules/compile.rs` and `src/modules/resolve.rs` resolve source declarations and imports, and `src/modules/stdlib.rs` provides the bundled library. An explicitly local module can have the same spelling as a bundled module. References must follow the symbol the loader resolved. A failed import or privacy check must stop compilation before code generation. The one-buffer playground lessons avoid filesystem dependencies.

`src/check.rs` analyzes the resolved program. A `CheckedProgram` keeps access to its source AST and the inferred facts for that exact program. For `amplify`, multiplication constrains both inputs to `i32`. The comparison produces `bool`, both arms produce `i32`, and the function therefore returns `i32`. Change `gain` at the call site to `true` and check again. Expressions can parse successfully and still fail these constraints. Arithmetic also has runtime semantics. A type-correct multiplication whose mathematical result exceeds signed 32-bit range traps during execution.

The checker builds recursive dependency groups, solves equality constraints and generalizes eligible bindings. The **Read an inferred type** lesson uses the immutable function `keep` at both `bool` and `i32`. Its parameter and result share a type variable. Different calls may instantiate that variable differently. A mutable slot has one element type throughout its lifetime. For inference changes, start with `src/check/expressions.rs`, `src/check/graph.rs` and `src/check/solver.rs`. Check pattern coverage in `src/check/exhaustiveness.rs` and nominal field information in `src/check/projections.rs`. [Operand diagnostics](operand-diagnostics.md) · [Type models](type-models.md)

A successful type unification alone does not authorize a move. In **Follow an allocation**, `observe(sample)` borrows the box through a `read` parameter, so the subsequent `finish(move sample)` can consume it. If that transfer happens first, reading `sample` in the caller must fail. In **Move a cursor**, `advance(inout cursor)` requires a mutable caller binding and exclusive access while the call runs. Trace owner liveness through `src/check/ownership.rs` and checked place operations and cleanup through `src/typed/`. Test failure paths as well as successful results. [Ownership boundaries](blockers/ownership.md) · [Owned allocation](blockers/owned-allocation.md)

## Host execution and C11

Both `interpreter::run` and `backend::emit` check their program, perform callable lowering when required, and attempt the supported typed path. `typed::lower_main` specializes reachable definitions into the current host IR. The verifier checks the representation that the typed evaluator and C emitter consume. For an unsupported typed form, only eligible owner-free programs can take the general checked path. Programs with owners must pass the checked typed path, including cleanup obligations. In the **Typed IR** view, inspect the verified host representation for a supported `main`. Compare the owner lesson before and after replacing a read with a move. The `rir` view under kernel tooling instead requires a named scalar map. Read `src/interpreter.rs`, `src/backend.rs`, `src/typed.rs` and `src/typed/c/` in that order.

Use the C11 view of **Trace a native branch**. Starting at the generated C `main`, find the call that computes the result, then follow the signed comparison and the assignments made by each arm. Generated temporary names are implementation details. The stable observation is that a negative input returns 1 and an input of 8 returns 2. Use the CLI at two optimization levels to test that agreement with the interpreter.

```sh
target/debug/rill run examples/playground/native-branch.rill --interpret
target/debug/rill run examples/playground/native-branch.rill --opt-level 0
target/debug/rill run examples/playground/native-branch.rill --opt-level 3
```

Use `--interpret` to debug language semantics. Use `emit-c` to see representation and evaluation order. Use a native build when investigating foreign headers, C translation, linking or the external ABI. The host compiler checks C declarations against real headers. Raw C pointer lifetimes and foreign cleanup need their own contracts. [C interoperability](c-interop.md) · [Native builds](native-builds.md)

## Rill kernel IR and LLVM

The kernel IR is a versioned text representation for a small device-map profile. It is separate from the host typed representation above. The source extractor in `src/kernel_source.rs` selects a named function from a checked source program. Its type must be a monomorphic by-value `i32 -> i32`, and each admitted expression must be a pure, total member of the profile. A valid general Rill function, including `amplify` with its checked arithmetic, can therefore be rejected by this extractor.

Open **Classify GPU lanes** and use entry `band`. Its two comparisons become `slt.i32` and its conditionals become `select.i32`. Each named result has one definition. `src/kernel_ir.rs` verifies the version, header, definition order, operand and return types, and instruction budget. A comparison produces a boolean that can select an integer. Returning that boolean where the profile requires an integer is invalid.

Each input to IR evaluation represents one active lane. Use `-1, 0, 7, 8` to check both sides of both thresholds. Emission uses the same verified body with different calling conventions. The NVPTX wrapper accepts device buffers and a count. The Vulkan compute wrapper binds the input, output and count at separate descriptor bindings. An LLVM module is an intermediate result. Device execution also exercises target code generation, resource binding, dispatch bounds and host synchronization. [Target ABIs, validators and device harnesses](gpu-compilation.md)

## Replay a playground request

Start `cargo run --locked -- playground`. The browser sends the source and selected view to the same local endpoint a command-line client can use. This example asks the type checker for the signature of a polymorphic identity function.

```sh
curl -sS http://127.0.0.1:7878/api/inspect \
  -H 'Content-Type: application/json' \
  -d '{"source":"fn keep(x) { x }","view":"types"}'
```

The JSON result has `ok`, `output` and `diagnostics` fields. Look for `fn keep(x: 'a) -> 'a` in `output`. Change `view` to `run` to see the missing-entry diagnostic. `GET /api/lessons` returns the bundled lesson sources, exercises, stages and expectations. The editor shows each lesson's repository filename. Diagnostics for the edited in-memory buffer use `playground.rill`.

Follow the request through `src/playground/http.rs` and `src/playground/worker.rs` to `rill::explorer::inspect` in `src/explorer.rs`. To test a new compiler operation without HTTP, pass the same JSON value to `inspect` in `tests/explorer_core.rs`. The worker accepts a single source buffer with explicitly bundled standard-library imports for compilation. Native dependencies and project-relative imports belong in the CLI workflow. Each request has a five-second wall-clock budget, a 64 KiB source budget and a 256 KiB compiler-text budget. Linux also caps the worker address space at 512 MiB. Cancelling or timing out a calculation leaves the server available for the following edit.

For host IR, try `fn twice(x) { x + x } fn main() { twice(21) }`. Find `Binary Add : i32` inside `twice` and `Call f0(value) : i32` in `main` for this example. A slot or function ID identifies an entity within one compilation. For an allocation, look for a move from the first owner slot and `DropWholeOwner` on the scope cleanup. The type after a block describes its expression result. The host listing is for inspection, while the separate `rir 0` kernel format has its own versioned verifier.

## Editor and compiler tests

`src/lsp/mod.rs` handles requests for versioned documents. `src/lsp/analysis/` uses source and checker information for diagnostics, navigation, hover and inlay hints. `src/lsp/text.rs` translates byte offsets to and from UTF-16 positions in the editor protocol. When debugging a position, try a line containing a non-ASCII character before the target. An editor error may concern a stale buffer or a coordinate conversion even when the compiler's source span is correct. [Editor tooling](editor-support.md)

| When you change | First evidence to collect |
| --- | --- |
| Lexing, grammar or recovery | A minimal valid file and a malformed variant in `syntax_tree` or `conditional_patterns`. Check the formatter and versioned diagnostics in `lsp_protocol`. |
| Inference, patterns or ownership | An accepting/rejecting pair in `operand_diagnostics`, `function_inference`, `owner_borrows` or `core_models`. Compare interpreted and compiled behavior when a program executes. |
| Host lowering or native layout | Inspect the generated C. Use `backend_reference` or `owner_optimization` and run the example at O0 and O3. Check destruction and allocation effects for owner code. |
| Kernel semantics or layout | Run `cargo test --locked --lib kernel_` and `cargo test --locked --test kernel_cli`. Compare the CPU IR result before validating target artifacts or dispatching a device kernel. |
| Playground modes or chapters | Exercise the same source in the edited view and run `rill playground --check-lessons`. Rebuild `rill docs --language` and run the offline link check. |

For a regression report, include the smallest source, the exact command or playground view, the expected result, and the compiler's actual output. Include the kernel entry and lane inputs for device cases. Typed checks, finite independent models, formal theorems and hardware execution each answer different questions. The closed Option calculus has mechanized progress and preservation. Broader source inference, ownership and foreign behavior require additional evidence. [Formal proof scope](option-core-proof.md) · [Implementation status](phase-status.md)
