use std::path::PathBuf;

use rand::Rng;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncConfig, LanSyncDeviceIdentity, LanSyncPairedDevice, LanSyncSyncMode,
    default_lan_sync_v2_port,
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
            let config = read_json_file(&path).await?;
            validate_config(&config)?;
            return Ok(config);
        }

        let port = rand::rng().random_range(49152..=65534);
        let config = LanSyncConfig {
            port,
            v2_port: default_lan_sync_v2_port(port),
            sync_mode: LanSyncSyncMode::Incremental,
        };
        validate_config(&config)?;
        write_json_file(&path, &config).await?;
        Ok(config)
    }

    pub async fn save_config(&self, config: &LanSyncConfig) -> Result<(), DomainError> {
        validate_config(config)?;
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

fn validate_config(config: &LanSyncConfig) -> Result<(), DomainError> {
    if config.port == 0 {
        return Err(DomainError::InvalidData(
            "LAN sync port must not be 0".to_string(),
        ));
    }
    if config.v2_port == 0 {
        return Err(DomainError::InvalidData(
            "LAN Sync v2 port must not be 0".to_string(),
        ));
    }
    if config.port == config.v2_port {
        return Err(DomainError::InvalidData(
            "LAN sync v1 and v2 ports must be different".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LanSyncStore;

    fn temp_default_user_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tauritavern-lan-store-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn config_creation_sets_stable_v2_port() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());

        let config = store.load_or_create_config().await.expect("create config");

        assert_ne!(config.port, 0);
        assert_ne!(config.v2_port, 0);
        assert_ne!(config.port, config.v2_port);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn legacy_config_loads_with_derived_v2_port() {
        let default_user_dir = temp_default_user_dir();
        let config_dir = default_user_dir.join("user").join("lan-sync");
        tokio::fs::create_dir_all(&config_dir)
            .await
            .expect("create config dir");
        tokio::fs::write(
            config_dir.join("config.json"),
            br#"{"port":55000,"sync_mode":"Incremental"}"#,
        )
        .await
        .expect("write legacy config");

        let store = LanSyncStore::new(default_user_dir.clone());
        let config = store.load_or_create_config().await.expect("load config");

        assert_eq!(config.port, 55000);
        assert_eq!(config.v2_port, 55001);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn save_config_rejects_invalid_v2_port() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());

        let result = store
            .save_config(&crate::domain::models::lan_sync::LanSyncConfig {
                port: 55000,
                v2_port: 55000,
                sync_mode: crate::domain::models::lan_sync::LanSyncSyncMode::Incremental,
            })
            .await;

        assert!(matches!(
            result,
            Err(crate::domain::errors::DomainError::InvalidData(_))
        ));

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }
}
