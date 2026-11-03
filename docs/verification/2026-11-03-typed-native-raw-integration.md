# Integrated raw native transport and real SDL3 captures

Verified on 3 November 2026 against source commit
`2015bdd00fb8ac591e578ea2fb98af83be05bbd3`. This report and the accompanying
handoff updates are documentation-only changes after that source verification.

## Accepted scope

The bounded F0–F3 migration now routes existing supported Copy foreign scalars,
nullable raw pointers, calls/constants, conversions, equality, closed aggregates,
concrete generics and root/field slot loans through verified typed C. No new
source ABI or pointer lifetime guarantee was added. Static C strings remain
literal-only call operands. The complete native manifest and target-header
assertions are preserved, even for unused declarations. The checked uniform
emitter remains an explicit differential oracle and fallback for unrelated
unsupported Copy layouts. The evaluator still rejects reached foreign work; it
does not call host C.

The earlier function-instance origin index remains a narrower opt-in API.
Inactive raw ADT fields and raw phantom arguments are explicitly rejected after
ordinary lowering and verification, rather than yielding a partial index or an
incorrect metadata-corruption error. Ordinary typed output is unchanged.

## Final integrated checks

All commands ran from the exact clean source above using Rust/Cargo 1.99.1,
GCC 14.2.0, Clang/libclang 19.1.7 and SDL3 3.2.10 on x86_64 Linux.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| `cargo test --locked --no-fail-fast -- --nocapture` | 614 passed |
| `cargo clippy --locked --all-targets -- -D warnings` | passed |
| `cargo test --locked --features c-import --no-fail-fast -- --nocapture` | 648 passed |
| `cargo clippy --locked --features c-import --all-targets -- -D warnings` | passed |
| Windows GNU default all-target compile check | passed |
| Windows GNU importer-enabled all-target compile check | passed |
| Capture harness standalone Python tests | 18 passed |

Both full test configurations reported zero failed or ignored tests and no
dependency-skip notices. Windows checks compile but do not execute Windows
programs. The Python capture tests also passed independently before integration.
Fresh package builds recorded the correct checkout and artifact identity;
crate-root timestamp refreshes forced rebuilds without changing source bytes.

Independent review on this same source additionally passed 82 focused Rust
tests, seven former-red origin-boundary/control cases, seven public lowering
cases, 11 evaluator outcomes, eight source rejections, and 42 native comparison
rows. GCC and Clang each ran seven cases at O0/O2/O3, comparing exact exit status,
stdout and stderr against handwritten C. Six cases also used the checked
uniform C oracle; the generalized-capture case used handwritten C because of the
uniform emitter's separately documented local-shape limitation. These rows
comprise 120 program executions. The separate author checkpoint passed 596/630
full tests and a 93-test actual-Clang replay; those counts are not relabeled as
independent integrated runs.

## Actual SDL3 application evidence

A fresh importer-enabled executable was built from the verified source, with
Cargo recording the exact manifest, feature set and `fresh: false`.
Its SHA-256 was `beb7a53cfdebfe243b1878b2244ad05ad18b992a26294d5871105dffd2525c59`.

The [capture matrix](../sdl3-capture-matrix.md) then ran the unchanged direct SDL3
example, its selected generated-binding variant, and the collector/shim example
under GCC and Clang at O0 and O2. All 12 native executions exited normally,
printed `0`, and captured frame 40 using SDL3's actual software renderer and
dummy video driver. All emitted C was retained and checked to contain neither
`rl_value` nor `RL_CAP` before that exact C was compiled.

- Direct and generated scenes: 640 × 400 pixels.
- Collector scene: 640 × 480 pixels.
- Ten within-compiler complete-pixel comparisons passed.
- Six corresponding GCC/Clang complete-pixel comparisons passed.
- All expected pixel probes passed, and all 12 lossless PNGs were visually
  inspected. Direct/generated scenes show their green gates and moving gold
  line; the collector shows its blue player and yellow target.

Canonical RGBA SHA-256 values, identical across the tested optimizations and
compilers:

- Direct/generated: `6071a59e83212ddc1a1da1f497520c48a8f39cb1316f5f1bad553a9880f5e269`
- Collector: `27b676673a327aeef3e68d1f48d0e382e1e79817a1fbc55ba507fd0faf22b4a5`

The evidence archive retains PNGs, actual emitted C, generated/original Rill
sources, capture wrapper, manifests, command arguments and stdout/stderr.
Images were converted losslessly from captured BMPs, without redraw or overlay.
The ordinary direct/generated build/run CLI tests remain separate and passed in
the full suite; this retained-C capture path does not replace those tests.

## Retained failures and corrections

- Float/pointer comma-expression macros exposed a checked uniform-oracle
  operand-grouping bug. The narrow grouping correction has red/green author
  evidence and independent optimized native coverage.
- The optional origin API's shallow source raw-value flag missed inactive
  native fields and phantom arguments. Four independent failures were retained;
  the complete verified-representation gate now passes all seven controls.
- An initial integration helper accessed a tuple parameter as a named field.
  Its fresh E0609 compiler failure was retained, corrected by tuple destructuring,
  and followed by the complete final verification above.

## Limits and remaining work

This is a bounded native transport/integration checkpoint, not P1 or P5 phase
completion. Raw handles have no checked foreign lifetime, pointee independence,
resource cleanup or effect contract. Owner/native combinations remain rejected.
No host FFI evaluator, aggregate C ABI, callbacks, exports, stable ABI or new
source syntax is claimed. Unrelated owned-record work is not included.

These are real headless software framebuffers, not physical keyboard/mouse,
desktop-window or GPU-renderer tests. No Zig or AArch64/NEON runtime evidence is
claimed here. Complete typed-node/generated-C mappings, broader ownership and
remaining plan phases still need their own accepted increments.
