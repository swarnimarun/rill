local root = assert(vim.env.RILL_REPO)
vim.opt.runtimepath:append(vim.fs.joinpath(root, 'editors', 'neovim'))
require('rill').setup({ lsp = false, parser_path = assert(vim.env.RILL_PARSER) })

local source = vim.fs.joinpath(root, 'examples', 'sdl3-direct', 'main.rill')
vim.cmd.edit(vim.fn.fnameescape(source))
assert(vim.bo.filetype == 'rill', 'filetype detection failed')
local parser = vim.treesitter.get_parser(0, 'rill')
local tree = parser:parse()[1]
assert(not tree:root():has_error(), 'Tree-sitter rejected SDL3 example')
local query = assert(vim.treesitter.query.get('rill', 'highlights'))
local captures = {}
for capture, node in query:iter_captures(tree:root(), 0, 0, -1) do
  local name = query.captures[capture]
  captures[name] = true
  assert(vim.treesitter.get_node_text(node, 0) ~= '')
end
assert(captures['function'], 'missing function highlight capture')
assert(captures['operator'], 'missing operator highlight capture')
assert(captures['comment'], 'missing comment highlight capture')

vim.bo.modified = false
vim.cmd.enew()
vim.api.nvim_buf_set_name(0, vim.fs.joinpath(assert(vim.env.RILL_TEST_WORKSPACE), 'main.rill'))
vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'fn main() -> bool { not false }' })
vim.bo.filetype = 'rill'
parser = vim.treesitter.get_parser(0, 'rill')
assert(not parser:parse()[1]:root():has_error())
vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'fn main() -> bool { not false' })
assert(parser:parse()[1]:root():has_error(), 'incremental parse missed removed brace')
vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'fn main() -> bool { not false }' })
assert(not parser:parse()[1]:root():has_error(), 'incremental parse did not recover')

if vim.env.RILL_BIN then
  require('rill').setup({
    cmd = { vim.env.RILL_BIN, 'lsp' },
    parser_path = assert(vim.env.RILL_PARSER),
  })
  local function client()
    for _, running in ipairs(vim.lsp.get_clients({ bufnr = 0, name = 'rill' })) do
      if running.initialized then
        return running
      end
    end
  end
  assert(vim.wait(10000, function() return client() ~= nil end), 'Rill LSP did not attach')
  local running = assert(client())
  local result = running:request_sync('textDocument/documentSymbol', {
    textDocument = { uri = vim.uri_from_bufnr(0) },
  }, 5000, 0)
  assert(result and not result.err and result.result[1].name == 'main', 'document symbols failed')
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'fn main() { let broken = ; }' })
  assert(vim.wait(10000, function() return #vim.diagnostic.get(0) > 0 end), 'missing live syntax diagnostic')
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'fn main() { 42 }' })
  assert(vim.wait(10000, function() return #vim.diagnostic.get(0) == 0 end), 'diagnostic did not clear')
  assert(vim.fn.filereadable(vim.api.nvim_buf_get_name(0)) == 0, 'server wrote unsaved source')
  vim.lsp.stop_client(running.id)
  assert(vim.wait(5000, function() return running:is_stopped() end), 'server did not shut down')
end

vim.bo.modified = false
print('Rill Neovim smoke passed')
vim.cmd.qa()
