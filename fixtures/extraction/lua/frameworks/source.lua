local lapis = require("lapis")
local app = lapis.Application()

app:get("/", function(self)
  return "hi"
end)

app:match("user", "/users/:id", function(self)
  return { render = "user" }
end)

vim.api.nvim_create_user_command("Greet", function(opts)
  print(opts.args)
end, { nargs = 1, desc = "Greet someone" })

vim.api.nvim_create_autocmd({ "BufWritePre" }, {
  pattern = "*.lua",
  callback = function()
    vim.lsp.buf.format()
  end,
})

vim.keymap.set({ "n", "v" }, "<leader>g", "<cmd>Greet<cr>", { desc = "Greet" })

function love.load()
  player = { x = 0, y = 0 }
end

function love.draw()
  love.graphics.print("hi", player.x, player.y)
end

return {
  {
    "nvim-telescope/telescope.nvim",
    cmd = "Telescope",
    dependencies = { "nvim-lua/plenary.nvim" },
    config = function()
      local telescope = require("telescope")
      telescope.setup({})
    end,
  },
}
