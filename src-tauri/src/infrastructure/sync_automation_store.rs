use std::path::PathBuf;

use ttsync_core::dataset::ResolvedDatasetPolicy;

use crate::domain::errors::DomainError;
use crate::domain::models::sync_automation::{
    SYNC_AUTOMATION_MAX_INTERVAL_MINUTES, SYNC_AUTOMATION_MIN_INTERVAL_MINUTES,
    SyncAutomationConfig,
};
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct SyncAutomationStore {
    config_path: PathBuf,
}

impl SyncAutomationStore {
    pub fn new(default_user_dir: PathBuf) -> Self {
        Self {
            config_path: default_user_dir
                .join("user")
                .join("lan-sync")
                .join("automation.json"),
        }
    }

    pub async fn load_or_create_config(&self) -> Result<SyncAutomationConfig, DomainError> {
        if self.config_path.is_file() {
            let config = read_json_file(&self.config_path).await?;
            validate_config(&config)?;
            return Ok(config);
        }

        let config = SyncAutomationConfig::default();
        write_json_file(&self.config_path, &config).await?;
        Ok(config)
    }

    pub async fn save_config(&self, config: &SyncAutomationConfig) -> Result<(), DomainError> {
        validate_config(config)?;
        write_json_file(&self.config_path, config).await
    }
}

pub fn validate_config(config: &SyncAutomationConfig) -> Result<(), DomainError> {
    if config.interval_minutes < SYNC_AUTOMATION_MIN_INTERVAL_MINUTES
        || config.interval_minutes > SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
    {
        return Err(DomainError::InvalidData(format!(
            "Auto sync interval must be between {} and {} minutes",
            SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
        )));
    }

    if config.auto_sync_enabled && config.target.is_none() {
        return Err(DomainError::InvalidData(
            "Auto sync target is required when auto sync is enabled".to_string(),
        ));
    }

    ResolvedDatasetPolicy::from_selection(&config.selection)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SyncAutomationStore, validate_config};
    use crate::domain::models::sync_automation::{
        SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, SyncAutomationConfig,
    };

    fn temp_default_user_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-sync-automation-store-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[tokio::test]
    async fn load_or_create_config_writes_default_local_config() {
        let default_user_dir = temp_default_user_dir();
        let store = SyncAutomationStore::new(default_user_dir.clone());

        let config = store.load_or_create_config().await.expect("create config");

        assert!(!config.lan_server_auto_start);
        assert!(!config.auto_sync_enabled);
        assert_eq!(config.interval_minutes, 30);
        assert!(
            default_user_dir
                .join("user")
                .join("lan-sync")
                .join("automation.json")
                .is_file()
        );

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[test]
    fn validation_rejects_too_frequent_interval() {
        let config = SyncAutomationConfig {
            interval_minutes: SYNC_AUTOMATION_MIN_INTERVAL_MINUTES - 1,
            ..SyncAutomationConfig::default()
        };

        assert!(matches!(
            validate_config(&config),
            Err(crate::domain::errors::DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn validation_rejects_enabled_auto_sync_without_target() {
        let config = SyncAutomationConfig {
            auto_sync_enabled: true,
            ..SyncAutomationConfig::default()
        };

        assert!(matches!(
            validate_config(&config),
            Err(crate::domain::errors::DomainError::InvalidData(_))
        ));
    }
}
