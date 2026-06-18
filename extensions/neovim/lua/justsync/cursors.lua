local M = {}

-- Clamp a row/col position of a cursor to valid bounds for the given buffer.
-- Any incoming position is guaranteed to be a valid position in the given buffer
-- after parsing it.
local function clamp_position(bufnr, row, col)
    local line_count = vim.api.nvim_buf_line_count(bufnr)
    row = math.max(0, math.min(row, line_count - 1))

    local line = vim.api.nvim_buf_get_lines(bufnr, row, row + 1, false)[1] or ""
    col = math.max(0, math.min(col, #line))

    return row, col
end

function dump(o)
   if type(o) == 'table' then
      local s = '{ '
      for k,v in pairs(o) do
         if type(k) ~= 'number' then k = '"'..k..'"' end
         s = s .. '['..k..'] = ' .. dump(v) .. ','
      end
      return s .. '} '
   else
      return tostring(o)
   end
end

function M.handle_remote_cursor(err, result, ctx, _config)
    print("Result: ", dump(result))
    if err or not result then return end
    if not result.uri or not result.position or not result.agent_id then return end

    local state = require("justsync.state")
    local highlights = require("justsync.highlights")

    local agent_id = result.agent_id
    local uri = result.uri:match("^%w+://") and result.uri or vim.uri_from_fname(result.uri)
    local bufnr = vim.uri_to_bufnr(uri)

    if not vim.api.nvim_buf_is_loaded(bufnr) then return end

    local row, col = clamp_position(bufnr, result.position.line, result.position.character)

    local peer = state.get_peer(agent_id)
    local hl_group

    if peer then
        hl_group = peer.hl_group

        if peer.bufnr ~= bufnr then
            pcall(vim.api.nvim_buf_del_extmark, bufnr, state.ns, peer.mark_id)
            peer = nil
        end
    else
        hl_group = highlights.aqcuire()
    end

    local opts = {
        end_col = math.min(col + 1, #(vim.api.nvim_buf_get_lines(bufnr, row, row + 1, false)[1] or "") + 1),
        hl_group = hl_group,
        hl_mode = "replace",
        priority = 1000,
        virt_text = { { "┃", hl_group } },
        virt_text_pos = "orverlay,"
    }

    if peer and peer.bufnr == bufnr then
        opts.id = peer.mark_id
    end

    local ok, mark_id = pcall(vim.api.nvim_buf_set_extmark, bufnr, state.ns, row, col, opts)
    if not ok then return end

    state.set_peer(agent_id, { mark_id = mark_id, bufnr = bufnr, hl_group = hl_group })
end

function M.handle_peer_left(err, result, _ctx, _config)
    if err or not result or not result.agent_id then return end
    M.remove_cursor(result.agent_id)
end

function M.remove_cursor(agent_id)
    local state = require("justsync.state")
    local highlights = require("justsync.highlights")

    local peer = state.remove_peer(agent_id)
    if not peer then return end

    pcall(vim.api.nvim_buf_del_extmark, peer.bufnr, state.ns, peer.mark_id)
    highlights.release(peer.hl_group)
end

function M.clear_all()
    local state = require("justsync.state")

    for agent_id in pairs(state.peers) do
        M.remove_cursor(agent_id)
    end
end

return M
