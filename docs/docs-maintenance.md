# Maintain and host the documentation

Edit Markdown and source documentation in the repository, then generate the
HTML site with the same Rill compiler used by readers. The generated site is a
build artifact. Keep it out of Git history. GitHub Pages can publish that
artifact directly, without a separate `gh-pages` branch or another site generator.

## Sources and addresses

The language guides live under `docs/`, with the standard-library catalogue in
`stdlib/README.md`. The repository README introduces the project and links into
those guides. Public API comments belong beside declarations in `.rill` files,
using `///` comments. Each change should update the guide and examples that
describe its behavior, including current feature limits.

The bundled language manual uses the explicit chapter list in
`src/docgen/content.rs`. When adding a guide, register its Markdown there so
`rill docs --language` includes it. The compiler embeds those sources, so rebuild
Rill after changing a chapter. The inclusion list keeps historical reports and
design proposals distinct from the current language guide.

Keep established filenames, section headings and declaration names when their
meaning stays the same. They define document and fragment addresses used by
bookmarks and search results. When moving material, update its incoming links
and verify the generated site. Prefer relative links between repository guides;
the generator rewrites supported local targets to relative HTML addresses.
External links should point directly to the source they support.

See [the docs command](docs-command.md) for rendering rules, project API pages,
address escaping, publication ownership and corpus limits.

## Build and check locally

From the repository root:

```sh
cargo run --locked --release --no-default-features -- docs --language -o target/site
python3 scripts/check-docs-links.py target/site
node scripts/test-docs-search.cjs
```

CI selects Rust 1.99.1, Python 3.13 and Node 22. The documentation command does
not run source programs or native dependency tools, and disabling default
features avoids the optional C-import frontend. SDL, Raylib, Slang and GPU
drivers are not prerequisites for this documentation job.

The Python check follows generated local links, checks unique HTML IDs and
verifies every serialized search address. It does not fetch external websites.
The Node check tests the shipped search UI logic, including ranking, safe text,
query URLs, pagination, Unicode, keyboard behavior and local-file navigation.
It takes no site-directory argument. The generated search index's addresses
are checked by the Python command.

Open `target/site/index.html` directly, or serve it locally:

```sh
python3 -m http.server 8000 --directory target/site
```

Review the navigation, a guide with code, the standard-library catalogue and a
search result with a section fragment. The generated site's relative links also
support a repository subdirectory such as `/rill/`; no root-domain rewrite or
application server is required.

## Pull requests and branch builds

[The documentation workflow](../.github/workflows/docs.yml) runs on relevant
pull requests targeting `v1` and on relevant pushes to `v1`. Its path filters
include Markdown, standard-library and compiler sources, Cargo inputs, the
documentation checkers and the workflow itself. A manually started run always
builds and checks the selected ref; its `deploy` input defaults to `false`.

Every successful build uploads `rill-docs-site`, retained for 14 days. Download
that artifact from the Actions run, extract it and open `index.html` to review
the exact generated output. It is a downloadable review site, not a public
preview deployment. Pull request builds do not deploy Pages and receive only
repository read permission.

To require `Build and check documentation` in branch protection, first remove
the workflow's pull-request path filter so every pull request targeting `v1`
produces that check. The push path filter can remain. With the current filters,
keep the documentation check optional for changes outside its input set.
GitHub documents that a skipped path-filtered required workflow can remain
pending in its [workflow syntax reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpushpull_requestpull_request_targetpathspaths-ignore).

## Enable GitHub Pages later

This workflow prepares the publishing path. Adding it does not change remote
repository settings or publish a site. A repository administrator must select
**Settings > Pages > Build and deployment > Source > GitHub Actions**. Configure
the `github-pages` environment to permit `v1` deployments and add any desired
review requirement. GitHub describes this setup in its
[custom Pages workflow guide](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).

After that setup, a successful relevant push to `v1` builds, checks and deploys
the manual. A manual run publishes only when the selected ref is `v1` and
`deploy=true`. The deploy job depends on the successful build, serializes Pages
updates and alone receives `pages: write` and `id-token: write`. The Pages URL
appears on its `github-pages` deployment environment. `main`, pull requests and
other manually selected refs remain artifact-only.

The Pages configuration step has `enablement: false`. It reads the configured
site and does not attempt to enable Pages with a privileged token. No personal
access token or separate deployment branch is needed. Before Pages is enabled,
the build and review artifact can succeed while a requested deployment fails
with a configuration error.

GitHub requires the workflow file on the repository's default branch to expose
manual dispatch in the UI. The current `v1` implementation and older `main`
have separate histories. Register the workflow on the chosen default branch
through an explicit repository decision; this setup does not merge those
histories or change the default branch. Automatic build filters target `v1`
because the older `main` does not contain the current documentation compiler.
Expand those branch filters only after another branch carries that compiler
and its documentation sources. A manually selected ref also needs those files
for its build to succeed.
GitHub documents the registration condition under
[workflow_dispatch](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch).

## Keep generated output reproducible

Review Markdown, API comments and generator changes together. Commit source
changes and check scripts, while leaving `target/site` and preview downloads
untracked. A compiler release carries the manual embedded at that checkpoint;
the Pages site follows the latest successfully deployed `v1` source. State that
difference when linking readers to experimental features or older releases.

Run the local commands before claiming a documentation change passes. A local
build is evidence for generation, links and search logic. It is not evidence
that a GitHub workflow ran or a Pages deployment succeeded.
