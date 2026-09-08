pub mod client;
pub mod control;
pub mod discovery;
pub mod peer_discovery;
pub mod server;
pub mod store;

/// LAN metadata extends the shared transfer status without changing its wire contract.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct LanStatusResponse {
    #[serde(flatten)]
    pub status: ttsync_contract::status::StatusResponse,
    #[serde(default)]
    pub platform: Option<String>,
}

impl LanStatusResponse {
    pub fn update_peer(
        &self,
        peer: &mut tt_domain::models::lan_sync::LanSyncPairedDevice,
        base_url: &str,
    ) {
        peer.base_url = base_url.to_string();
        if let Some(name) = &self.status.device_name {
            peer.grant.device_name = name.clone();
        }
        peer.platform = self.platform.clone();
    }
}
