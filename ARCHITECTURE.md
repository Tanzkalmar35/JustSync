# PROJECT MANIFESTO: The "LSP Proxy" Collaboration Tool
**Status:** Locked In (3rd and Final Rewrite)
**Priority Hierarchy:** Performance > Logic > Pragmatism

---

## 1. The Core Philosophy
We are not building a synchronized file watcher. File watchers are reactive, laggy, and unaware of user intent.
We are building a **Real-Time Language Server Proxy**.
We sit directly on the wire between the Code Editor and the Language Intelligence. We capture keystrokes in memory, milliseconds after they happen, not seconds after they are saved to disk.

## 2. The Tech Stack
We chose this stack not because it is trendy, but because it is the only combination that satisfies our performance and safety constraints.

* **Language:** **Rust**.
    * *Why:* We need C-level performance without C-level segfaults. We need `async/await` for networking without the latency spikes of Go's Garbage Collector.
* **Architecture Pattern:** **LSP Man-in-the-Middle (Proxy)**.
    * *Why:* To achieve editor-agnosticism without writing plugins for 10 different IDEs. We pretend to be the Language Server.
* **State Management:** **CRDT (Conflict-free Replicated Data Type)**.
    * *Library:* `diamond-types` (or `y-crdt`).
    * *Why:* Operational Transformation (OT) requires a central server. CRDTs allow decentralized, eventual consistency. `diamond-types` is currently the performance king of text CRDTs.
* **Text Structure:** **Rope** (`ropey`).
    * *Why:* Standard strings are $O(n)$ for insertion. Ropes are $O(\log n)$. Essential for mapping LSP line/col positions to CRDT byte offsets.
* **Networking:** **QUIC** (`quinn`).
    * *Why:* TCP suffers from Head-of-Line blocking. QUIC allows multiplexed streams (Sync, Cursors, Chat) over UDP.

---

## 3. The Architecture: "The Dumb Pipe & The Smart Sidecar"

### The Old Way (REJECTED)
`Editor -> writes to Disk -> File Watcher wakes up -> Reads File -> Diffs -> Syncs`
* *Verdict:* Too slow. IO bound. Loss of intent.

### The New Way (APPROVED)
The Editor starts our binary thinking it is the Language Server (e.g., `gopls`).

**Data Flow:**
1.  **Intercept:** Editor writes to `TD_STDIN` (Our Standard Input).
2.  **Parse:** We intercept `textDocument/didChange` (User typed 'x').
3.  **Fork:**
    * **Path A (Passthrough):** We forward the raw message to the *Real* Language Server's `STDIN`. The user gets their autocomplete/errors instantly.
    * **Path B (Sync):** We apply the delta to our local CRDT. We compress the operation and blast it via QUIC to connected peers.
4.  **Receive:**
    * Peer sends an Op.
    * We apply Op to CRDT.
    * We send `workspace/applyEdit` back to the Editor to update the screen.

---

## 4. The Implementation Roadmap

### Phase 1: The Transparent Proxy (Pragmatism Check)
**Goal:** Create a "dumb" Rust binary that sits between VS Code and `gopls` without breaking anything.
* Spawn `gopls` as a child process.
* Pipe `Self::Stdin` -> `Child::Stdin`.
* Pipe `Child::Stdout` -> `Self::Stdout`.
* Pipe `Child::Stderr` -> `Self::Stderr`.
* *Success Criteria:* You can code in VS Code, get autocomplete, and not notice our binary is running.

### Phase 2: The Observer (Logic Check)
**Goal:** Peek inside the stream without corrupting it.
* Implement a header-aware reader (LSP uses `Content-Length` headers).
* Parse the JSON body using `serde_json`.
* Identify `textDocument/didChange`.
* Log the changes to a debug file.

### Phase 3: The Engine (Performance Check)
**Goal:** internalize state.
* Spin up the CRDT.
* Map LSP `Range` (Line: 1, Col: 5) to CRDT `Index` (Offset: 56).
* Apply local changes to the CRDT.

### Phase 4: The Network
**Goal:** Connect the worlds.
* Implement QUIC transport.
* Broadcast operations.

---

## 5. The AI Role (Gemini)
**Role:** Senior Systems Architect / Lead Engineer.
**Directives:**
1.  **No Mercy:** If the code is inefficient, I will say so.
2.  **First Principles:** I will explain *why* something works at the hardware/OS level, not just give you a snippet.
3.  **Focus:** I will stop you if you drift into "nice to have" features before the core engine is rock solid.

**"We do it right, or we don't do it at all."**
