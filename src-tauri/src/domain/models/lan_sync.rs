use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, PeerGrant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanSyncSyncMode {
    Incremental,
    Mirror,
}

impl Default for LanSyncSyncMode {
    fn default() -> Self {
        Self::Incremental
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncConfig {
    pub port: u16,
    pub sync_mode: LanSyncSyncMode,
    pub v2_port: u16,
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
            sync_mode: LanSyncSyncMode,
            #[serde(default)]
            v2_port: Option<u16>,
        }

        let compat = LanSyncConfigCompat::deserialize(deserializer)?;
        Ok(Self {
            port: compat.port,
            sync_mode: compat.sync_mode,
            v2_port: compat
                .v2_port
                .unwrap_or_else(|| default_lan_sync_v2_port(compat.port)),
        })
    }
}

pub fn default_lan_sync_v2_port(port: u16) -> u16 {
    port.checked_add(1).unwrap_or(port.saturating_sub(1))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncDeviceIdentity {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub pair_secret: String,
    pub last_known_address: Option<String>,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub protocol_version: u8,
    pub last_known_address: Option<String>,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

impl From<LanSyncPairedDevice> for LanSyncPairedDeviceSummary {
    fn from(device: LanSyncPairedDevice) -> Self {
        Self {
            device_id: device.device_id,
            device_name: device.device_name,
            protocol_version: 1,
            last_known_address: device.last_known_address,
            paired_at_ms: device.paired_at_ms,
            last_sync_ms: device.last_sync_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncV2Identity {
    pub device_id: DeviceId,
    pub device_name: String,
    /// base64url(no pad) 32 bytes, used to derive Ed25519 signing key.
    pub ed25519_seed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncV2PairedDevice {
    pub grant: PeerGrant,
    pub base_url: String,
    pub spki_sha256: String,
}

impl From<LanSyncV2PairedDevice> for LanSyncPairedDeviceSummary {
    fn from(device: LanSyncV2PairedDevice) -> Self {
        Self {
            device_id: device.grant.device_id.to_string(),
            device_name: device.grant.device_name,
            protocol_version: 2,
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
    pub v2_running: bool,
    pub v2_port: Option<u16>,
    pub v2_spki_sha256: Option<String>,
    pub pairing_enabled: bool,
    pub pairing_expires_at_ms: Option<u64>,
    pub sync_mode: LanSyncSyncMode,
    pub sync_mode_persistent: LanSyncSyncMode,
    pub sync_mode_overridden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPairRequest {
    pub target_device_id: String,
    pub target_device_name: String,
    pub target_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPairResponse {
    pub source_device_id: String,
    pub source_device_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairRequestEvent {
    pub request_id: String,
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub peer_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncManifestEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncManifest {
    pub entries: Vec<LanSyncManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncDiffPlan {
    pub download: Vec<LanSyncManifestEntry>,
    #[serde(default)]
    pub delete: Vec<String>,
    pub files_total: usize,
    pub bytes_total: u64,
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
    use super::{LanSyncConfig, LanSyncSyncMode, default_lan_sync_v2_port};

    #[test]
    fn config_deserializes_legacy_file_with_stable_v2_port() {
        let config: LanSyncConfig =
            serde_json::from_str(r#"{"port":55000,"sync_mode":"Mirror"}"#).unwrap();

        assert_eq!(config.port, 55000);
        assert_eq!(config.sync_mode, LanSyncSyncMode::Mirror);
        assert_eq!(config.v2_port, 55001);
    }

    #[test]
    fn config_deserializes_explicit_v2_port() {
        let config: LanSyncConfig =
            serde_json::from_str(r#"{"port":55000,"v2_port":56000}"#).unwrap();

        assert_eq!(config.port, 55000);
        assert_eq!(config.sync_mode, LanSyncSyncMode::Incremental);
        assert_eq!(config.v2_port, 56000);
    }

    #[test]
    fn default_v2_port_handles_upper_bound() {
        assert_eq!(default_lan_sync_v2_port(u16::MAX), u16::MAX - 1);
    }
}
