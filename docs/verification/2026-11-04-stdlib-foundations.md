# Standard library foundations verification

The catalogue expands from six to 15 bundled modules, with nine new source
modules, 77 exported functions and five exported types. The Rust change is an
explicit include catalogue and a bundled documentation chapter. No compiler
builtin, IR, runtime, native binding, dependency or implicit prelude is added.
Existing `std.result` and `std.alloc` sources remain byte-identical, preserving
the canonical allocation contract. No PLAN.md phase is marked complete.

See the [guide](../standard-library.md), [catalogue](../../stdlib/README.md) and
[remaining compiler/library prerequisites](../blockers/standard-library.md).

## Executed checks at the feature checkpoint

```sh
cargo test --locked --test stdlib_ops --test stdlib_int --test stdlib_range --test stdlib_ascii --test stdlib_text --test stdlib_bytes --test stdlib_binary --test bundled_stdlib --test fallible_allocation_module --test owner_adt_module --test stdlib_cli --test stdlib_json --test stdlib_reload --test docs_cli --test project_cli
cargo test --locked --no-default-features --test stdlib_ops --test stdlib_int --test stdlib_range --test stdlib_ascii --test stdlib_text --test stdlib_bytes --test stdlib_binary
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

The final default selection passes **62 tests**; the minimal selection passes
all **24 new tests**. Counts overlap and are not added. Both strict all-target
lint configurations, formatting and diff checks pass. Independent spec/quality
review found no implementation defect and checked the oracle construction,
arithmetic guards, indexing, ownership and preflight behavior.

## Complete frozen-source matrix

```sh
SDKROOT="$(xcrun --show-sdk-path)" cargo test --locked --no-fail-fast
```

The full command exits successfully on feature source `23c50f51`, reporting
**1,489 passed, 0 failed and 0 ignored** across 149 result groups. All 24 new
tests pass in this run. Source and test files were frozen before starting;
only this evidence and the unbundled implementation plan change afterward.

This includes existing compiler/IR, ownership/cleanup, native imports and C ABI,
module/editor, project/watch, docs and library regressions. Configured real
SDL3 and both AArch64 NEON tests pass, including actual vector execution and
the emitted XOR instruction check. The host is arm64 macOS with Rust 1.99.1
and Apple Clang 17. No Windows execution or general target portability proof
is added. Selected and full counts overlap and are not summed.

The GPU target reports successful test returns for availability skips, rather
than marking them ignored. A separate `--test sdl3_voxel_gpu -- --nocapture`
run confirms three hardware preflights return status 77 because the video
driver adds no displays; the fourth, 256-cubed capacity branch is disabled
without `RILL_TEST_GPU_256=1`. All four GPU hardware branches are skipped.
The aggregate count therefore does not establish GPU execution acceptance.

## Semantic evidence

- `std.int` compares 353 signed input pairs against a wider Rust oracle for
  checked arithmetic and saturating results, plus unary/clamp cases. MIN
  negation, MIN/-1 division and remainder, zero divisors and inverted bounds
  return exact errors before invalid operations.
- `std.range` alternates front/back iteration, covers empty/reversed ranges,
  both signed endpoints and lengths that cannot fit positive i32. Guarded
  updates preserve endpoints without wrapping; empty cursors remain unchanged.
- `std.ascii` checks all 256 byte codes and eight out-of-domain integers across
  17 functions. `std.text` compares 256 string pairs and 25 trim/span cases,
  including UTF-8, embedded NUL, all six whitespace bytes and invalid bounds.
  Private helpers remain inaccessible through imported interfaces.
- `std.bytes` compares 738 small/invalid memmove ranges against Rust's own
  `copy_within`. Complete destination bytes, overlap directions, empty ranges,
  byte errors, distinct-root copies and loan rejection are checked.
- `std.binary` compares 25 signed encodings against Rust endian bytes, reads
  independently generated inputs for all 256 sign bytes, and exercises 126
  valid/invalid offset/size combinations. Unaligned ranges work. Failed reads
  and writes preserve complete input/destination buffers.
- C observers intercept allocation/free in a separately compiled translation
  unit. Exact counts match caller-created buffers at O0/O3, including the
  hundreds of repeated memmove calls. The utilities introduce no extra
  allocations and normal cleanup releases every test owner once.
- Generic Option/Result tests exercise multiple concrete types, Copy captures,
  success/error callbacks and conversions. Intentional trapping callbacks prove
  unselected branches stay lazy. An ordinary fallback argument still traps when
  eagerly evaluated. Type-only Option carries a live owner; importing generic
  helpers into owner programs rejects explicitly.

All new execution fixtures compare the checked interpreter with actual C11
programs at **O0 and O3**. Numeric/range Copy fixtures also compare the separate
reference interpreter. This establishes the tested semantics, not a universal
soundness, performance or allocation-effect proof.

## Tooling and examples

The checked-in `utilities.rill` and `options.rill` examples each print `42` in
the interpreter and both C optimization modes. The first composes monomorphic
utilities with owned storage; the second uses generic Copy callbacks. Imports
run from fresh temporary directories without copying any bundled sources.
The complete 15-module catalogue is also discovered through the ordinary file
loader. Local files cannot shadow the reserved bundled Option identity.

Two generated example-project documentation sites include the manual and all
nine new module APIs across the pair. The actual HTML audits pass with:

| Example | HTML pages | Checked links | Search addresses |
| --- | ---: | ---: | ---: |
| Utilities | 36 | 3,358 | 273 |
| Option/Result callbacks | 33 | 2,866 | 240 |

Relative source-document links resolve. Generated sites, test executables and
caches are outside Git. No new rendered-browser layout claim is made; the
previous local-file browser-policy limitation remains.

## Exact capability boundary

Importing bytes/binary introduces owner-typed declarations even when unused,
so those imports cannot coexist with generic `_ops` imports yet. Tests split
catalogue discovery, Copy helper execution and monomorphic owner execution
instead of hiding the limitation through unused-declaration pruning.

The library does not provide growable containers, runtime strings, general
Unicode operations, OS streams, filesystem/network/process input, clocks or
randomness. Those require the documented compiler/resource/platform contracts.
Checked scalar utilities and endian bytes do not establish a new C aggregate
ABI, native reload loader, stable standard-library release or completed phase.
