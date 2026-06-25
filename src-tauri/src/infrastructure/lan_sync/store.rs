use std::path::PathBuf;

use rand::Rng;
use ttsync_contract::sync::SyncMode;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::LanSyncConfig;
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

    pub async fn load_or_create_config(&self) -> Result<LanSyncConfig, DomainError> {
        let path = self.config_path();
        if path.is_file() {
            let config = read_json_file(&path).await?;
            validate_config(&config)?;
            return Ok(config);
        }

        let port = rand::rng().random_range(49152..=65535);
        let config = LanSyncConfig {
            port,
            sync_mode: SyncMode::Incremental,
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
}

fn validate_config(config: &LanSyncConfig) -> Result<(), DomainError> {
    if config.port == 0 {
        return Err(DomainError::InvalidData(
            "LAN sync port must not be 0".to_string(),
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
    async fn config_creation_round_trips_without_port_drift() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());

        let config = store.load_or_create_config().await.expect("create config");
        let reloaded = store.load_or_create_config().await.expect("reload config");

        assert_ne!(config.port, 0);
        assert_eq!(reloaded.port, config.port);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn saved_config_round_trips_without_port_drift() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());
        let mut config = store.load_or_create_config().await.expect("create config");
        config.sync_mode = ttsync_contract::sync::SyncMode::Mirror;

        store.save_config(&config).await.expect("save config");
        let reloaded = store.load_or_create_config().await.expect("reload config");

        assert_eq!(reloaded.port, config.port);
        assert_eq!(reloaded.sync_mode, ttsync_contract::sync::SyncMode::Mirror);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_without_https_port_uses_existing_port() {
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
        .expect("write old config");

        let store = LanSyncStore::new(default_user_dir.clone());
        let config = store.load_or_create_config().await.expect("load config");

        assert_eq!(config.port, 55000);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_with_explicit_https_port_keeps_that_port() {
        let default_user_dir = temp_default_user_dir();
        let config_dir = default_user_dir.join("user").join("lan-sync");
        tokio::fs::create_dir_all(&config_dir)
            .await
            .expect("create config dir");
        tokio::fs::write(
            config_dir.join("config.json"),
            br#"{"port":55000,"v2_port":56000,"sync_mode":"Mirror"}"#,
        )
        .await
        .expect("write old config");

        let store = LanSyncStore::new(default_user_dir.clone());
        let config = store.load_or_create_config().await.expect("load config");

        assert_eq!(config.port, 56000);
        assert_eq!(config.sync_mode, ttsync_contract::sync::SyncMode::Mirror);

        store
            .save_config(&config)
            .await
            .expect("save migrated config");
        let reloaded = store
            .load_or_create_config()
            .await
            .expect("reload migrated config");
        assert_eq!(reloaded.port, 56000);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }
}
