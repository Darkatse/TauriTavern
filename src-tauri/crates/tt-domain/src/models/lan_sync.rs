use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, PeerGrant, Permissions};
use ttsync_contract::sync::{OverwritePolicy, SyncMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanServerSettings {
    pub port: u16,
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPreferences {
    pub manual_default_mode: SyncMode,
    #[serde(default)]
    pub overwrite_policy: OverwritePolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub platform: Option<String>,
    pub last_known_address: Option<String>,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncIdentity {
    pub device_id: DeviceId,
    pub device_name: String,
    /// Runtime platform; importing identity data must not change the reported OS.
    #[serde(skip)]
    pub platform: String,
    /// base64url(no pad) 32 bytes, used to derive Ed25519 signing key.
    pub ed25519_seed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPairedDevice {
    pub grant: PeerGrant,
    #[serde(default)]
    pub platform: Option<String>,
    pub base_url: String,
    pub spki_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanPairCompleteRequest {
    pub device_id: DeviceId,
    pub device_name: String,
    #[serde(default)]
    pub device_platform: Option<String>,
    pub device_pubkey: String,
    pub client_base_url: String,
    pub client_spki_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanPairCompleteResponse {
    pub server_device_id: DeviceId,
    pub server_device_name: String,
    #[serde(default)]
    pub server_device_platform: Option<String>,
    pub server_device_pubkey: String,
    pub granted_permissions: Permissions,
}

impl From<LanSyncPairedDevice> for LanSyncPairedDeviceSummary {
    fn from(device: LanSyncPairedDevice) -> Self {
        Self {
            device_id: device.grant.device_id.to_string(),
            device_name: device.grant.device_name,
            platform: device.platform,
            last_known_address: Some(device.base_url),
            paired_at_ms: device.grant.paired_at_ms,
            last_sync_ms: device.grant.last_sync_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncStatus {
    pub device_name: String,
    pub running: bool,
    pub address: Option<String>,
    pub available_addresses: Vec<String>,
    pub port: u16,
    pub sync_mode: SyncMode,
    pub manual_default_mode: SyncMode,
    pub sync_mode_overridden: bool,
    pub overwrite_policy: OverwritePolicy,
}

pub fn validate_device_name(name: &str) -> Result<(), crate::errors::DomainError> {
    if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(crate::errors::DomainError::InvalidData(
            "Device name must contain 1–64 characters without control characters".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairRequestEvent {
    pub request_id: String,
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub peer_ip: String,
}
