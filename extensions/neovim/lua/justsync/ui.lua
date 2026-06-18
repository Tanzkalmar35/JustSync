local M = {}

function M.status_msg(msg, is_error)
    local hl = is_error and "ErrorMsg" or "Question"
    vim.api.nvim_echo({ { "[JustSync] ", "Identifier" }, { msg, hl } }, true, {})
end

function M.open_log()
    local path = vim.lsp.get_log_path()
    if vim.fn.filereadable(path) == 1 then
        vim.cmd("tabnew " .. path)
        vim.cmd("normal! G")
    else
        vim.notify("LSP log not found: " .. path, vim.log.levels.ERROR)
    end
end

--- Prompt helper: chains vim.ui.input calls, calls cb(values) or cb(nil) on cancel.
--- @param prompts { prompt: string, required: boolean }[]
--- @param cb fun(results: string[]|nil)
function M.prompt_chain(prompts, cb)
    local results = {}
    local function next(i)
        if i > #prompts then
            cb(results); return
        end
        local p = prompts[i]
        vim.ui.input({ prompt = p.prompt }, function(val)
            if p.required and (val == nil or val == "") then
                M.status_msg(p.prompt:gsub(":.*", "") .. " is required!", true)
                cb(nil)
                return
            end
            results[i] = val
            next(i + 1)
        end)
    end
    next(1)
end

return M
