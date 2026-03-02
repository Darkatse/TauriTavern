use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn random_base64url(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn sha256_base64url(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn sign_request(key: &[u8], method: &str, path: &str, body: &[u8]) -> String {
    let body_hash = sha256_base64url(body);
    let canonical = format!("{}\n{}\n{}", method.to_ascii_uppercase(), path, body_hash);

    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC key length is not accepted by Sha256");
    mac.update(canonical.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn verify_request_signature(
    key: &[u8],
    method: &str,
    path: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    sign_request(key, method, path, body) == signature
}

pub fn derive_pair_secret(pair_code: &str, source_device_id: &str, target_device_id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(pair_code.as_bytes())
        .expect("HMAC key length is not accepted by Sha256");
    mac.update(b"TT-LANSYNC-PAIR-SECRET");
    mac.update(source_device_id.as_bytes());
    mac.update(target_device_id.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}
