# Boolean negation and SDL3 main

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](2026-11-04-ernie-integration.md).

The lexer previously treated `not` as an identifier and rejected standalone
`!`. `if not false { 42 } else { 0 }` consequently failed with `expected Then`.
The current SDL3 main also had an initialization guard without the required
`else` branch.

Both `not value` and `!value` now accept boolean operands. They bind before
binary operators, evaluate their operand once and retain short-circuit behavior
when combined with `&&` and `||`. Parenthesize comparisons, such as
`not (x == y)`. `not` is now reserved; names such as `not_ready` remain identifiers.

The parser lowers negation to boolean equality with false, reusing the checked
interpreter and C operations. Tooling retains a unary syntax node and explicit
`BooleanNegation` origins for the expression and synthesized false operand.
The SDL3 example uses both spellings and keeps initialization failure separate
from window creation with an `else` branch. Cleanup behavior is unchanged.

Verification:

- Both new regression targets failed before the fix. They now check truth
  tables, precedence, single evaluation, short-circuit traps, rejected operand
  types, depth limits, source origins, formatting and module preservation.
- All 85 default test executables passed, totaling 551 cases, plus doc tests.
  Cargo enumerated the executables with `--no-run --message-format=json`;
  each ran with two threads, at most three targets concurrently. The final
  updated AST provenance fixture also passed its affected-target rerun.
- The five importer-enabled SDL3 tests passed against the real library,
  including generated bindings, Zig, framebuffer pixels and lifecycle checks.
- `check` and an `-O2` build accepted the actual SDL3 main. Its full headless
  run and desktop execution both printed `0` with no stderr. Desktop execution
  used the already documented exception for GUI-restricted sandbox execution.
- Formatting, default/importer strict all-target Clippy and diff checks passed.

The two existing default-suite invalid-byte filename capability skips remain;
no SDL3 or Zig case was skipped. This fixes the reported syntax/application
failure without changing phase acceptance or the foreign ownership boundary.
