# Parsed AST source origins

`SyntaxTree::ast_source_map()` is an opt-in post-parse tooling pass. It uses the
shared-parser grammar tree and the exact unmodified parsed AST, rather than
parsing a second grammar or changing semantic representations. It returns a
complete `AstSourceMap` or `MappingError`, never a partial map. Ordinary parser,
compiler and formatter APIs do not call it: a map failure cannot turn accepted
syntax into a compiler rejection. Files with syntax/lexical diagnostics have
no AST and cannot produce an origin map.

## Structural paths and lookup

The experimental `source_map` module exposes:

- `AstPath { declaration: AstDeclaration, steps: Vec<AstStep> }` roots a path in
  the corresponding function/type/extern/constant vector index. `root` and
  `child` construct paths without pretending they have already been validated.
- `AstPath::resolve_program(&Program)` and `resolve(&ParsedModule)` return an
  `AstNode` reference only after validating every child role and index. A wrong
  role, missing annotation or out-of-range index returns `None`, not a guessed
  node. `AstNode::kind`, `child` and `child_steps` expose structural inspection.
- `AstSourceMap::entries()` returns origins in source declaration order and
  semantic preorder. `origin(&AstPath)` retrieves an existing entry. Each
  `AstOrigin` records the path, `AstNodeKind`, tree-local grammar `NodeId`, span
  and explicit provenance. The source/tree must remain associated with the map.
- Mapping first walks declarations in source order and children in semantic
  order. It then checks coverage against independently enumerated program roots
  (functions, externs, constants, types) and structural children. Failure ordering
  is deterministic. A `MappingError` identifies a path when available and uses
  the `E-SOURCE-MAP` display prefix.

The covered structural nodes are every declaration, `VariantDecl`, `Param`,
`TypeExpr` including nested arguments, `Expr` including generated expressions,
`Pattern` including nested constructor fields, `MatchArm`, `ExternParam`, and
foreign parameter/result/constant `ExternType` slot. Parameters and declarations
are indexed, so duplicate names remain distinguishable before name checking.
Scalar name strings, operators, generic parameter-name strings, export-name
metadata and native configuration strings are not separate AST nodes; their
source remains in the enclosing grammar constructs. Imports have grammar nodes
but no corresponding declaration in the current parsed AST.

`AstStep` uses typed roles: function parameters/body and annotations; type
variants/field types/arguments; foreign parameters/types/result; let values and
bodies; binary operands; branch/loop conditions and bodies; assignment values;
call arguments; record field values; match values/arms/patterns/bodies; and
constructor-pattern fields. The resolver rejects using a role on the wrong
node category.

These are **structural addresses in one unmodified parsed program**, not stable
IDs across edits, module rewriting, name resolution, specialization or lowering.
Resolving the same address in a changed program establishes only that the path
exists, not that an old source origin still belongs to it. Downstream transforms
must explicitly preserve/remap origins and source-file identity. The optional
[module loader](module-source-origins.md) now does so for its existing
flattening/renaming pass. This API does
not manufacture an empty `ParsedModule` around a transformed `Program`.

## Source versus generated origins

Eight `OriginKind`s describe actual parser behavior:

- `Source`: an ordinary source construct. Redundant parentheses and expression
  wrappers do not create AST nodes; the inner semantic construct retains its
  own source origin, while grouping remains in the CST.
- `BlockBinding`: the AST `Let`/`LetMut` generated from a braced local binding.
  Its source anchor is the binding through its semicolon.
- `BlockSequence`: the generated outer `$block` binding for a non-tail
  expression statement, anchored to that statement through its semicolon.
- `StatementDiscard`: the generated inner `$discard` binding for that same
  statement, with its own structural path and the same source anchor.
- `ImplicitUnit`: a generated unit. Block fallthrough is a zero-width position
  at the closing brace; a statement-discard unit is anchored to its semicolon.
  An omitted braced `else` has a zero-width anchor at the end of its `if`
  expression. Written `()` is instead `Source`.
- `UnaryNegation`: a unary expression lowered to subtraction and its introduced
  zero operand. The subtraction spans the unary source form; the zero is
  anchored to the minus token. The special `-2147483648` literal remains
  `Source` and introduces no zero. Explicit `0 - x` is also ordinary source.
- `BooleanNegation`: `not value` or `!value` lowered to equality with false.
  The equality spans the unary expression; its introduced false operand is
  anchored to the negation token. The actual operand retains its source origin.
- `RecordVariant`: a record type's synthetic single AST variant, anchored to
  the record declaration. Its field types still have their own source origins.

An origin span is always inside its referenced grammar node, uses UTF-8 byte
and one-based line/scalar positions, and never claims that a synthesized zero,
false or unit was written in the source. Origin spans are **anchors**, not necessarily
whole semantic-subtree ranges: a block binding's later body can have an origin
outside that binding's anchor. This is intentionally different from the CST's
strict child-span partition invariant.

## Limits and remaining work

This mapping covers the current parser's structural AST, not resolved symbols,
inferred types, typed HIR/MIR, lowered cleanup operations, generated C or source
file identity across imported modules. The standard semantic checks remain
required. The [LSP](editor-support.md) uses these structural origins. Finer parser
recovery and incremental semantic updates remain open.
The implementation is bounded by existing parser depth limits but has no separate
whole-file memory budget. P1 is not complete.

See [fresh verification](verification/2026-11-03-ast-source-origins.md).
