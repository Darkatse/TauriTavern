use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use ttsync_contract::peer::{DeviceId, Permissions};
use url::Url;

use crate::domain::errors::DomainError;
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::tt_sync::v2_api::sync_error_to_domain;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncV2PairCompleteRequest {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_pubkey: String,
    pub client_base_url: String,
    pub client_spki_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncV2PairCompleteResponse {
    pub server_device_id: DeviceId,
    pub server_device_name: String,
    pub server_device_pubkey: String,
    pub granted_permissions: Permissions,
}

#[derive(Debug, Clone)]
pub struct LanSyncV2PairingSession {
    pub token: String,
    pub expires_at_ms: u64,
}

#[async_trait]
pub trait LanSyncV2PairingCoordinator: Send + Sync {
    async fn active_pairing_session(&self) -> Option<LanSyncV2PairingSession>;

    async fn request_pairing_decision(
        &self,
        peer_device_id: String,
        peer_device_name: String,
        peer_ip: String,
    ) -> Result<bool, DomainError>;

    async fn clear_pairing_session(&self);
}

#[async_trait]
impl LanSyncV2PairingCoordinator for LanSyncRuntime {
    async fn active_pairing_session(&self) -> Option<LanSyncV2PairingSession> {
        self.get_pairing_session()
            .await
            .map(|session| LanSyncV2PairingSession {
                token: session.token,
                expires_at_ms: session.expires_at_ms,
            })
    }

    async fn request_pairing_decision(
        &self,
        peer_device_id: String,
        peer_device_name: String,
        peer_ip: String,
    ) -> Result<bool, DomainError> {
        self.request_pairing_decision(peer_device_id, peer_device_name, peer_ip)
            .await
    }

    async fn clear_pairing_session(&self) {
        self.clear_pairing_session().await;
    }
}

pub fn default_lan_v2_permissions() -> Permissions {
    Permissions {
        read: true,
        write: false,
        mirror_delete: true,
    }
}

pub fn decode_device_pubkey_b64url(value: &str) -> Result<Vec<u8>, DomainError> {
    let public_key = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if public_key.len() != 32 {
        return Err(DomainError::InvalidData(
            "LAN Sync v2 device public key must be 32 bytes".to_string(),
        ));
    }

    Ok(public_key)
}

pub fn validate_https_base_url(value: &str) -> Result<(), DomainError> {
    ttsync_http::client::validate_https_origin(value).map_err(sync_error_to_domain)
}

pub fn host_for_pairing_prompt(base_url: &str) -> Result<String, DomainError> {
    let parsed =
        Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    parsed
        .host_str()
        .map(str::to_string)
        .ok_or_else(|| DomainError::InvalidData("LAN Sync v2 base URL is missing host".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_permissions_allow_read_and_mirror_delete_only() {
        let permissions = default_lan_v2_permissions();
        assert!(permissions.read);
        assert!(permissions.mirror_delete);
        assert!(!permissions.write);
    }

    #[test]
    fn decode_device_pubkey_requires_32_bytes() {
        let encoded = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert_eq!(
            decode_device_pubkey_b64url(&encoded).unwrap(),
            vec![7u8; 32]
        );

        let short = URL_SAFE_NO_PAD.encode([7u8; 31]);
        assert!(matches!(
            decode_device_pubkey_b64url(&short),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn base_url_must_be_https() {
        validate_https_base_url("https://127.0.0.1:50000").unwrap();
        assert!(matches!(
            validate_https_base_url("http://127.0.0.1:50000"),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn base_url_must_be_origin() {
        assert!(matches!(
            validate_https_base_url("https://127.0.0.1:50000/v2"),
            Err(DomainError::InvalidData(_))
        ));
        assert!(matches!(
            validate_https_base_url("https://user@127.0.0.1:50000"),
            Err(DomainError::InvalidData(_))
        ));
    }
}
