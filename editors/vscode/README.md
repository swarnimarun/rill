# Rill for VS Code

This extension registers `.rill` files, starts `rill lsp` over stdio and supplies
TextMate highlighting. The separate Tree-sitter grammar is used by editors
with Tree-sitter support; VS Code does not load its query files directly.

Build the compiler from the repository root:

```sh
cargo build --release --locked
export PATH="$PWD/target/release:$PATH"
```

Build and install the local extension with Node.js 22 or later:

```sh
cd editors/vscode
npm ci
npm test
npm run package -- --out /tmp/rill-language.vsix
code --install-extension /tmp/rill-language.vsix
```

Alternatively, set `rill.serverPath` in VS Code settings to the compiler
executable. This is a single path, not a shell command. Arguments are always
`["lsp"]`. Reload the VS Code window after changing the path. Workspace trust
is required before starting the language server.

Open `examples/sdl3-direct/main.rill` and its `sdl.rill` binding module. Syntax
highlighting loads immediately, and the server provides its advertised editor
features. Compiler/header/link validation still belongs to `rill check` and
`rill build`; the language server does not execute native dependency tools.

The Output panel's **Rill Language Server** channel contains client/server
startup diagnostics. If startup fails, confirm `rill lsp` resolves to this
checkout's rebuilt compiler and that `rill.serverPath` does not include arguments.

The automated tests exercise extension startup/shutdown with a mocked VS Code
host and tokenize the grammar using VS Code's actual TextMate/Oniguruma
libraries. They do not launch a graphical VS Code Extension Host.

The lockfile pins the language client to 11.0.0 and overrides its transitive
`minimatch` dependency to patched 11.0.1 for
[GHSA-g3hv-75wg-m249](https://github.com/advisories/GHSA-g3hv-75wg-m249).
The override retains the constructor and named-function APIs used by the client
and packager; both are covered by tests. The patched dependency supports Node
18, 20 and 22 or later; the packaging instructions use Node 22 or later.
