use std::path::PathBuf;

use rand::Rng;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncConfig, LanSyncDeviceIdentity, LanSyncPairedDevice, LanSyncSyncMode,
};
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct LanSyncStore {
    lan_sync_dir: PathBuf,
}

impl LanSyncStore {
    pub fn new(default_user_dir: PathBuf) -> Self {
        Self {
            lan_sync_dir: default_user_dir.join("user").join("lan-sync"),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.lan_sync_dir.join("config.json")
    }

    fn identity_path(&self) -> PathBuf {
        self.lan_sync_dir.join("identity.json")
    }

    fn paired_devices_path(&self) -> PathBuf {
        self.lan_sync_dir.join("paired-devices.json")
    }

    pub async fn load_or_create_config(&self) -> Result<LanSyncConfig, DomainError> {
        let path = self.config_path();
        if path.is_file() {
            return read_json_file(&path).await;
        }

        let port = rand::thread_rng().gen_range(49152..=65535);
        let config = LanSyncConfig {
            port,
            sync_mode: LanSyncSyncMode::Incremental,
        };
        write_json_file(&path, &config).await?;
        Ok(config)
    }

    pub async fn save_config(&self, config: &LanSyncConfig) -> Result<(), DomainError> {
        let path = self.config_path();
        write_json_file(&path, config).await
    }

    pub async fn load_or_create_identity(&self) -> Result<LanSyncDeviceIdentity, DomainError> {
        let path = self.identity_path();
        if path.is_file() {
            return read_json_file(&path).await;
        }

        let identity = LanSyncDeviceIdentity {
            device_id: Uuid::new_v4().to_string(),
            device_name: "TauriTavern".to_string(),
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

    pub async fn save_paired_devices(
        &self,
        devices: &[LanSyncPairedDevice],
    ) -> Result<(), DomainError> {
        let path = self.paired_devices_path();
        write_json_file(&path, devices).await
    }

    pub async fn upsert_paired_device(
        &self,
        device: LanSyncPairedDevice,
    ) -> Result<(), DomainError> {
        let mut devices = self.load_paired_devices().await?;

        if let Some(existing) = devices
            .iter_mut()
            .find(|item| item.device_id == device.device_id)
        {
            *existing = device;
        } else {
            devices.push(device);
        }

        self.save_paired_devices(&devices).await
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        let devices = self.load_paired_devices().await?;
        let filtered = devices
            .into_iter()
            .filter(|device| device.device_id != device_id)
            .collect::<Vec<_>>();

        self.save_paired_devices(&filtered).await
    }
}
