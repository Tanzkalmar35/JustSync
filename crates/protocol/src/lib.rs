//! Wire types shared between the JustSync client and relay server.
//!
//! The protocol is split into one module per traffic flow:
//!
//! * [`relay`] - control messages exchanged between a client and the relay server.
//! * [`handshake`] - control messages exchanged between two clients during peer setup.
//! * [`sync`] - messages exchanged between two clients once the peer setup is done.

pub mod client_handshake;
pub mod relay;
pub mod sync;

/// Application-layer protocol negotiation (ALPN) identifiers advertised during
/// the QUIC/TLS handshake. The client and relay must agree on at least one of
/// these to establish a connection.
#[must_use]
pub fn alpn() -> Vec<Vec<u8>> {
    vec![b"justsync".to_vec()]
}
