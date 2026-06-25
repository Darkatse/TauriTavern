use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::SyncMode;

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncConfig {
    pub port: u16,
    pub sync_mode: SyncMode,
}

impl<'de> Deserialize<'de> for LanSyncConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct LanSyncConfigCompat {
            port: u16,
            #[serde(default)]
            sync_mode: SyncMode,
            #[serde(default)]
            v2_port: Option<u16>,
        }

        let compat = LanSyncConfigCompat::deserialize(deserializer)?;
        Ok(Self {
            port: compat.v2_port.unwrap_or(compat.port),
            sync_mode: compat.sync_mode,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub last_known_address: Option<String>,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncIdentity {
    pub device_id: DeviceId,
    pub device_name: String,
    /// base64url(no pad) 32 bytes, used to derive Ed25519 signing key.
    pub ed25519_seed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPairedDevice {
    pub grant: PeerGrant,
    pub base_url: String,
    pub spki_sha256: String,
}

impl From<LanSyncPairedDevice> for LanSyncPairedDeviceSummary {
    fn from(device: LanSyncPairedDevice) -> Self {
        Self {
            device_id: device.grant.device_id.to_string(),
            device_name: device.grant.device_name,
            last_known_address: Some(device.base_url),
            paired_at_ms: device.grant.paired_at_ms,
            last_sync_ms: device.grant.last_sync_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncStatus {
    pub running: bool,
    pub address: Option<String>,
    pub available_addresses: Vec<String>,
    pub port: u16,
    pub pairing_enabled: bool,
    pub pairing_expires_at_ms: Option<u64>,
    pub sync_mode: SyncMode,
    pub sync_mode_persistent: SyncMode,
    pub sync_mode_overridden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairRequestEvent {
    pub request_id: String,
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub peer_ip: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum LanSyncSyncPhase {
    Scanning,
    Diffing,
    Downloading,
    Deleting,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncSyncProgressEvent {
    pub phase: LanSyncSyncPhase,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncSyncCompletedEvent {
    pub files_total: usize,
    pub bytes_total: u64,
    pub files_deleted: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncSyncErrorEvent {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::LanSyncConfig;
    use ttsync_contract::sync::SyncMode;

    #[test]
    fn config_deserializes_current_file_without_port_drift() {
        let config: LanSyncConfig =
            serde_json::from_str(r#"{"port":55000,"sync_mode":"Mirror"}"#).unwrap();

        assert_eq!(config.port, 55000);
        assert_eq!(config.sync_mode, SyncMode::Mirror);
    }

    #[test]
    fn config_prefers_existing_https_port() {
        let config: LanSyncConfig =
            serde_json::from_str(r#"{"port":55000,"v2_port":56000}"#).unwrap();

        assert_eq!(config.port, 56000);
        assert_eq!(config.sync_mode, SyncMode::Incremental);
    }
}
