local M = {}

function M.check()
  vim.health.start("justsync")

  local config = require("justsync.config").options
  local binary = config.cmd_path or "just_sync"

  if vim.fn.executable(binary) == 1 then
    vim.health.ok("Binary found: " .. binary)
  else
    vim.health.error("Binary not found: " .. binary, {
      "Make sure `" .. binary .. "` is in your PATH",
      "Or set cmd_path in your setup() call",
    })
  end

  if vim.fn.has("nvim-0.8") == 1 then
    vim.health.ok("Neovim version OK")
  else
    vim.health.warn("Neovim 0.8+ recommended")
  end
end

return M
