# Helix workflow integration plan

Historical plan/evidence retained from archive snapshot `69277817`.
Its commands, counts and checkboxes describe that checkpoint. The current port
is tracked in the [integration report](../../verification/2026-11-04-ernie-integration.md).

Goal: make the existing Rill compiler, LSP and grammar work together through
normal Helix editing actions, including incomplete expressions and function
values. Preserve other user settings and all unrelated implementation work.

## Work and acceptance

- [x] Check the installed configuration and baseline compiler-backed editor
  responses against functions, direct SDL3 and voxel sources. Confirm no native
  tools are needed for editing and no configuration replacement is necessary.
- [x] Reproduce and fix missing scoped/callable completion details, incomplete
  local completion, record member/callback completion and operator navigation.
  Infer members from checked nominal identities, preserving visibility and
  UTF-16 positions. Do not invent types for discarded incomplete syntax.
- [x] Correct structural query gaps, including lambda function textobjects,
  expression-position local references and parameter/member color collisions.
  Verify generated parser reproducibility, corpus parsing and actual captures.
- [x] Exercise real Helix `gd`, hover, completion selection, symbol picker,
  formatting and structural selection/indentation with private temporary
  fixtures, decoded terminal output and matching LSP response IDs. Retain
  existing live diagnostics, inline hint and signature popup proof.
- [x] Rebuild the release compiler, run affected tests, formatting, strict
  Clippy and the complete regression inventory as appropriate. Independently
  inspect fixes and record limitations and capability skips.
- [x] Stage and review the current parser/queries, refresh only Rill's installed
  Helix files with backups, and verify installed health plus unchanged other
  settings and compiler symlink. Record final evidence in a verification report.

Evidence: [Helix workflow verification](../../verification/2026-11-04-helix-workflows.md).

## Scope and invariants

The LSP uses compiler snapshots, source origins and the normal module resolver.
It does not run C compilers or pkg-config. Error recovery may locate a scope or
call context, but cannot supply fabricated semantic types. Tree-sitter and Helix
queries provide editing structure and highlighting; they are not compiler or
ownership proofs. No language phase or stable-language claim is made.

Function textobjects follow Helix's bundled Rust convention: inside selects the
function body, around selects the declaration/expression. The actual editor
test determines the selection range, including nested and empty lambdas.
