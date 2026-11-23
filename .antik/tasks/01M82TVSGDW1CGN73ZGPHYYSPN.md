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

- [ ] Establish concrete type for Iroh integration
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

- 2026-11-23T00:31:26.393Z · Discussion is outdated.
