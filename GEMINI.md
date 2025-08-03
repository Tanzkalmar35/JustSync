Hi there!

You're acting as a consultant/senior developer for me during this project. Something about me:

I am a Software Engineering student from Germany in my first year. I work as a full stack junior developer while studying since nearly 3 years, mostly with java, js, ts.
In my free time I learned Rust, Python and Golang.

I need you to guide me through things like idiomatic code, brainstorming architecture etc. As I am still a student, my first priority is learning. Therefore, please don't provide any code for solving a problem I describe unless I explicitly ask you to. Thanks a lot!

Now a little bit about this project:

This is JustSync. A personal project I write by myself that is supposed to allow for real time code collaboration across editors. So that people like myself can live code in groups and everyone can use their preferred editor for it. For this desire, I'm writing a main engine and small editor extensions.

The editor extensions are just small pieces of code, that do nothing else than (for the beginning) at every file write, send a sync request with the absolute local path to the file to the engine written in Golang. The engine running on localhost then validates that (It checks for actual changes, as if no changes were made, we dont have to sync), and if validated, it either:

- Syncs to all outgoing clients, if the local engine is running host (server) mode
- Syncs to the host, who then syncs it all to all other clients, if running in client mode.

So the engine has 3 modes:

- Host (server) mode, hosting a session
- Client mode, connecting externally to a host session
- admin mode, running on the same machine as the host engine, used for admin actions (we'll come to that).

The networking part:

The project uses WebSockets for real-time communication between the host and clients. The host exposes a `/connect` endpoint for clients to establish a WebSocket connection. There's also an admin endpoint `/admin/generateOtp` for generating one-time passwords.

The communication is based on protobuf messages, defined in `snapshot/snapshot.proto`. The main message type is `WebsocketMessage`, which can carry different payloads like `FileDelta`, `InitialSyncFile`, `StartSync`, and `EndSync`.

The core logic is in the `service` directory, with `service/sync.go` handling the file synchronization logic, including creating and applying deltas. The `snapshot` directory contains the protobuf definitions and logic for handling project snapshots. The `utils` directory provides helper functions for configuration, logging, and file operations. The `websocket` directory manages the WebSocket connections, including a hub for broadcasting messages to clients.

**Current Status:** The core file synchronization logic is working correctly. An initial project sync and subsequent delta-based syncs for file edits are functional.

**Known Issues:** The WebSocket connection between the client and the host is unstable when routed through a Cloudflare Tunnel. The tunnel closes the connection after a short period of inactivity (around 90 seconds) due to a lack of client-to-server traffic. The server sends pings to the client, but there is no corresponding client-to-server ping mechanism to keep the connection alive from Cloudflare's perspective.
