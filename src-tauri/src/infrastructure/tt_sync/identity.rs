use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey};

use crate::domain::errors::DomainError;

pub fn device_pubkey_b64url(ed25519_seed_b64url: &str) -> Result<String, DomainError> {
    let seed = decode_seed(ed25519_seed_b64url)?;
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();
    Ok(URL_SAFE_NO_PAD.encode(verifying.to_bytes()))
}

pub fn sign_ed25519_b64url(
    ed25519_seed_b64url: &str,
    message: &[u8],
) -> Result<String, DomainError> {
    let seed = decode_seed(ed25519_seed_b64url)?;
    let signing = SigningKey::from_bytes(&seed);
    let signature: Signature = signing.sign(message);
    Ok(URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn decode_seed(ed25519_seed_b64url: &str) -> Result<[u8; 32], DomainError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(ed25519_seed_b64url.as_bytes())
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;

    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| DomainError::InvalidData("ed25519 seed must be 32 bytes".to_string()))?;

    Ok(seed)
}
