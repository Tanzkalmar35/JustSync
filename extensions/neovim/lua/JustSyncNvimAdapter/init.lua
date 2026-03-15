local M = {}

M.config = {
    cmd_path = "justsync", 
    log_level = vim.log.levels.INFO,
}

M.autocmd_registered = false

-- Create namespace for remote cursors
local ns_id = vim.api.nvim_create_namespace('justsync_cursor')
-- Define a default highlight for the remote cursor (Blue background, white text)
vim.api.nvim_set_hl(0, 'JustSyncRemoteCursor', { bg = '#31748f', fg = '#ffffff', default = true })

local function setup_buffer_autocommands(bufnr)
    vim.api.nvim_buf_set_option(bufnr, 'autoread', true)
    local group = vim.api.nvim_create_augroup("JustSyncAutoread-" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd({ "CursorHold", "FocusGained", "BufEnter" }, {
        group = group,
        buffer = bufnr,
        callback = function() 
            vim.cmd("checktime") 
        end
    })
end

local function status_msg(msg, is_error)
    local prefix = "[JustSync] "
    local hl = is_error and "ErrorMsg" or "Question"
    vim.api.nvim_echo({{prefix, "Identifier"}, {msg, hl}}, true, {})
end

local function handle_remote_cursor(err, result, ctx, config)
    if err then return end
    if not result or not result.uri or not result.position then return end

    local raw_uri = result.uri
    local position = result.position
    local uri = raw_uri:match("^%w+://") and raw_uri or vim.uri_from_fname(raw_uri)
    local bufnr = vim.uri_to_bufnr(uri)
    
    if not vim.api.nvim_buf_is_loaded(bufnr) then return end

    vim.api.nvim_buf_clear_namespace(bufnr, ns_id, 0, -1)
    
    -- Draw the remote cursor
    pcall(vim.api.nvim_buf_set_extmark, bufnr, ns_id, position.line, position.character, {
        end_col = position.character + 1,
        hl_group = 'JustSyncRemoteCursor',
        hl_mode = 'replace',
        priority = 1000,
        -- Add a virtual text "cursor" if the character is empty (e.g. end of line)
        virt_text = {{ "┃", "JustSyncRemoteCursor" }},
        virt_text_pos = "overlay",
    })
end

local function scan_log_for_token()
    local log_path = vim.lsp.get_log_path()
    local file = io.open(log_path, "r")
    if not file then return false end
    
    local size = file:seek("end")
    local start_pos = math.max(0, size - 5000)
    file:seek("set", start_pos)
    
    local content = file:read("*a")
    file:close()
    
    if content then
        for line in content:gmatch("[^\r\n]+") do
            if line:find("Token") then
                status_msg("Token Found: " .. line:match("Token.*") or line)
                return true
            end
        end
    end
    return false
end

local function launch_client(args, mode_name)
    local root_dir = vim.fs.dirname(vim.fs.find({'.git', 'Cargo.toml', 'package.json'}, { upward = true })[1])
    if not root_dir then root_dir = vim.fn.getcwd() end

    local capabilities = vim.lsp.protocol.make_client_capabilities()
    capabilities.textDocument.synchronization.didSave = true
    capabilities.textDocument.synchronization.willSave = true
    capabilities.textDocument.synchronization.didChange = true

    local cmd = { M.config.cmd_path }
    for _, arg in ipairs(args) do table.insert(cmd, arg) end

    local client_id = vim.lsp.start({
        name = "justsync",
        cmd = cmd,
        root_dir = root_dir,
        capabilities = capabilities,
        flags = { debounce_text_changes = 150 },
        handlers = {
            ['$/justsync/remoteCursor'] = handle_remote_cursor,
            ['window/showMessage'] = function(_, result) 
                if result then status_msg(result.message, result.type == 1) end
            end,
            ['window/logMessage'] = function(_, result)
                if result and result.message:find("Token") then
                    status_msg(result.message)
                end
            end,
        },
        on_attach = function(client, bufnr)
            setup_buffer_autocommands(bufnr)
            status_msg("JustSync Attached (" .. mode_name .. ")")
            
            if mode_name == "Host" then
                local timer = vim.loop.new_timer()
                local count = 0
                if timer then
                    timer:start(1000, 1000, function() 
                        count = count + 1
                        if count > 20 then timer:close() return end
                        vim.schedule(function()
                            if scan_log_for_token() then timer:close() end
                        end)
                    end)
                end
            end

            -- Setup outbound cursor tracking
            local grp = vim.api.nvim_create_augroup("JustSyncCursor-" .. bufnr, { clear = true })
            vim.api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI" }, {
                group = grp,
                buffer = bufnr,
                callback = function()
                    local cursor = vim.api.nvim_win_get_cursor(0)
                    local line = cursor[1] - 1
                    local char = cursor[2]
                    
                    local params = {
                        textDocument = { uri = vim.uri_from_bufnr(bufnr) },
                        position = { line = line, character = char }
                    }
                    -- Use Colon operator (Client:notify) for 0.12+ compatibility
                    if client.notify then
                        client.rpc.notify('$/justsync/cursor', params)
                    end
                end
            })
        end,
    })

    if not M.autocmd_registered then
        local grp = vim.api.nvim_create_augroup("JustSyncAutoAttach", { clear = true })
        vim.api.nvim_create_autocmd("BufEnter", {
            group = grp,
            pattern = "*",
            callback = function(ev)
                local clients = vim.lsp.get_clients({ name = "justsync" })
                if #clients > 0 then
                    vim.lsp.buf_attach_client(ev.buf, clients[1].id)
                end
            end
        })
        M.autocmd_registered = true
    end
end

function M.host()
    launch_client({ "--mode", "host", "--port", "4444" }, "Host")
end

function M.join_interactive()
    vim.ui.input({ prompt = 'Host IP (default 127.0.0.1): ' }, function(ip)
        if ip == "" or ip == nil then ip = "127.0.0.1" end
        vim.ui.input({ prompt = 'Security Token: ' }, function(token)
            if token == nil or token == "" then
                status_msg("Token is required!", true)
                return
            end
            launch_client({ "--mode", "peer", "--remote-ip", ip, "--token", token }, "Peer")
        end)
    end)
end

function M.setup(opts)
    M.config = vim.tbl_deep_extend("force", M.config, opts or {})
end

return M