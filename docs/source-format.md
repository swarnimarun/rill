# Lossless source and conservative formatting

This is an experimental P1/tooling increment, not completion of P1 or the
lossless grammar CST/recovery requirement in `PLAN.md` sections 16 and 19.

## Command

```sh
cargo run -- fmt examples/blocks/main.rill
cargo run -- fmt examples/blocks/main.rill --check
```

`fmt <source.rill>` prints the entire formatted file to standard output only
after validation. `--check` prints no source and exits successfully only when
the file already matches the formatter. A mismatch reports `E-FMT-CHECK` and
exits with status 1. Syntax failures report `E-FMT-SYNTAX`; read/write failures
report `E-FMT-IO`. Neither mode changes the input. There is no in-place write,
recursive-directory mode, stdin mode or multi-file mode in this increment.
Do not redirect stdout over the input path: the shell would truncate it before
Rill reads it. Save to a separate file and review it instead.

Formatting parses one module without resolving imports, type checking,
loading C headers, resolving pkg-config packages or running external tools.
It can format an exported binding or library module without a `main` function
and without installing its native dependencies. Use `check` separately for
semantic/native validation.

## Formatting contract

The formatter changes whitespace, not syntax:

- Braces and semicolons stay exactly where they were in the token stream.
  Nonempty braced bodies and statement boundaries receive line breaks. Blocks,
  record fields and match arms are indented with four spaces. Parenthesized
  multiline contents receive continuation indentation.
- Horizontal spacing is normalized conservatively. Existing nonempty line
  boundaries are retained, with at most one blank line between pieces. Leading
  and trailing whitespace is removed, and nonempty output ends in LF. There
  is no line-length wrapping or full grammar-aware style for types/expressions.
- Every token's original spelling is retained, including integer leading zeros,
  ordinary and C-string contents, UTF-8 bytes and escape spellings. String
  literals are never decoded and re-encoded for output. Physical multiline
  string literals remain rejected; supported escaped newlines are preserved.
- Every `//` comment retains its exact bytes and order. Inline comments stay
  inline with the preceding token; standalone comments stay on their own
  lines. Comment whitespace is content and is not trimmed, including CR before
  LF in a CRLF comment. Ordinary whitespace line breaks are emitted as LF.
- Identifier-to-parenthesis adjacency is retained, including separated legacy
  calls, constructors and parameter forms. Adjacent unary-minus spelling is
  retained. The current parser uses token positions only for diagnostics,
  not semantic decisions; this is not permission to erase legacy boundaries.
- Malformed or unsupported syntax is rejected, with no partial formatted
  output. Rill currently supports line comments, not `/* block comments */`.

Before returning output, `formatter::format` re-lexes it and checks equality of
all significant token kinds, exact token spellings and comment bytes. It
re-parses the module and compares the complete semantic parsed structure
(including native declarations and exports), then verifies that a second
layout pass is identical. Import paths/aliases are also protected by exact
token equality even though the parser's module AST does not retain them.
A failed preservation check reports `E-FMT-INVARIANT`, never changed source.
These checks are regression defenses, not a formal semantics proof.

## Source representation

`source::SourceFile::lex` and `lexer::lex_lossless` expose the same scanner used
by the compiler's existing `lexer::lex` API. There is no parallel lexer grammar.
A `SourceFile` borrows its UTF-8 input and provides ordered `Lexeme` pieces:
semantic tokens, whitespace and line comments, followed by a zero-width EOF.
Each piece retains its exact source slice and half-open `Span`. Both endpoints
carry UTF-8 byte offsets plus one-based line and Unicode-scalar column numbers.
LF starts a line; CR and tabs each advance one column, matching existing lexer
diagnostics. Columns are not display widths or UTF-16 offsets.

Concatenating all piece texts with `SourceFile::reconstruct` reproduces every
source byte. Valid lexical input can round-trip even if it is not valid syntax.
Invalid lexical input still returns the existing error; there are no recovery
pieces. The old `Token { kind, line, column }` API remains intact and projects
significant tokens from this scanner. No AST, typed or backend representation
has changed.

The flat lexical representation is also the leaf representation of the grammar
tree below. Full semantic source mapping and editor/LSP integration remain open.

## Shared-parser grammar trees

`syntax::SyntaxTree::parse(text)` records nested productions in the existing
recursive-descent parser, without a second grammar. Functions, types, foreign
slots, blocks/statements, expressions, patterns and match arms have lossless
children and spans. Binary precedence/association and source parentheses are
retained. Every token/trivia lexeme is a leaf exactly once, including EOF.
Node IDs are deterministic only within one immutable parse, not across edits.

`source`, `root`, `node`, `nodes`, `node_text` and `reconstruct` expose the tree.
Production spans include their significant tokens and internal trivia; enclosing
nodes own intervening trivia. `parsed_module` returns the ordinary AST only if
there are no diagnostics. `declarations` maps declaration vector indices to
source nodes in source order, including exports and optional semicolons. It does
not provide semantic validation. The optional [AST origin pass](ast-source-origins.md)
adds complete structural mappings for the current parsed AST.

Source-tree diagnostics have a code, byte/scalar span and original strict error
message. Failed top-level items become opaque error nodes. Recovery uses balanced
delimiters or a later unambiguous declaration at the original indentation, and
stops after 63 failures plus a final limit diagnostic. This is a bounded tooling
heuristic; later declarations are not guaranteed recoverable. Failed native
directives are rolled back before further validation. Any error suppresses the
entire AST and declaration map. All existing parser depth limits remain.

Lexical failure retains the whole file as an `UnlexedSource` leaf with the first
scanner diagnostic. It does not resume lexing or invent compiler tokens. Strict
lexer/parser/compiler APIs and the formatter still reject malformed input.
The opt-in [module loader](module-source-origins.md) preserves resolved AST
origins. Typed/backend origins, finer recovery, incremental editing and LSP remain
open. This increment does not complete P1. Fresh reconstruction evidence is in
[the grammar verification record](verification/2026-11-03-grammar-source-tree.md).
