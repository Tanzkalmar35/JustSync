local M = {}

function M.setup(opts)
  require("justsync.config").setup(opts)
  require("justsync.highlights").setup()
end

function M.host()
  local ui = require("justsync.ui")
  ui.prompt_chain({
    { prompt = "Relay server address: ", required = true },
    { prompt = "Password to use: ",      required = true },
  }, function(res)
    if not res then return end
    require("justsync.rpc").launch_client(
      { "--mode", "host", "--remote-ip", res[1], "--key", res[2] },
      "Host"
    )
  end)
end

function M.join()
  local ui = require("justsync.ui")
  ui.prompt_chain({
    { prompt = "Relay server address: ", required = true },
    { prompt = "Session name: ",         required = true },
    { prompt = "Session password: ",     required = true },
  }, function(res)
    if not res then return end
    require("justsync.rpc").launch_client(
      { "--mode", "peer", "--remote-ip", res[1], "--session-name", res[2], "--key", res[3] },
      "Peer"
    )
  end)
end

return M
