# Architectural Notes for JustSync

## V2 Architecture: CRDTs with Zero-Knowledge Host

This document outlines the architecture for JustSync based on Conflict-Free Replicated Data Types (CRDTs) and a privacy-preserving, zero-knowledge host model.

### Core Principles

1.  **Zero-Knowledge Host**: The central server acts as a simple, stateless relay. It is responsible for managing user sessions (rooms) and broadcasting encrypted messages. It has no access to the content of the files being synced.

2.  **Client-Side State**: All logic and state related to file content is managed exclusively on the client side. Each peer in a session maintains its own in-memory map of CRDT documents (`map[string]*y.Doc`).

3.  **CRDT Library**: The project will use the `skyterra/y-crdt` library, a Go port of the Yjs CRDT framework.

4.  **End-to-End Encryption**: All CRDT update payloads must be encrypted by the sending client and decrypted by the receiving clients. The host only ever handles encrypted data blobs.

### Synchronization Flow

#### Initial Sync (Bootstrapping)

The initial population of a client's state is handled via a peer-to-peer transfer to maintain the zero-knowledge principle.

1.  **Genesis Peer**: The first user to start a session becomes the "genesis peer." Their client creates the initial `YDoc`s by reading the project files from their local disk.
2.  **New Peer Joins**: Subsequent peers connect to the relay and are introduced to a peer already in the session.
3.  **P2P State Clone**: The existing peer serializes its `YDoc` state (using a function like `EncodeStateAsUpdate`) and sends it directly to the new peer. The new peer constructs its `YDoc`s from this state, creating a perfect clone. The new peer then saves the rendered text to its local files.

#### Ongoing Sync (File Edits)

When a user saves a file, the change is broadcasted via the relay.

1.  The client's `RequestSync` handler is triggered.
2.  It looks up the corresponding in-memory `YDoc` for that file.
3.  It performs a `diff` between the content on disk and the content of the `YDoc`.
4.  It applies the changes from the diff to the `YDoc` (`text.Insert`, `text.Delete`).
5.  It calls the CRDT library to encode the update into a binary payload (e.g., `y_crdt.EncodeStateAsUpdate(doc, []uint8{})`).
6.  This payload is encrypted and sent to the relay host, which broadcasts it to all other peers.

### Persistence

To prevent the loss of CRDT history on restart, each client persists its `SyncedFiles` map to a local snapshot file.

-   The snapshot must store the full CRDT state (the `crdt_state` byte array), not the rendered plain text.
-   On startup, the client "hydrates" its in-memory `SyncedFiles` map from this snapshot file.

## Project Status & Next Steps

### Completed

-   `snapshot/snapshot.proto` has been refactored to use `FileUpdate` and simplified `InitialSyncFile` messages.
-   The `RequestSync` function in `api/sync.go` has been updated with the logic for *generating* `FileUpdate` messages.

### Immediate Next Step

-   **Initialize `service.SyncedFiles` map**: This is the current blocker. The map is accessed by `RequestSync` but is never populated. Logic must be added to create the `YDoc`s for each file when a client session starts (either from the file system for the genesis peer or from a snapshot).

### Subsequent Steps

1.  **Handle Incoming Updates**: Replace the old `ApplyFileDelta` function with a new `ApplyFileUpdate` function that can receive `FileUpdate` messages, decrypt them, and apply the CRDT payload to the local `YDoc`.
2.  **Fix Persistence**: Refactor the snapshot saving/loading logic to handle the `crdt_state` instead of plain text.
3.  **Implement P2P Initial Sync**: Design and build the mechanism for peers to find each other and transfer the initial state.
4.  **Implement E2EE Layer**: Add the encryption/decryption logic for all `FileUpdate` messages.
