# Searchable HTML documentation

Build a static documentation site with the installed compiler:

```sh
rill docs --language
rill docs my-project
rill project docs my-project
rill project docs my-project --open
```

`rill docs` includes the language manual bundled with the executable. When the
current directory contains `rill.json`, it also documents that project. An
explicit project directory must have a valid manifest. `--language` omits the
current project and cannot be combined with an explicit project directory.
`rill project docs` uses the same driver and manifest validation as the other
project commands.

The default output is `build/docs`, relative to the selected project root or
the current directory for language-only documentation. Override it with `-o` or
`--output`; relative overrides also resolve from the selected project root.
The command prints the resulting `index.html` path. `--open` launches that file
with the platform's normal document opener after successful generation.

```sh
rill docs my-project -o public/reference
rill docs --language -o /tmp/rill-language-docs
```

## Contents and addresses

The site contains bundled language guides, project `README.md` and Markdown
under `docs/`, plus API pages for reached source modules. Each document has its
own HTML URL, with deterministic headings and stable source symbol fragments. Examples include:

```text
index.html
language/syntax-tour.html
project/README.html
project/docs/guide.html
api/main.html
api/math.html#fn-double
api/math.html#type-point
search.html?q=double
```

Imported modules named `main` use `api/~main.html`, leaving `api/main.html` for
the entry module. Unusual Markdown filename bytes use portable `~hh` escapes.

API pages show source declarations and adjacent `///` documentation comments.
They do not infer missing annotations, import C headers or execute project code.
Declaration anchors are reserved before comment headings, and each comment
renders independently. They document the parsed source interface; this command is not a replacement
for `rill check`. Private helpers in imported modules are omitted. Entry-point
source is documented with its own page.

```rill
/// Return twice the supplied integer.
export fn double(value: i32) -> i32 {
    value + value
}
```

Markdown supports headings, links, lists, fenced code and tables. Links to
included documents and source API pages become relative generated links.
Unsupported local-file links remain readable labels. Raw HTML is escaped and
unsafe URL schemes are disabled. Image references remain alt text; the command
does not copy local image assets or load remote images. Project text is treated as documentation, not
as scripts to execute.

## Search and hosting

Use the search field or press `/` outside an editable control. Search matches
section text, code, page titles and API symbols without contacting a server.
Several search terms narrow the results together, using up to 200 characters
and 12 distinct terms. Results include snippets and
link directly to the matching section. The query stays in the search-page URL,
so it can be copied or bookmarked.

The site loads its index from a local script and works when `index.html` is
opened directly from disk. All CSS, JavaScript, navigation and search assets
are included. To host it, copy the complete generated directory to any static
host, including a subdirectory; no application server or root-relative URL
configuration is required. Generated pages remain readable without JavaScript;
interactive search requires it.

## Regeneration and input limits

Choose a dedicated output directory. A first generation requires that the
output does not already exist. Later generations recognize `.rill-docs.json`
and replace a complete previously generated tree after building its candidate.
A failed parse/render leaves the previous output intact. An existing directory
with unrelated files, user additions, invalid ownership metadata or symlinked
output/assets rejects. The generator never treats an arbitrary folder as
replaceable output.

Parent directory aliases are canonicalized before publication, including host
temporary paths. Inputs and output directories must remain stable during a
command; this is a local development tool, not a filesystem security sandbox.
Missing generated files are recreated during regeneration. Source files and
hand-written Markdown stay outside the generated directory.
Do not edit generated files. Add `.rill-docs.json` to neither source docs nor
an unrelated folder. New project scaffolds already ignore `/build/`.

Project Markdown discovery is bounded to 256 files, 1 MiB per file, depth 16
and 8,192 directory entries. The complete collected Markdown/API corpus has a
16 MiB limit, as does the source graph used for API extraction. Source modules retain the existing loader's graph and
file limits. The renderer bounds its corpus and serialized search index to 32 MiB each,
with at most 50,000 headings/indexed sections. Navigation labels, headings
and source item labels are limited to 256 Unicode characters. Generated
relative paths are limited to 1,024 bytes; output is limited to 2,048 files
and 128 MiB, checked before accumulating more files. Exceeding a
limit reports a diagnostic instead of silently truncating searchable content.
Bundled language documentation belongs to the compiler version; rebuild the
compiler to update it.
