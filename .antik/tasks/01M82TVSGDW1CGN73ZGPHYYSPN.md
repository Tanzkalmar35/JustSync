---
antik: 1
id: 01M82TVSGDW1CGN73ZGPHYYSPN
kind: task
project: 01M81PET31EGMVT1DXNS3VCCH5
created: 2026-11-22T23:53:32.429Z
title: Network layer rewrite
---

# Network layer rewrite

- phase: implement
- lane: learn
- scope: 

## Description

In the old version (not in the code anymore), I had a client and a relay crate. The relay was a manual implementation of a QUIC relay server / session manager. In the new rewrite, I will use iroh as that fits naturally due to them using QUIC too, and takes many problems off me. I already started with the rewrite, but I'm far from done.

## Plan

### 1. Define Network Adapter Interface

- [x] Establish concrete type for Iroh integration
  Create a dedicated struct to encapsulate the external network client and active session state.
- [ ] Implement lifecycle management methods
  Define logic to initialize the connection, spawn the dispatch loop, and gracefully terminate resources.

### 2. Integrate Adapter into Core Logic

- [ ] Connect adapter to core messaging channels
  Wire the new implementation into the central logic so it can exchange data via message queues.
- [ ] Update entry point initialization
  Modify the main application flow to instantiate the new adapter and pass control endpoints to core.

### 3. Validate Invitation Model

- [ ] Implement invitation generation logic
  Ensure the system can produce a complete connection token containing all necessary peer information.
- [ ] Verify peer connection flow
  Confirm that a new peer can successfully use the generated token to join an existing session.

### 4. Change extension interfaces accordingly

- [ ] Intellij
- [ ] VsCode
- [ ] NeoVim

## Definition of done

- [ ] The codebase compiles without errors in the continuous integration environment. — manual
- [ ] A running instance of the host application starts successfully and accepts incoming connections. — manual
- [ ] An external peer can connect using a generated invitation token and exchange data with the host. — manual

## Discussion

**You · 2026-11-23T00:09:14.83Z**

what am I missing? Am I being too vague? Any suggestinos?

**Model · 2026-11-23T00:09:26.233Z**

Your plan has a critical gap: you’re implementing `NetworkAdapter` for Iroh but haven’t defined the concrete type (`IrohNetworkAdapter`) or wired it into the existing `core`/`handler` modules. The current `network.rs` stubs out `start`, `shutdown`, and message enums, but no implementation exists yet.

Order:
1. Define `IrohNetworkAdapter` struct with fields for iroh client/session.
2. Implement `NetworkAdapter::start` to initialize an iroh client, create a session (host or peer), and spawn the network loop that dispatches `CoreToNetworkMessage`.
3. Implement `NetworkAdapter::shutdown` to gracefully close the iroh session.
4. Wire `IrohNetworkAdapter` into `core` so it can send/receive messages via the mpsc channels.
5. Update `main` to instantiate `IrohNetworkAdapter` and pass its channel endpoints to `core`.

Definition of done (checkable by another dev):
- [ ] `client/src/internal/network.rs` contains a full `IrohNetworkAdapter` impl with `start`, `shutdown`, and message handling.
- [ ] CI builds and passes on the new network module.
- [ ] Integration test in `client/tests/network_iroh.rs` shows: host starts, peer joins, both send/recv patches and cursors, and shutdown cleans up without leaks.
- [ ] Manual run: `just_sync_client` starts with `--host` or `--join <token>` and connects to a remote session via iroh.

Missing pieces: error handling for iroh init, ALPN negotiation, and how you’ll expose the iroh token in CLI args. Add those before CI green.

**You · 2026-11-23T00:11:29.827Z**

yeah but I'll also switch to an invitation model. That means the user can generate an invitation from the editor, and that needs to include all information another peer needs to connect to us

**Model · 2026-11-23T00:11:40.414Z**

Your plan has a critical gap: you’re implementing `NetworkAdapter` for Iroh but haven’t defined the concrete type (`IrohNetworkAdapter`) or wired it into the existing `core`/`handler` modules. The current `network.rs` stubs out `start`, `shutdown`, and message enums, but no implementation exists yet.

Order:
1. Define `IrohNetworkAdapter` struct with fields for iroh client/session.
2. Implement `NetworkAdapter::start` to initialize an iroh client, create a session (host or peer), and spawn the network loop that dispatches `CoreToNetworkMessage`.
3. Implement `NetworkAdapter::shutdown` to gracefully close the iroh session.
4. Wire `IrohNetworkAdapter` into `core` so it can send/receive messages via the mpsc channels.
5. Update `main` to instantiate `IrohNetworkAdapter` and pass its channel endpoints to `core`.

