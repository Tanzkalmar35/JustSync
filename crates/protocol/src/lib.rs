use std::vec;

pub mod relay;
pub mod handshake;

pub fn alpn() -> Vec<Vec<u8>> {
    vec![b"justsync".to_vec()]
}
