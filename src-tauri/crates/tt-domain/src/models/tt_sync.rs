use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, Permissions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtSyncIdentity {
    pub device_id: DeviceId,
    pub device_name: String,
    /// base64url(no pad) 32 bytes, used to derive Ed25519 signing key.
    pub ed25519_seed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtSyncPairedServer {
    pub server_device_id: DeviceId,
    pub server_device_name: String,
    pub base_url: String,
    pub spki_sha256: String,
    pub permissions: Permissions,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}
