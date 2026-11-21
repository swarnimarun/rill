# Rill for Neovim 0.11+

This client uses Neovim's built-in LSP and Tree-sitter APIs. It does not require
`nvim-lspconfig` or `nvim-treesitter`. The grammar requires Tree-sitter language ABI 15; this was verified with
Neovim 0.11.5. Check `:lua print(vim.treesitter.language_version)` when using a
different runtime build.

From the repository root:

```sh
cargo build --release --locked
python3 editors/neovim/build-parser.py
export RILL_REPO="$PWD"
```

The parser command needs Python 3 and a C compiler. `--cc` accepts one compiler
executable and `--output` selects a different library path. The built `.so` file
is local build output and is ignored by Git, including on macOS.

Add this to `init.lua`, using the checkout path in `RILL_REPO`:

```lua
local rill_repo = assert(vim.env.RILL_REPO, 'Set RILL_REPO to your Rill checkout')
vim.opt.runtimepath:append(vim.fs.joinpath(rill_repo, 'editors', 'neovim'))
require('rill').setup({
  cmd = { vim.fs.joinpath(rill_repo, 'target', 'release', 'rill'), 'lsp' },
})
```

If `rill` is already on `PATH`, omit `cmd`. `.rill` buffers get the `rill`
filetype; the language server starts through `vim.lsp.config` and
`vim.lsp.enable`. The compiled local parser is loaded using
`vim.treesitter.language.add`, its highlight query is read from the adjacent
`tree-sitter-rill` package, and highlighting starts on Rill buffers.

Use `parser_path` or `query_path` to select a relocated parser or query file.
`highlight = false` disables Tree-sitter setup; `lsp = false` enables only
highlighting. A missing parser produces a warning while the LSP still starts.
The configuration never installs packages or builds the compiler automatically.

Open `examples/sdl3-direct/main.rill`, then check `:checkhealth vim.lsp` and
`:InspectTree`. `K` shows hover, `gO` shows document symbols, and
`<C-x><C-o>` requests completion using Neovim's built-in mappings. Use
`:lua vim.lsp.buf.definition()` for definition navigation and
`:lua vim.lsp.buf.format()` for formatting.

Run the actual headless integration test:

```sh
python3 editors/neovim/test/run.py
RILL_BIN="$PWD/target/release/rill" python3 editors/neovim/test/run.py
```

The first command builds and loads the parser, checks real SDL3 highlighting,
and exercises an incremental edit/error/recovery. The second also starts the
actual server, opens an unsaved buffer, requests symbols, checks diagnostic
publication and clearing, and shuts the server down. Set `NVIM` to select a
different Neovim executable. All test state stays in a temporary directory.

The setup follows the official [Neovim 0.11 LSP API](https://github.com/neovim/neovim/blob/v0.11.5/runtime/doc/lsp.txt)
and [Tree-sitter API](https://github.com/neovim/neovim/blob/v0.11.5/runtime/doc/treesitter.txt).
