//! Wire types shared between the JustSync client and relay server.

pub mod error;
pub mod framing;
// pub mod envelope;

pub const PROTOCOL_VERSION: u16 = 1;

pub const MAX_FRAME: usize = 512; // pre-auth stream: keep it tiny
pub const MAX_TOKEN: usize = 256;

/// Application-layer protocol negotiation (ALPN) identifiers advertised during
/// the QUIC/TLS handshake. The client and relay must agree on at least one of
/// these to establish a connection.
#[must_use]
pub fn alpn() -> Vec<Vec<u8>> {
    vec![b"justsync".to_vec()]
}

/// Shared by both sides so they can never disagree on the byte layout.
pub fn signing_input(
    nonce: &[u8; 32],
    exporter: &[u8; 32],
    session_id: &[u8; 16],
    identity_pk: &[u8; 32],
    token: &[u8],
) -> Vec<u8> {
    let mut m = Vec::with_capacity(128 + token.len());
    m.extend_from_slice(b"justsync/1 create"); // domain separation
    m.extend_from_slice(nonce);
    m.extend_from_slice(exporter);
    m.extend_from_slice(session_id);
    m.extend_from_slice(identity_pk);
    m.extend_from_slice(&(token.len() as u32).to_le_bytes()); // length-prefix the variable field
    m.extend_from_slice(token);
    m
}
