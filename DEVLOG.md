# DEVLOG: Real-Time LSP Collaboration

**Current Phase:** Phase 1 (The Transparent Proxy)
**Status:** Code Complete. Awaiting Integration Verification.

---

## 1. What We Built Today
We pivoted from the "File Watcher" architecture to the **LSP Proxy** architecture to prioritize latency and correctness.

* **The Core (`src/proxy.rs`):**
    * Built a `tokio`-based async wrapper.
    * Solved the Ownership problem: We explicitly `.take()` handles from the child process to spawn independent read/write tasks.
    * Implemented full-duplex bridging using `tokio::io::copy` (Non-blocking I/O).
* **The CLI (`src/main.rs`):**
    * Dynamic argument parsing.
    * Safety checks to ensure the binary exits cleanly (Exit Code 1) rather than panicking if arguments are missing.
    * **Current State:** The binary compiles and successfully pipes data for basic CLI tools (tested with `cat`).

---

## 2. IMMEDIATE NEXT STEP: The Neovim Integration
**Do not touch Rust code until this passes.**

We need to confirm that the proxy is invisible to Neovim.

### The Action Plan:
1.  **Build the Binary:**
    ```bash
    cargo build
    # Binary location: ./target/debug/lsp-proxy
    ```

2.  **Modify Neovim Config (`init.lua` / `lsp.lua`):**
    Find your `rust_analyzer` (or `gopls`) setup and inject the proxy:
    ```lua
    require'lspconfig'.rust_analyzer.setup {
      cmd = {
        "/ABSOLUTE/PATH/TO/YOUR/PROJECT/target/debug/lsp-proxy", -- 1. Your wrapper
        "rust-analyzer"                                          -- 2. The real target
      },
      -- keep other settings...
    }
    ```

3.  **The Smoke Test:**
    * Restart Neovim.
    * Open a Rust file (`main.rs`).
    * Run `:LspInfo` -> Check if Client is attached.
    * **Trigger Autocomplete:** Type `std::` -> If a menu appears, **WE WIN.**

---

## 3. Upcoming Roadmap (Phase 2)
Once Neovim works, we stop being a "Dumb Pipe" and start looking at the data.

1.  **Implement Header Parsing:** LSP messages are prefixed with `Content-Length: 123\r\n\r\n`. We need to read this to know how much JSON to read.
2.  **Implement JSON Inspection:** Use `serde_json` to deserialize *only* the method field.
3.  **The Target:** Intercept `textDocument/didChange` and log it to a file.
