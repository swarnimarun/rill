# Rust migration and diagnostics implementation plan

> **For agentic workers:** Use the delegate skill for independent documentation
> review and focused diagnostic implementation, then verify the combined result.

**Goal:** Help Rust developers write accepted Rill programs and recover from
unsupported syntax and misspelled names.

**Architecture:** Keep strict parsing and semantic checking unchanged. Attach
contextual help to rejected syntax and unknown names, using only visible names
for suggestions. Publish checked migration examples and a separate feature
proposal queue so proposed syntax cannot be mistaken for implemented syntax.

**Tech stack:** Existing Rust parser/checker/module resolver and Markdown docs.

**Spec:** User requests for missing-feature assessment, `break`/`continue`
alternatives, better diagnostics and a ready Rust-to-Rill migration guide.

## Boundaries

- Do not add abrupt control flow without owner/loan/cleanup semantics.
- Do not reserve currently valid identifiers merely to recognize Rust keywords.
- Preserve error codes and locations; append actionable help to actual errors.
- Suggest only unambiguous nearby names in the correct accessible namespace.
- Do not reveal private imported declarations or internal mangled names.
- Validate every new complete Rill example through checking and execution.
- Label missing features and implementation proposals explicitly.

## Task 1: Rejection help and spelling suggestions

Files: `src/diagnostics.rs`, `src/parser.rs`, `src/lexer.rs`, `src/check.rs`,
`src/modules/resolve.rs`, `src/lib.rs`, `tests/migration_diagnostics.rs`.

```rust
fn unknown_name<'a>(message: String, name: &str,
    candidates: impl IntoIterator<Item = &'a str>) -> String;
```

- [ ] Add bounded deterministic edit-distance suggestions. Ignore ties, very
  short names and compiler-generated identities.
- [ ] Integrate unknown local/function/type/field and module-name suggestions.
  Imported candidates must be exported and in the requested namespace.
- [ ] Add contextual help for Rust declaration words, unsupported abrupt flow,
  local annotations, assignment and unsupported lexical operators.
- [ ] Test exact errors and valid similarly named identifiers, privacy, recovery
  and interpreter/C behavior of the recommended loop patterns.
- [ ] Run focused parser, syntax, module and checker tests before committing.

## Task 2: Migration guide and feature decisions

Files: `docs/rust-to-rill.md`, `docs/proposals/rust-migration.md`,
`examples/rust-migration/`, `README.md`, `docs/language-features.md`,
`src/docgen/content.rs`, `tests/rust_migration_examples.rs`.

- [ ] Document commands, expression results, mutation, records/enums, modules,
  explicit ownership/loans, checked arithmetic, library and native boundaries.
- [ ] Provide complete runnable search/filter loop examples using boolean
  conditions and guarded bodies. Explain where these patterns differ from
  `break`, `continue`, early return and iterators.
- [ ] Inventory useful missing features with tradeoffs, prerequisites and
  acceptance criteria. Keep implementation decisions open for discussion.
- [ ] Link the guide into the offline searchable docs catalogue.
- [ ] Verify examples natively and in the interpreter; verify generated docs
  links/search and report actual checks before committing.
