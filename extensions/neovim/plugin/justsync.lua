if vim.g.loaded_justsync then return end

vim.g.loaded_justsync = true

vim.api.nvim_create_user_command("JustSyncHost", function()
    require("justsync").host()
end, { desc = "Start JustSync in Host mode" })

vim.api.nvim_create_user_command("JustSyncJoin", function()
    require("justsync").join()
end, { desc = "Join a JustSync session" })

vim.api.nvim_create_user_command("JustSyncLog", function()
    require("justsync.ui").open_log()
end, { desc = "Open JustSync LSP log" })
