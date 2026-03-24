use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, Permissions};
use ttsync_contract::sync::SyncPhase;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TtSyncDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone, Serialize)]
pub struct TtSyncProgressEvent {
    pub direction: TtSyncDirection,
    pub phase: SyncPhase,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TtSyncCompletedEvent {
    pub direction: TtSyncDirection,
    pub files_total: usize,
    pub bytes_total: u64,
    pub files_deleted: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TtSyncErrorEvent {
    pub direction: TtSyncDirection,
    pub message: String,
}
