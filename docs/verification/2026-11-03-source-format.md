# Lossless source and formatter verification, 3 November 2026

## Scope

A bounded P1/section-18 increment on the continued `v1` lineage: one shared
scanner retains exact lexical source pieces and spans; conservative formatting
is available through the library and a stdout/`--check` CLI. See
[the contract](../source-format.md). No full grammar CST, recovery or P1
completion is claimed. No source write mode or native tool invocation was
added to formatting.

## Focused checks

On x86_64 Linux, Rust/Cargo 1.99.1 and GCC 14.2:

```sh
cargo fmt --all -- --check
cargo test --locked --lib parser::tests -- --nocapture
cargo test --locked --test source_format --test format_cli --test parser_stack --test brace_syntax -- --nocapture
cargo clippy --locked --lib --bin rill --test source_format --test format_cli -- -D warnings
```

- 18 existing parser unit tests passed.
- 23 affected integration tests passed: 11 source/format tests, 4 CLI tests,
  1 parser-stack test and 7 braced-syntax tests.
- All 38 repository `.rill` files under examples, stdlib and test fixtures
  reconstructed byte-for-byte, parsed identically after formatting and formatted
  idempotently. Corpus testing reads files without rewriting them.
- Original and formatted control-flow, generic Copy-pattern/legacy-call and
  scoped-owner programs each returned `42` through interpretation and C at
  both `-O0` and `-O2` (12 native executions).
- Tests cover UTF-8/scalar positions, CRLF, tabs, whitespace-only sources,
  comments at EOF and inline/standalone comments, exact literal escapes,
  C strings, nested types/records/matches, missing native dependencies/imports,
  legacy calls and constructors, negative arguments, semicolon/unit behavior,
  malformed input, unsupported block comments and deep-input rejection.
- CLI tests verify stdout/check mode, paths containing spaces and non-UTF-8
  filenames (Unix), formatting native modules with no tool PATH, missing files,
  invalid UTF-8, malformed source, rejected options and unchanged input files.

An additional old/new scanner differential experiment compared exact tokens,
positions and diagnostics against the pre-increment lexer on 50,000 generated
Unicode/operator/string/comment inputs and the same 38 repository files, with
no difference.

## Independent review

The independent reviewer audited every parser and module-loader position read:
line/column values feed diagnostics, not parsing decisions. The module AST
comparison covers native configuration/declarations and exports; significant
lexeme comparison covers the import details discarded by the parser AST.

Its separate harness accepted 852 valid modules from repository sources and
extracted Rust fixture candidates, tested 10,224 randomized whitespace variants
for AST/token/comment/identifier-parenthesis preservation and idempotence,
and verified that 336 malformed candidates reject with `E-FMT-SYNTAX`. A
separate 50,000-input lexer differential also found no discrepancy. These are
bounded generated tests, not an exhaustive proof.

The reviewer also checked 8 focused before/after programs covering UTF-8 and
escaped strings, nested generic constructors/matches, legacy negative calls,
record mutation, byte buffers, loops, scoped-owner cleanup and a comment-split
unary minus. All agreed in the production interpreter; the 7 non-owner cases
also agreed in the checked AST oracle. All 64 before/after C builds/runs passed
with GCC and Clang at `-O0`/`-O2`. Independent CLI checks covered non-UTF-8
paths, unchanged symlink inputs, check-mode stdout and native declarations
with deliberately unusable compiler/PATH settings. No blocking review finding
remained. The independent adversarial generator also exercised formatter
acceptance/rejection and invariants on its 50,000 inputs.

The parent continuation task owns integrated default/`c-import` full suites
and aggregate Clippy after combining independent increments. This record does
not claim those aggregate checks were run in this isolated worktree.
