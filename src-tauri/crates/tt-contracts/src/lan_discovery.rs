use serde::{Deserialize, Serialize};
use ttsync_contract::peer::DeviceId;

/// Public connection information for a running LAN HTTPS server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanDiscoveryAnnouncement {
    pub device_id: DeviceId,
    pub device_name: String,
    #[serde(default)]
    pub platform: Option<String>,
    pub port: u16,
    pub spki_sha256: String,
}

/// Unverified discovery hints. Paired connections must use their saved TLS pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanDiscoveredDevice {
    pub device_id: DeviceId,
    pub device_name: String,
    pub platform: Option<String>,
    pub base_urls: Vec<String>,
    pub spki_sha256: String,
}
