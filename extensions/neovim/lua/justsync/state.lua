local M = {}

M.ns = vim.api.nvim_create_namespace("justsync_cursor")

M.peers = {}

M.palette = {}

M.palette_in_use = {}

function M.get_peer(agent_id)
    return M.peers[agent_id]
end

function M.set_peer(agent_id, entry)
    M.peers[agent_id] = entry
end

function M.remove_peer(agent_id)
    local entry = M.peers[agent_id]
    M.peers[agent_id] = nil
    return entry
end

return M
