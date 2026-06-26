use std::path::PathBuf;

use async_trait::async_trait;
use rand::Rng;
use serde::Deserialize;
use ttsync_contract::sync::SyncMode;

use crate::application::services::lan_sync_service::ports::LanSyncSettingsRepository;
use crate::application::services::sync_automation_service::{
    LoadedLanServerSettings, SyncAutomationLanSettingsRepository,
};
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanServerSettings, SyncPreferences};
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

    fn legacy_config_path(&self) -> PathBuf {
        self.lan_sync_dir.join("config.json")
    }

    fn server_settings_path(&self) -> PathBuf {
        self.lan_sync_dir.join("server-settings.json")
    }

    fn sync_preferences_path(&self) -> PathBuf {
        self.lan_sync_dir.join("sync-preferences.json")
    }

    pub async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError> {
        self.migrate_legacy_config().await?;

        let path = self.server_settings_path();
        if path.is_file() {
            let settings = read_json_file(&path).await?;
            validate_server_settings(&settings)?;
            return Ok(settings);
        }

        let settings = LanServerSettings {
            port: rand::rng().random_range(49152..=65535),
            auto_start: false,
        };
        validate_server_settings(&settings)?;
        write_json_file(&path, &settings).await?;
        Ok(settings)
    }

    pub async fn save_server_settings(
        &self,
        settings: &LanServerSettings,
    ) -> Result<(), DomainError> {
        validate_server_settings(settings)?;
        write_json_file(&self.server_settings_path(), settings).await
    }

    pub async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError> {
        self.migrate_legacy_config().await?;

        let path = self.sync_preferences_path();
        if path.is_file() {
            return read_json_file(&path).await;
        }

        let preferences = SyncPreferences {
            manual_default_mode: SyncMode::Incremental,
        };
        write_json_file(&path, &preferences).await?;
        Ok(preferences)
    }

    pub async fn save_sync_preferences(
        &self,
        preferences: &SyncPreferences,
    ) -> Result<(), DomainError> {
        write_json_file(&self.sync_preferences_path(), preferences).await
    }

    async fn migrate_legacy_config(&self) -> Result<(), DomainError> {
        let server_settings_path = self.server_settings_path();
        let sync_preferences_path = self.sync_preferences_path();
        let legacy_path = self.legacy_config_path();
        if server_settings_path.is_file() && sync_preferences_path.is_file() {
            return remove_legacy_config(&legacy_path).await;
        }

        if !legacy_path.is_file() {
            return Ok(());
        }

        let legacy: LegacyLanSyncConfig = read_json_file(&legacy_path).await?;
        let settings = LanServerSettings {
            port: legacy.v2_port.unwrap_or(legacy.port),
            auto_start: false,
        };
        let preferences = SyncPreferences {
            manual_default_mode: legacy.sync_mode,
        };
        validate_server_settings(&settings)?;

        if !server_settings_path.is_file() {
            write_json_file(&server_settings_path, &settings).await?;
        }
        if !sync_preferences_path.is_file() {
            write_json_file(&sync_preferences_path, &preferences).await?;
        }

        remove_legacy_config(&legacy_path).await
    }
}

async fn remove_legacy_config(path: &std::path::Path) -> Result<(), DomainError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(error.to_string())),
    }
}

#[async_trait]
impl SyncAutomationLanSettingsRepository for LanSyncStore {
    async fn load_or_create_server_settings(&self) -> Result<LoadedLanServerSettings, DomainError> {
        let created = !self.server_settings_path().is_file();
        let settings = LanSyncStore::load_or_create_server_settings(self).await?;
        Ok(LoadedLanServerSettings { settings, created })
    }

    async fn save_server_settings(&self, settings: &LanServerSettings) -> Result<(), DomainError> {
        LanSyncStore::save_server_settings(self, settings).await
    }

    async fn load_manual_default_sync_mode(&self) -> Result<SyncMode, DomainError> {
        Ok(LanSyncStore::load_or_create_sync_preferences(self)
            .await?
            .manual_default_mode)
    }
}

#[async_trait]
impl LanSyncSettingsRepository for LanSyncStore {
    async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError> {
        LanSyncStore::load_or_create_server_settings(self).await
    }

    async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError> {
        LanSyncStore::load_or_create_sync_preferences(self).await
    }

    async fn save_sync_preferences(
        &self,
        preferences: &SyncPreferences,
    ) -> Result<(), DomainError> {
        LanSyncStore::save_sync_preferences(self, preferences).await
    }
}

#[derive(Deserialize)]
struct LegacyLanSyncConfig {
    port: u16,
    #[serde(default)]
    sync_mode: SyncMode,
    #[serde(default)]
    v2_port: Option<u16>,
}

