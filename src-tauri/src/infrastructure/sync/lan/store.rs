use std::path::PathBuf;

use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_core::crypto::random_base64url;
use uuid::Uuid;

use crate::application::services::lan_sync_service::ports::LanPeerRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanSyncIdentity, LanSyncPairedDevice};
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

#[derive(Debug, Clone)]
pub struct LanPeerStore {
    state_dir: PathBuf,
}

#[async_trait::async_trait]
impl LanPeerRepository for LanPeerStore {
    async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError> {
        LanPeerStore::load_or_create_identity(self).await
    }

    async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
        LanPeerStore::load_paired_devices(self).await
    }

    async fn upsert_paired_device(&self, device: LanSyncPairedDevice) -> Result<(), DomainError> {
        LanPeerStore::upsert_paired_device(self, device).await
    }

    async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError> {
        LanPeerStore::remove_paired_device(self, device_id).await
    }
}

impl LanPeerStore {
    pub fn new(default_user_dir: PathBuf) -> Self {
        Self {
            state_dir: default_user_dir.join("user").join("lan-sync").join("v2"),
        }
    }

    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone()
    }

    fn identity_path(&self) -> PathBuf {
        self.state_dir.join("identity.json")
    }

    fn paired_devices_path(&self) -> PathBuf {
        self.state_dir.join("peers.json")
    }

    pub async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError> {
        let path = self.identity_path();
        if path.is_file() {
            return read_json_file(&path).await;
        }

        let identity = LanSyncIdentity {
            device_id: DeviceId::new(Uuid::new_v4().to_string())
                .expect("generated uuid must be valid"),
            device_name: "TauriTavern".to_string(),
            ed25519_seed: random_base64url(32),
        };
        write_json_file(&path, &identity).await?;
        Ok(identity)
    }

    pub async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
        let path = self.paired_devices_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }

        read_json_file(&path).await
    }

    pub async fn upsert_paired_device(
        &self,
        device: LanSyncPairedDevice,
    ) -> Result<(), DomainError> {
        let mut devices = self.load_paired_devices().await?;
        if let Some(existing) = devices
            .iter_mut()
            .find(|item| item.grant.device_id == device.grant.device_id)
        {
            *existing = device;
        } else {
            devices.push(device);
        }

        self.save_paired_devices(&devices).await
    }

    pub async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError> {
        let devices = self.load_paired_devices().await?;
        let filtered = devices
            .into_iter()
            .filter(|device| &device.grant.device_id != device_id)
            .collect::<Vec<_>>();

        self.save_paired_devices(&filtered).await
    }

    pub async fn get_paired_device(
        &self,
        device_id: &DeviceId,
    ) -> Result<LanSyncPairedDevice, DomainError> {
        self.load_paired_devices()
            .await?
            .into_iter()
            .find(|device| &device.grant.device_id == device_id)
            .ok_or_else(|| DomainError::NotFound(format!("LAN Sync peer not found: {}", device_id)))
    }

    pub async fn get_peer_grant(&self, device_id: &DeviceId) -> Result<PeerGrant, DomainError> {
        self.get_paired_device(device_id)
            .await
            .map(|device| device.grant)
    }

    pub async fn save_peer_grant(&self, grant: PeerGrant) -> Result<(), DomainError> {
        let mut device = self.get_paired_device(&grant.device_id).await?;
        device.grant = grant;
        self.upsert_paired_device(device).await
    }

    async fn save_paired_devices(
        &self,
        devices: &[LanSyncPairedDevice],
    ) -> Result<(), DomainError> {
        write_json_file(&self.paired_devices_path(), devices).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ttsync_contract::peer::Permissions;

    fn temp_default_user_dir() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-lan-peer-store-{}", Uuid::new_v4()))
    }

    fn test_device_id() -> DeviceId {
        DeviceId::new("550e8400-e29b-41d4-a716-446655440000".to_string()).unwrap()
    }

    fn test_paired_device(device_id: DeviceId) -> LanSyncPairedDevice {
        LanSyncPairedDevice {
            grant: PeerGrant {
                device_id,
                device_name: "Peer".to_string(),
                public_key: vec![7; 32],
                permissions: Permissions {
                    read: true,
                    write: false,
                    mirror_delete: true,
                },
                paired_at_ms: 1,
                last_sync_ms: None,
            },
            base_url: "https://127.0.0.1:50000".to_string(),
            spki_sha256: "spki".to_string(),
        }
    }

    #[tokio::test]
    async fn store_round_trips_identity() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());

        let first_identity = store
            .load_or_create_identity()
            .await
            .expect("create identity");
        let second_identity = store
            .load_or_create_identity()
            .await
            .expect("load identity");
        assert_eq!(first_identity.device_id, second_identity.device_id);
        assert_eq!(first_identity.ed25519_seed, second_identity.ed25519_seed);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn store_round_trips_and_removes_peer() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());
        let device_id = test_device_id();

        store
            .upsert_paired_device(test_paired_device(device_id.clone()))
            .await
            .expect("upsert peer");

        let peer = store
            .get_paired_device(&device_id)
            .await
            .expect("load peer");
        assert_eq!(peer.grant.device_id, device_id);
        assert_eq!(peer.base_url, "https://127.0.0.1:50000");

        store
            .remove_paired_device(&device_id)
            .await
            .expect("remove peer");
        assert!(matches!(
            store.get_paired_device(&device_id).await,
            Err(DomainError::NotFound(_))
        ));

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }
}
