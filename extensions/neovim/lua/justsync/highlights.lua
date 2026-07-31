local M = {}

local COLORS = {
    "#eb6f92",
    "#31748f",
    "#9ccfd8",
    "#f6c177",
    "#c4a7e7",
    "#ebbcba",
}

function M.setup()
    local state = require("justsync.state")
    state.palette = {}

    for i, color in ipairs(COLORS) do
        local group = "JustSyncCursor" .. i
        vim.api.nvim_set_hl(0, group, { bg = color, fg = "#1a1a1a", default = true })
        state.palette[i] = group
    end
end

function M.aqcuire()
    local state = require("justsync.state")

    for i, group in ipairs(state.palette) do
        if not state.palette_in_use[i] then
            state.palette_in_use[i] = true
            return group
        end
    end

    local idx = (vim.tbl_count(state.peers) % #state.palette) + 1
    return state.palette[idx]
end

function M.release(hl_group)
    local state = require("justsync.state")
    for i, group in ipairs(state.palette) do
        if group == hl_group then
            state.palette_in_use[i] = nil
            return
        end
    end
end

return M
