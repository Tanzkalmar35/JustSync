if vim.g.loaded_justsync_nvim_adapter then
  return
end
vim.g.loaded_justsync_nvim_adapter = 1

local adapter = require("JustSyncNvimAdapter")

-- Command: :JustSyncHost
vim.api.nvim_create_user_command("JustSyncHost", function()
    adapter.host()
end, { desc = "Start JustSync in Host mode" })

-- Command: :JustSyncJoin
-- Starts the interactive mode (asking for ip and token)
vim.api.nvim_create_user_command("JustSyncJoin", function()
    adapter.join_interactive()
end, { 
    desc = "Join a JustSync session (Interactive)" 
})

-- Helper for opening the lsp log directly
vim.api.nvim_create_user_command("JustSyncLog", function()
    local log_path = vim.lsp.get_log_path()
    if vim.fn.filereadable(log_path) == 1 then
        vim.cmd("tabnew " .. log_path)
        vim.cmd("normal! G") -- Scroll to bottom
    else
        vim.notify("LSP Log file not found at: " .. log_path, vim.log.levels.ERROR)
    end
end, { desc = "Open JustSync logs directly" })
