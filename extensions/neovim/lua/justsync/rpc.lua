local M = {}

local function setup_buffer_autocommands(bufnr)
    vim.api.nvim_buf_set_option(bufnr, "autoread", true)
    local group = vim.api.nvim_create_augroup("JustSyncAutoread-" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd({ "CursorHold", "FocusGained", "BufEnter" }, {
        group    = group,
        buffer   = bufnr,
        callback = function() vim.cmd("checktime") end,
    })
end

local function setup_cursor_tracking(client, bufnr)
    local grp = vim.api.nvim_create_augroup("JustSyncCursor-" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI" }, {
        group    = grp,
        buffer   = bufnr,
        callback = function()
            local cursor = vim.api.nvim_win_get_cursor(0)
            client.rpc.notify("$/justsync/cursor", {
                textDocument = { uri = vim.uri_from_bufnr(bufnr) },
                position     = { line = cursor[1] - 1, character = cursor[2] },
            })
        end,
    })
end

function M.launch_client(args, mode_name)
    local config                                        = require("justsync.config").options
    local state                                         = require("justsync.state")
    local ui                                            = require("justsync.ui")
    local cursors                                       = require("justsync.cursors")

    local root_dir                                      = vim.fs.dirname(
        vim.fs.find({ ".git", "Cargo.toml", "package.json" }, { upward = true })[1]
    ) or vim.fn.getcwd()

    local capabilities                                  = vim.lsp.protocol.make_client_capabilities()
    capabilities.textDocument.synchronization.didSave   = true
    capabilities.textDocument.synchronization.willSave  = true
    capabilities.textDocument.synchronization.didChange = true

    local cmd                                           = { config.cmd_path }
    for _, arg in ipairs(args) do table.insert(cmd, arg) end

    vim.lsp.start({
        name         = "just_sync",
        cmd          = cmd,
        root_dir     = root_dir,
        capabilities = capabilities,
        flags        = { debounce_text_changes = 150 },
        handlers     = {
            ["$/justsync/remoteCursor"]   = cursors.handle_remote_cursor,
            ["$/justsync/sessionCreated"] = function(_, res)
                vim.notify(res.name)
            end,
            ["window/showMessage"]        = function(_, result)
                if result then ui.status_msg(result.message, result.type == 1) end
            end,
            ["window/logMessage"]         = function(_, result)
                if result and result.message:find("Token") then
                    ui.status_msg(result.message)
                end
            end,
        },
        on_attach    = function(client, bufnr)
            setup_buffer_autocommands(bufnr)
            setup_cursor_tracking(client, bufnr)
            ui.status_msg("JustSync attached (" .. mode_name .. ")")
        end,
    })

    if not state.autocmd_registered then
        local grp = vim.api.nvim_create_augroup("JustSyncAutoAttach", { clear = true })
        vim.api.nvim_create_autocmd("BufEnter", {
            group    = grp,
            pattern  = "*",
            callback = function(ev)
                local clients = vim.lsp.get_clients({ name = "just_sync" })
                if #clients > 0 then
                    vim.lsp.buf_attach_client(ev.buf, clients[1].id)
                end
            end,
        })
        state.autocmd_registered = true
    end
end

return M
