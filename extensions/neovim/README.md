# JustSyncNvimAdapter

A Neovim adapter for [JustSync](https://github.com/Tanzkalmar35/JustSync). 
It launches the `just_sync` binary as a standard LSP client for real-time collaboration.

## Setup

### Lazy.nvim

```lua
{
    "Tanzkalmar35/JustSyncNvimAdapter",
    opts = {
        -- Optional: Point to your binary if it's not in PATH
        cmd_path = "/home/user/code/justsync/target/release/justsync"
    },
}
