use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncConfig {
    pub port: u16,
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
pub struct LanSyncStatus {
    pub running: bool,
    pub address: Option<String>,
    pub port: u16,
    pub pairing_enabled: bool,
    pub pairing_expires_at_ms: Option<u64>,
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
    pub files_total: usize,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub enum LanSyncSyncPhase {
    Scanning,
    Diffing,
    Downloading,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncSyncErrorEvent {
    pub message: String,
}
