use async_trait::async_trait;
use ttsync_contract::peer::DeviceId;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanServerSettings, LanSyncIdentity,
    LanSyncPairedDevice, LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent,
    LanSyncSyncProgressEvent, SyncPreferences,
};
use crate::domain::models::sync::SyncOperationOptions;

#[derive(Debug, Clone)]
pub struct LanServerInfo {
    pub port: u16,
    pub spki_sha256: String,
}

#[derive(Debug, Clone)]
pub struct LanPairingApprovalRequest {
    pub request_id: String,
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub peer_ip: String,
    pub expires_at_ms: u64,
}

#[async_trait]
pub trait LanSyncSettingsRepository: Send + Sync {
    async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError>;
    async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError>;
    async fn save_sync_preferences(&self, preferences: &SyncPreferences)
    -> Result<(), DomainError>;
}

#[async_trait]
pub trait LanPeerRepository: Send + Sync {
    async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError>;
    async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError>;
    async fn upsert_paired_device(&self, device: LanSyncPairedDevice) -> Result<(), DomainError>;
    async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError>;
}

#[async_trait]
pub trait LanServerControl: Send + Sync {
    async fn start(&self, port: u16) -> Result<LanServerInfo, DomainError>;
    async fn stop(&self) -> Result<(), DomainError>;
    async fn running_info(&self) -> Option<LanServerInfo>;
}

#[async_trait]
pub trait LanAddressDiscovery: Send + Sync {
    fn list_available_addresses(&self, port: u16) -> Result<Vec<String>, DomainError>;
    fn default_advertise_address(
        &self,
        port: u16,
        available_addresses: &[String],
    ) -> Option<String>;
    async fn routed_advertise_address(
        &self,
        peer_base_url: &str,
        local_port: u16,
    ) -> Result<String, DomainError>;
}

#[async_trait]
pub trait LanPairingClient: Send + Sync {
    async fn complete_pairing(
        &self,
        base_url: &str,
        spki_sha256: &str,
        token: &str,
        request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError>;
}

#[async_trait]
pub trait PairingApproval: Send + Sync {
    async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError>;
    async fn confirm(&self, request_id: &str, accept: bool) -> Result<(), DomainError>;
    async fn cancel_all(&self);
}

pub trait LanSyncEventPublisher: Send + Sync {
    fn publish_progress(&self, payload: LanSyncSyncProgressEvent);
    fn publish_completed(&self, payload: LanSyncSyncCompletedEvent);
    fn publish_error(&self, payload: LanSyncSyncErrorEvent);
}

#[async_trait]
pub trait LanInboundRequestHandler: Send + Sync {
    async fn complete_pairing(
        &self,
        token: String,
        request: LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError>;

    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError>;
}
