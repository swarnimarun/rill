local M = {}

local source = debug.getinfo(1, 'S').source:sub(2)
local directory = vim.fs.dirname(vim.fs.dirname(source))

function M.setup(options)
  options = options or {}
  vim.filetype.add({ extension = { rill = 'rill' } })

  if options.highlight ~= false then
    local parser = options.parser_path or vim.fs.joinpath(directory, 'parser', 'rill.so')
    if vim.fn.filereadable(parser) == 1 then
      vim.treesitter.language.add('rill', { path = parser })
      local query = options.query_path
        or vim.fs.joinpath(directory, '..', 'tree-sitter-rill', 'queries', 'highlights.scm')
      vim.treesitter.query.set('rill', 'highlights', table.concat(vim.fn.readfile(query), '\n'))
      local group = vim.api.nvim_create_augroup('RillHighlight', { clear = true })
      vim.api.nvim_create_autocmd('FileType', {
        group = group,
        pattern = 'rill',
        callback = function(event)
          vim.treesitter.start(event.buf, 'rill')
        end,
      })
      for _, buffer in ipairs(vim.api.nvim_list_bufs()) do
        if vim.api.nvim_buf_is_loaded(buffer) and vim.bo[buffer].filetype == 'rill' then
          vim.treesitter.start(buffer, 'rill')
        end
      end
    else
      vim.notify('Rill Tree-sitter parser missing; run editors/neovim/build-parser.py', vim.log.levels.WARN)
    end
  end

  if options.lsp ~= false then
    vim.lsp.config('rill', {
      cmd = options.cmd or { 'rill', 'lsp' },
      filetypes = { 'rill' },
      root_markers = { '.git', 'PLAN.md' },
    })
    vim.lsp.enable('rill')
  end
end

return M