fn validate_server_settings(settings: &LanServerSettings) -> Result<(), DomainError> {
    if settings.port == 0 {
        return Err(DomainError::InvalidData(
            "LAN sync port must not be 0".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LanSyncStore;
    use ttsync_contract::sync::SyncMode;

    fn temp_default_user_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tauritavern-lan-store-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn server_settings_creation_round_trips_without_port_drift() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());

        let settings = store
            .load_or_create_server_settings()
            .await
            .expect("create settings");
        let reloaded = store
            .load_or_create_server_settings()
            .await
            .expect("reload settings");

        assert_ne!(settings.port, 0);
        assert_eq!(reloaded.port, settings.port);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn sync_preferences_round_trip_manual_default_mode() {
        let default_user_dir = temp_default_user_dir();
        let store = LanSyncStore::new(default_user_dir.clone());
        let mut preferences = store
            .load_or_create_sync_preferences()
            .await
            .expect("create preferences");
        preferences.manual_default_mode = SyncMode::Mirror;

        store
            .save_sync_preferences(&preferences)
            .await
            .expect("save preferences");
        let reloaded = store
            .load_or_create_sync_preferences()
            .await
            .expect("reload preferences");

        assert_eq!(reloaded.manual_default_mode, SyncMode::Mirror);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_without_https_port_uses_existing_port() {
        let default_user_dir = temp_default_user_dir();
        write_legacy_config(
            &default_user_dir,
            br#"{"port":55000,"sync_mode":"Incremental"}"#,
        )
        .await;

        let store = LanSyncStore::new(default_user_dir.clone());
        let settings = store
            .load_or_create_server_settings()
            .await
            .expect("load settings");

        assert_eq!(settings.port, 55000);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_with_explicit_https_port_migrates_once() {
        let default_user_dir = temp_default_user_dir();
        write_legacy_config(
            &default_user_dir,
            br#"{"port":55000,"v2_port":56000,"sync_mode":"Mirror"}"#,
        )
        .await;

        let store = LanSyncStore::new(default_user_dir.clone());
        let settings = store
            .load_or_create_server_settings()
            .await
            .expect("load settings");
        let preferences = store
            .load_or_create_sync_preferences()
            .await
            .expect("load preferences");
        let reloaded = store
            .load_or_create_server_settings()
            .await
            .expect("reload settings");

        assert_eq!(settings.port, 56000);
        assert_eq!(reloaded.port, 56000);
        assert_eq!(preferences.manual_default_mode, SyncMode::Mirror);
        assert!(
            !default_user_dir
                .join("user")
                .join("lan-sync")
                .join("config.json")
                .exists()
        );

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_migrates_when_preferences_load_runs_first() {
        let default_user_dir = temp_default_user_dir();
        write_legacy_config(
            &default_user_dir,
            br#"{"port":55000,"v2_port":56000,"sync_mode":"Mirror"}"#,
        )
        .await;

        let store = LanSyncStore::new(default_user_dir.clone());
        let preferences = store
            .load_or_create_sync_preferences()
            .await
            .expect("load preferences");
        let settings = store
            .load_or_create_server_settings()
            .await
            .expect("load settings");

        assert_eq!(preferences.manual_default_mode, SyncMode::Mirror);
        assert_eq!(settings.port, 56000);
        assert!(
            !default_user_dir
                .join("user")
                .join("lan-sync")
                .join("config.json")
                .exists()
        );

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn invalid_old_config_fails_without_creating_new_state() {
        let default_user_dir = temp_default_user_dir();
        write_legacy_config(&default_user_dir, br#"{"port":0}"#).await;

        let store = LanSyncStore::new(default_user_dir.clone());
        assert!(store.load_or_create_server_settings().await.is_err());
        assert!(
            !default_user_dir
                .join("user")
                .join("lan-sync")
                .join("server-settings.json")
                .exists()
        );
        assert!(
            !default_user_dir
                .join("user")
                .join("lan-sync")
                .join("v2")
                .join("identity.json")
                .exists()
        );

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_is_removed_when_new_files_already_exist() {
        let default_user_dir = temp_default_user_dir();
        let config_dir = default_user_dir.join("user").join("lan-sync");
        tokio::fs::create_dir_all(&config_dir)
            .await
            .expect("create config dir");
        tokio::fs::write(
            config_dir.join("server-settings.json"),
            br#"{"port":56000,"auto_start":true}"#,
        )
        .await
        .expect("write settings");
        tokio::fs::write(
            config_dir.join("sync-preferences.json"),
            br#"{"manual_default_mode":"Mirror"}"#,
        )
        .await
        .expect("write preferences");
        tokio::fs::write(
            config_dir.join("config.json"),
            br#"{"port":55000,"sync_mode":"Incremental"}"#,
        )
        .await
        .expect("write old config");

        let store = LanSyncStore::new(default_user_dir.clone());
        let settings = store
            .load_or_create_server_settings()
            .await
            .expect("load settings");

        assert_eq!(settings.port, 56000);
        assert!(settings.auto_start);
        assert!(!config_dir.join("config.json").exists());

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    async fn write_legacy_config(default_user_dir: &std::path::Path, bytes: &[u8]) {
        let config_dir = default_user_dir.join("user").join("lan-sync");
        tokio::fs::create_dir_all(&config_dir)
            .await
            .expect("create config dir");
        tokio::fs::write(config_dir.join("config.json"), bytes)
            .await
            .expect("write old config");
    }
}
