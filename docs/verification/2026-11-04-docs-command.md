# Searchable docs command verification

This checkpoint adds `rill docs` and `rill project docs` as read-only source
consumers producing a managed static HTML directory. It does not change language
syntax, inference, ownership, native import or executable backend behavior, and
it closes no PLAN.md phase gate.

## Accepted behavior

- Language guides ship in the compiler, so generation works outside the source
  checkout and with native tools absent from PATH.
- Project manifests reuse existing validation. README/docs Markdown and reached
  source modules produce separate pages with portable relative links.
- Source APIs retain written signatures and adjacent documentation comments.
  Independent structured API items reserve their symbol IDs before commentary;
  comments cannot capture another declaration through headings or open fences.
- Source Markdown and published HTML fragment maps are distinct. Reserved shell
  IDs and duplicate headings retain correct source and generated destinations.
- Search indexes section/code/signature text, uses inert DOM text, and retains
  addressable query/result URLs. Search data is loaded only on search.html.
- Publication stages the complete candidate, accepts only a managed old tree,
  rejects untracked additions and symlinks, and repairs missing generated files.
  Renames provide complete content replacement and rollback, not a guarantee of
  uninterrupted directory availability between the two rename operations.
- Work, title, index and output limits reject excessive inputs before metadata
  expansion. A safe oversized-title regression retains the old index/manifest.

## Executed checks

Final source checks:

```sh
cargo test --locked --lib --bin rill --test docs_cli --test project_cli --test cli --test native_defaults
cargo test --locked --no-default-features --bin rill --test docs_cli --test project_cli
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
node scripts/test-docs-search.cjs
```

The default selection passes **601 tests**. The minimal selection passes **49
tests**. Each includes the final 13 docs CLI cases; counts overlap and are not
added. Both strict lint configurations, formatting, JS syntax and diff checks
pass. Python link-check syntax compiles with bytecode routed outside the repo.

The library/bin/CLI checks include all existing library unit tests, the existing
project workflow and native-default smoke tests. The larger C/SDL/GPU execution
matrix was not repeated for this isolated documentation feature. Historical
results, including the prior configured-SDL SDK setup failure and passing retry,
remain in the earlier checkpoint record.

## Generated artifact checks

A fresh site for the checked-in multi-module project contains 26 HTML pages.
`scripts/check-docs-links.py` checks **1,890 actual HTML links** and **185 search
result URLs**, including fragment existence, unique IDs, local containment and
relative asset/form destinations. The site was generated again after the final
renderer and guide edits; no generated files are committed.

`scripts/test-docs-search.cjs` evaluates the actual shipped JavaScript against a
small DOM model. It verifies multiword filtering, title ranking, snippets, safe
text, Unicode queries, URL updates, pagination, keyboard focus and the file-URL
history/native-GET fallback. This is a functional script check, not a browser
render or a claim that every browser's file handling was exercised.

## Review fixes and capability boundary

Independent review reproduced and fixed source-comment anchor theft, open-fence
bleed, blank search labels, reserved/duplicate fragment ambiguity and quadratic
metadata amplification. Heading IDs use per-base suffix counters and early
admission. Navigation/heading/item labels are limited to 256 Unicode characters;
serialized search data is capped incrementally at 32 MiB, and generated files
are capped before insertion at 128 MiB/2,048 files.

The browser tool rejected navigation to the generated file URL because its URL
policy permits only HTTP/HTTPS. No protocol workaround or alternate browser
surface was used. Visual browser, mobile screenshot and computed-layout QA are
therefore unavailable. CSS has responsive layouts and static style checks, but
no rendered-appearance success claim is made. The `--open` integration test uses
a controlled fake opener, verifies literal path arguments and failure handling,
and never launches a real browser. Windows opener execution was not tested.

No network services, source evaluation or native compilers run during docs
generation. No push or signing-configuration change is part of this checkpoint.
