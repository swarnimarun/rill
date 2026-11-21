# Searchable docs command implementation plan

> **For agentic workers:** Implement content discovery, rendering and CLI acceptance tests in parallel; integrate and review each boundary before committing.

**Goal:** Generate portable, searchable, addressable HTML for the language and a local Rill project.

**Architecture:** A Rust CLI driver collects bundled language Markdown, bounded project Markdown and parsed source declarations. A CommonMark renderer builds independent HTML pages, a section search index and offline assets. The publisher replaces only a previously owned output tree after complete generation.

**Tech Stack:** Rust, pinned pulldown-cmark 0.13.5, existing serde_json, static CSS and JavaScript.

**Spec:** User request for `rill docs`; existing CLI/project contracts in README.md and docs/projects.md.

## Global constraints

- Preserve language semantics, branch history and unrelated edits.
- Do not invoke C compilers, dependency tools or project code to generate docs.
- Render exact source declarations; do not guess inferred types or foreign signatures.
- All assets and search work from file URLs and under a hosted URL prefix.
- Render each API comment independently and reserve declaration anchors before prose headings.
- Do not overwrite source, unrelated output, symlinks or user additions in a generated directory.
- Keep compilation feature boundaries and existing diagnostics intact.

## Task 1: Content discovery

Files: src/docgen/content.rs; shared types in src/docgen/mod.rs.

```rust
struct Page {
    path: String,
    title: String,
    section: String,
    source: String,
    markdown: String,
    items: Vec<ApiItem>,
}
struct ApiItem { kind: String, name: String, signature: String, markdown: String }
struct Site { title: String, pages: Vec<Page> }
fn collect(project: Option<&crate::project::Project>) -> Result<Site, String>;
```

- [x] Bundle current language/manual/library guides in the executable with original relative source identities.
- [x] Collect project README.md and docs/**/*.md with deterministic traversal, 256 files, 1 MiB per file, 16 MiB total and depth 16.
- [x] Parse reached source modules through the existing loader/SyntaxTree and render exported declaration signatures, adjacent /// comments and stable function/type headings.
- [x] Test ordering, signatures, comments, malformed syntax, bounded traversal and path restrictions.

## Task 2: Offline HTML reader

Files: src/docgen/render.rs, src/docgen/assets/style.css, src/docgen/assets/site.js.

```rust
fn render(site: &Site) -> Result<std::collections::BTreeMap<String, Vec<u8>>, String>;
```

- [x] Render Markdown code, tables and text with deterministic duplicate-safe heading IDs and links.
- [x] Rewrite included relative Markdown links and preserve safe external destinations; escape raw HTML and disable unsafe links.
- [x] Emit grouped navigation, page TOC, a responsive reader, shareable search URLs and section/symbol results with snippets.
- [x] Load the search index as a local script, using safe DOM text operations rather than network fetches or untrusted HTML.
- [x] Test IDs, cross-links, escaping and search entries; audit generated HTML and shipped JS locally. Browser file-URL navigation was policy-blocked, so visual/viewport checks remain explicitly unavailable in the verification record.

## Task 3: CLI and output publication

Files: src/docgen/mod.rs, src/main.rs, src/project.rs, tests/docs_cli.rs, .gitignore.

```sh
rill docs --language
rill docs my-project -o build/docs
rill project docs my-project --open
```

- [x] Parse strict options, auto-detect a current project, default output to build/docs and reuse project manifest validation.
- [x] Generate a complete candidate directory and publish with an ownership manifest; retain old output after failed content/render validation and reject unmanaged files or symlink paths.
- [x] Open the generated index with structured platform process arguments only when --open is explicit.
- [x] Test installed-style language bundling, project/API pages, project-relative output, regeneration, no-clobber and argument errors.

## Integration, verification and checkpoints

- [x] Run docs unit/CLI/project tests with default and minimal features, formatter, strict all-target Clippy and suitable existing CLI smoke tests.
- [x] Review independent content/render/publisher boundaries and fix concrete findings.
- [x] Update README, project guide, handoff and a docs-command guide with output and search contracts.
- [x] Commit verified implementation steps using the user-authorized per-command unsigned override; do not push or change global signing configuration.
- [x] Record exact checks and capability limits; do not mark language phases complete.


## Review-driven contract refinement

Independent review found that concatenating source comments and declarations
into a single Markdown document let a comment heading claim a later symbol's
URL. An unterminated fence could also absorb the next declaration. API pages
now carry structured ApiItems. The renderer allocates declaration IDs before
user Markdown and renders each comment in its own parse. This preserves exact
signatures and stable symbol URLs without treating comment text as declarations.

A second regression covers legal blank Markdown headings, whose search results
must retain a nonempty link label. Heading allocation also retains a suffix
counter per base and enforces the global work bound before expanding results.


## Final evidence

Default targeted verification passes 601 tests; minimal-feature verification
passes 49, including all 13 final docs CLI regressions. Both all-target Clippy
modes, formatting, JS checks and diff checks pass. A generated 26-page example
passes 1,890 HTML-link and 185 search-address checks. The verification record
distinguishes static/functional evidence from the blocked browser visual QA.
