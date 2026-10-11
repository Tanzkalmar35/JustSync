use anyhow::anyhow;
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{ED25519, UnparsedPublicKey},
};

/// Generate a nonce that can be used for challenges for peers. The nonce consists purely of 32
/// random bytes.
///
/// # Arguments
///
/// * `rng` - The random provider.
///
/// # Returns
///
/// The nonce (32 random bytes), or an error.
pub(super) fn generate_nonce(rng: &SystemRandom) -> anyhow::Result<[u8; 32]>{
    let mut nonce = [0u8; 32];
    rng.fill(&mut nonce).map_err(|e| anyhow!(e.to_string()))?;
    Ok(nonce)
}

/// Verifies, that a given signature is valid, using the initial nonce that was signed, and the
/// public key of the keypair that was used for signing.
///
/// # Arguments
///
/// * `nonce` - The signed nonce.
/// * `identity` - The public key of the peer's keypair that was used for signing.
/// * `signature` - The signature to verify.
pub(super) fn verify_signature(
    nonce: [u8; 32],
    identity: [u8; 32],
    signature: [u8; 32],
) -> anyhow::Result<()> {
    UnparsedPublicKey::new(&ED25519, identity)
        .verify(&nonce, &signature)
        .map_err(|e| anyhow!(e.to_string()))
}