Definition of done (checkable by another dev):
- [ ] `client/src/internal/network.rs` contains a full `IrohNetworkAdapter` impl with `start`, `shutdown`, and message handling.
- [ ] CI builds and passes on the new network module.
- [ ] Integration test in `client/tests/network_iroh.rs` shows: host starts, peer joins, both send/recv patches and cursors, and shutdown cleans up

## Notes

- 2026-11-26T20:34:32.508Z · Freezing the session during a new peer joining is probably a good idea....
- 2026-11-26T20:34:10.578Z · On peer init, the protocol now is the following:


1. The new peer sends its agent_id
2.1 If local is just a normal peer, only send the own agent_id
2.2 If local is host, send session info (own agent_id and connection params for all peers), and the full state afterwards
3. Insert new peer into broadcast list
- 2026-11-23T23:59:42.314Z · I abstracted the recv and send methods as trait members, and let `IrohPeer` implement both. The traits are called `AsyncReader` and `AsyncSender`, and allow usage like: `peer.send(msg).await` or `peer.recv().await`
- 2026-11-23T00:31:26.393Z · Discussion is outdated.

## Attempts

- 2026-11-24T00:01:55.841Z · How to handle the peer? If I in the case of self being the host wait for the new peer to send the full sync request, I could desync due to msgs coming through while transmission. Maybe remove full sync requests and send that automatically?

## Implement discussion

**You · 2026-11-26T20:05:08.938Z**

okay so currently the accept peer flow is: remote sends agent_id, we send agent_id, or, if host, we send session information (all other session ids, so the new peer can establish connections to each), and after that the full state. Do you think it makes sense to freeze the session for everyone while a peer joins? Because I fear that something could happen while the peer is initiating, and the peer could start out of sync already...

**Model · 2026-11-26T20:05:19.757Z**

The `IrohPeer` abstraction using `AsyncReader`/`AsyncSender` is solid. Regarding your concern about desync during the "full sync" phase: freezing the entire session state isn't necessary if you handle ordering correctly. Instead of waiting for a manual "full sync request," have the host automatically stream the full state (or a snapshot) immediately after accepting the peer's `agent_id`.

To prevent desync while streaming:
1.  **Stream, don't batch:** Send state updates incrementally as they become available or via a pre-calculated snapshot.
2.  **Ack-based progression:** Require the new peer to acknowledge receipt of specific state chunks before you send the next chunk. This ensures the receiver is always caught up before you proceed.
3.  **Remove manual sync triggers:** Eliminate the explicit "full sync request" step. The handshake should be: `Accept` -> `Stream State (with ACKs)` -> `Resume Normal Flow`.

This approach avoids locking the session while guaranteeing consistency. Check your message queue implementation to ensure it handles backpressure if the new peer is slow to acknowledge large chunks.

**You · 2026-11-26T20:14:58.132Z**

I'm not sure how stable that is, because if I stream the state, and another peer sends a change to a file that is not already synced, the new peer would have the change but not the file... I think it's cleaner to do the freeze, as that's beasically risk-free, and costs only a short freeze. Or am I wrong on this?

**Model · 2026-11-26T20:15:04.492Z**

Freezing the entire session state is risky if your data model allows concurrent mutations (e.g., file edits) from existing peers; you'd need to serialize all active connections, which kills throughput. Instead, use an **Ack-based incremental sync** with a specific "catch-up" protocol:

1.  **Handshake**: Host sends `agent_id` + current snapshot hash.
2.  **Delta Stream**: Host streams missing deltas (files/changes) immediately upon accepting the peer.
3.  **Backpressure Handling**: If the new peer lags, pause streaming until it ACKs the last chunk. Existing peers continue normal operation; only the *streaming channel* to the new peer is managed.
4.  **Conflict Resolution**: If a change occurs on an existing peer while syncing, broadcast that delta as part of the stream. The new peer applies updates in order.

**Implementation check**: Ensure your message queue (e.g., `tokio::sync::mpsc` or similar) handles backpressure gracefully so the host doesn't block sending to *other* peers while waiting for the new peer's ACK. Add a timeout to the sync stream to prevent hanging if a peer disconnects mid-sync.
