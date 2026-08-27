local M = {}

M.defaults = {
    cmd_path  = "just_sync_client",
    log_level = vim.log.levels.INFO,
}

M.options = {}

function M.setup(opts)
    M.options = vim.tbl_deep_extend("force", M.defaults, opts or {})
end

return M
