# JustSync

[![Status](https://img.shields.io/badge/Status-Alpha%20v0.1.0-orange)]()
[![Language](https://img.shields.io/badge/Language-Rust-red)]()
[![License](https://img.shields.io/badge/License-MIT-blue)]()

**JustSync** is a high-performance, real-time code synchronization tool designed for Neovim and LSP-compliant editors.

It utilizes **CRDTs (Conflict-free Replicated Data Types)** for mathematical consistency and **QUIC** for low-latency transport, ensuring that collaborative editing feels native, even over unreliable networks.

> **⚠️ Alpha Warning:** This software is currently in active development (v0.1.0). While the core synchronization logic is stable, edge cases may still exist. Use with caution on critical data.

---


https://github.com/user-attachments/assets/9f55f365-05b3-486a-89fd-d1e441ab1f36


---

## 🚀 Key Features

* **Conflict-Free Editing:** Powered by [diamond-types](https://github.com/josephg/diamond-types), JustSync merges concurrent edits automatically without conflicts using state-of-the-art CRDTs.
* **Blazing Fast Transport:** Uses **QUIC** (via `quinn`) instead of TCP/WebSockets, reducing head-of-line blocking and latency.
* **Cursor Stability:** Implements an efficient differential update algorithm (`ropey` + custom diffing) to ensure the cursor never jumps or resets during remote updates.
* **Echo-Loop Protection:** Features a robust, timestamp-based "Echo Guard" that intelligently distinguishes between local user input and remote echoes, preventing infinite sync loops.
* **Editor Agnostic Protocol:** Built to interface with any editor that supports the Language Server Protocol (LSP) or standard stdin/stdout text manipulation.

---

### A typical data flow would look something like

```mermaid
sequenceDiagram
    autonumber
    
    box rgb(240, 248, 255) The Peer Node (Source of Truth)
        actor PeerUser as Peer User
        participant PeerEditor as Neovim (Editor)
        participant PeerHandler as handler.rs (stdin/stdout)
        participant PeerCore as core.rs (The Brain)
        participant PeerState as state.rs (CRDT + Rope)
        participant PeerNet as network.rs (QUIC Sender)
    end

    box rgb(255, 245, 245) The Network
        participant Internet as QUIC Stream (UDP)
    end

    box rgb(245, 255, 245) The Host Node (Receiver)
        participant HostNet as network.rs (QUIC Receiver)
        participant HostCore as core.rs (The Brain)
        participant HostState as state.rs (CRDT + Rope)
        participant HostHandler as handler.rs (stdin/stdout)
        participant HostEditor as Neovim (Editor)
        actor HostUser as Host User
    end

    note over PeerUser, PeerEditor: 1. User types 'x'
    PeerUser->>PeerEditor: Types 'x' into buffer

    note over PeerEditor, PeerHandler: 2. LSP Notification
    PeerEditor->>PeerHandler: stdout: {"method": "textDocument/didChange", params: {...}}

    note over PeerHandler, PeerCore: 3. Parse & Channel Send
    PeerHandler->>PeerHandler: Parse JSON -> Rust Struct
    PeerHandler->>PeerCore: channel send: Event::LocalChange { changes }

    note over PeerCore, PeerState: 4. Process Local Change
    PeerCore->>PeerState: doc.apply_local_changes(changes)

    note right of PeerState: A. Update Rope (View)<br/>B. Update CRDT Oplog (Truth)<br/>C. Generate binary patch
    PeerState-->>PeerCore: returns Option<Vec<u8>> (The Patch)

    note over PeerCore, PeerNet: 5. Prepare for Network
    PeerCore->>PeerNet: channel send: NetworkCommand::BroadcastPatch { patch }

    note over PeerNet, Internet: 6. Serialize & Transmit
    PeerNet->>PeerNet: Serialize into WireMessage::Patch
    PeerNet->>Internet: QUIC Stream Write (Frame + Bytes)

    %% --- Crossing the boundary ---

    note over Internet, HostNet: 7. Receive & Deserialize
    Internet->>HostNet: QUIC Stream Read
    HostNet->>HostNet: Deframe & Deserialize WireMessage

    note over HostNet, HostCore: 8. Inbound Event
    HostNet->>HostCore: channel send: Event::RemotePatch { patch }

    note over HostCore, HostState: 9. Process Remote Patch
    HostCore->>HostState: doc.apply_remote_patch(patch)

    note right of HostState: A. Decode patch into Oplog<br/>B. Fast-forward Branch (Checkout)<br/>C. Reconstruct Text & Calc Diff
    HostState-->>HostCore: returns Option<Vec<TextEdit>> (Minimal Diff)

    note over HostCore, HostHandler: 10. Prepare Editor Edits
    HostCore->>HostHandler: channel send: (uri, Vec<TextEdit>)

    note over HostHandler, HostEditor: 11. LSP Request
    HostHandler->>HostHandler: Wrap in "workspace/applyEdit" JSON
    HostHandler->>HostEditor: stdout: Content-Length: ... \r\n\r\n {"method":...}

    note over HostEditor, HostUser: 12. Update UI
    HostEditor->>HostEditor: Apply text edits to buffer
    HostEditor->>HostUser: User sees 'x' appear
```

### The "Echo Guard"
One of the hardest problems in LSP synchronization is the "Echo Loop," where the editor sends back changes the network just applied. JustSync solves this using a Timestamped Content Lock. It verifies if the didChange event matches the expected state within a tight time window, silently dropping echoes while allowing concurrent user edits to pass through.

## 📦 Installation

### Prerequisites

- Rust Toolchain (latest stable)
- Neovim (v0.8+)

### Build from Source

```Bash
git clone [https://github.com/Tanzkalmar35/justsync.git](https://github.com/yourusername/justsync.git)
cd justsync
cargo build --release
```

The binary will be located at ./target/release/justsync.

## 💻 Usage

JustSync is designed to be used directly through your editor of choice via our dedicated extensions.

### Supported Editors
*   **Neovim:** [JustSyncNvimAdapter](https://github.com/Tanzkalmar35/JustSyncNvimAdapter)
*   **VS Code:** [JustSyncVSCode](https://github.com/Tanzkalmar35/justsync-vscode)
*   **IntelliJ IDEA:** [JustSyncIntelliJ](https://github.com/Tanzkalmar35/justsync-jetbrains)

### How to Connect

**1. Start the Session (Host)**
*   **VS Code / IntelliJ:** Click the **Start** button in the extension panel, select **Host**, and copy the generated **Secret Token**.
*   **Neovim:** Run the command `:JustSyncStart`. The token will be displayed in the messages area.

**2. Join a Session (Peer)**
> **⚠️ Important:** Peers must start in an **empty directory**. The initial sync will download the project state from the host.

*   **VS Code / IntelliJ:** Click **Start**, select **Join**, enter the Host's **IP Address**, and paste the **Secret Token**.
*   **Neovim:** Run `:JustSyncJoin`, then follow the prompts to enter the IP and Token.

## 📄 License
This project is licensed under the MIT License.
