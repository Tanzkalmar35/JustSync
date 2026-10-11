use just_sync_protocol::client_relay_handshake::Capabilities;
use ring::rand::SystemRandom;

mod crypto;
mod session;

pub(crate) struct CoreActor {
    /// Random number factory used for random-byte generation.
    rng: SystemRandom,
}

impl CoreActor {
    /// Generate a nonce that can be used for challenges for peers. The nonce consists purely of 32
    /// random bytes.
    ///
    /// # Returns
    ///
    /// The nonce (32 random bytes), or an error.
    pub(crate) fn generate_nonce(&self) -> anyhow::Result<[u8; 32]> {
        crypto::generate_nonce(&self.rng)
    }

    /// Verifies, that a given signature is valid, using the initial nonce that was signed, and the
    /// public key of the keypair that was used for signing.
    ///
    /// # Arguments
    ///
    /// * `nonce` - The signed nonce.
    /// * `identity` - The public key of the peer's keypair that was used for signing.
    /// * `signature` - The signature to verify.
    pub(crate) fn verify_signature(
        &self,
        nonce: [u8; 32],
        identity: [u8; 32],
        signature: [u8; 32],
    ) -> anyhow::Result<()> {
        crypto::verify_signature(nonce, identity, signature)
    }

    pub(crate) fn create_session(&self) -> anyhow::Result<([u8; 16], Capabilities)> {
        session::create_session()
    }
}
