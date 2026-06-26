use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use ttsync_core::dataset::tauri_tavern_default_selection;

use crate::application::services::sync_automation_service::{
    LoadedScheduledSyncRule, SyncAutomationRuleRepository,
};
use crate::domain::errors::DomainError;
use crate::domain::models::sync_automation::{
    ScheduledSyncRule, SyncAutomationConfig, validate_scheduled_sync_rule,
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

    pub async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError> {
        if self.config_path.is_file() {
            let value = read_json_file::<Value>(&self.config_path).await?;
            let loaded = decode_rule(value)?;
            validate_scheduled_sync_rule(&loaded.rule)?;
            return Ok(loaded);
        }

        let rule = ScheduledSyncRule::default();
        write_json_file(&self.config_path, &rule).await?;
        Ok(LoadedScheduledSyncRule::new(rule))
    }

    pub async fn save_rule(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError> {
        validate_scheduled_sync_rule(rule)?;
        write_json_file(&self.config_path, rule).await
    }
}

#[async_trait]
impl SyncAutomationRuleRepository for SyncAutomationStore {
    async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError> {
        SyncAutomationStore::load_or_create_rule(self).await
    }

    async fn save_rule(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError> {
        SyncAutomationStore::save_rule(self, rule).await
    }
}

#[derive(Deserialize)]
struct LegacySyncAutomationConfig {
    #[serde(default, alias = "lanServerAutoStart")]
    lan_server_auto_start: bool,
    #[serde(default, alias = "autoSyncEnabled")]
    auto_sync_enabled: bool,
    #[serde(default = "legacy_default_interval_minutes", alias = "intervalMinutes")]
    interval_minutes: u16,
    #[serde(default)]
    target: Option<crate::domain::models::sync_automation::SyncAutomationTarget>,
    #[serde(default, alias = "syncMode")]
    sync_mode: ttsync_contract::sync::SyncMode,
    #[serde(default = "tauri_tavern_default_selection")]
    selection: ttsync_contract::dataset::DatasetSelection,
}

impl From<LegacySyncAutomationConfig> for SyncAutomationConfig {
    fn from(value: LegacySyncAutomationConfig) -> Self {
        SyncAutomationConfig {
            lan_server_auto_start: value.lan_server_auto_start,
            auto_sync_enabled: value.auto_sync_enabled,
            interval_minutes: value.interval_minutes,
            target: value.target,
            sync_mode: value.sync_mode,
            selection: value.selection,
        }
    }
}

fn decode_rule(value: Value) -> Result<LoadedScheduledSyncRule, DomainError> {
    let is_legacy = value.get("auto_sync_enabled").is_some()
        || value.get("lan_server_auto_start").is_some()
        || value.get("lanServerAutoStart").is_some()
        || value.get("autoSyncEnabled").is_some();
    let legacy_missing_sync_mode =
        value.get("sync_mode").is_none() && value.get("syncMode").is_none();

    if is_legacy {
        let legacy: LegacySyncAutomationConfig =
            serde_json::from_value(value).map_err(|error| {
                DomainError::InvalidData(format!("Invalid sync automation config: {error}"))
            })?;
        let lan_server_auto_start = legacy.lan_server_auto_start;
        let rule = SyncAutomationConfig::from(legacy).into_rule();
        return Ok(LoadedScheduledSyncRule {
            rule,
            legacy_lan_server_auto_start: Some(lan_server_auto_start),
            legacy_missing_sync_mode,
            rewrite_canonical: true,
        });
    }

    let rule: ScheduledSyncRule = serde_json::from_value(value).map_err(|error| {
        DomainError::InvalidData(format!("Invalid scheduled sync rule: {error}"))
    })?;
    Ok(LoadedScheduledSyncRule::new(rule))
}

fn legacy_default_interval_minutes() -> u16 {
    30
}

#[cfg(test)]
mod tests {
    use super::SyncAutomationStore;
    use crate::domain::models::sync_automation::{
        SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, ScheduledSyncRule, validate_scheduled_sync_rule,
    };

    fn temp_default_user_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-sync-automation-store-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[tokio::test]
    async fn load_or_create_rule_writes_default_local_config() {
        let default_user_dir = temp_default_user_dir();
        let store = SyncAutomationStore::new(default_user_dir.clone());

        let loaded = store.load_or_create_rule().await.expect("create rule");

        assert!(!loaded.rule.enabled);
        assert_eq!(loaded.rule.interval_minutes, 30);
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
        let rule = ScheduledSyncRule {
            interval_minutes: SYNC_AUTOMATION_MIN_INTERVAL_MINUTES - 1,
            ..ScheduledSyncRule::default()
        };

        assert!(matches!(
            validate_scheduled_sync_rule(&rule),
            Err(crate::domain::errors::DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn validation_rejects_enabled_auto_sync_without_target() {
        let rule = ScheduledSyncRule {
            enabled: true,
            ..ScheduledSyncRule::default()
        };

        assert!(matches!(
            validate_scheduled_sync_rule(&rule),
            Err(crate::domain::errors::DomainError::InvalidData(_))
        ));
    }

    #[tokio::test]
    async fn old_config_reports_canonical_rewrite() {
        let default_user_dir = temp_default_user_dir();
        let store = SyncAutomationStore::new(default_user_dir.clone());
        let path = default_user_dir
            .join("user")
            .join("lan-sync")
            .join("automation.json");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .expect("create dir");
        tokio::fs::write(
            &path,
            br#"{"lan_server_auto_start":true,"auto_sync_enabled":false,"interval_minutes":15,"selection":{"policy_version":1,"dataset_ids":["chat.character.history"]}}"#,
        )
        .await
        .expect("write old config");

        let loaded = store.load_or_create_rule().await.expect("load old config");

        assert_eq!(loaded.legacy_lan_server_auto_start, Some(true));
        assert_eq!(loaded.rule.interval_minutes, 15);
        assert!(loaded.rewrite_canonical);
        assert!(loaded.legacy_missing_sync_mode);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_camel_case_config_reports_canonical_rewrite() {
        let default_user_dir = temp_default_user_dir();
        let store = SyncAutomationStore::new(default_user_dir.clone());
        let path = default_user_dir
            .join("user")
            .join("lan-sync")
            .join("automation.json");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .expect("create dir");
        tokio::fs::write(
            &path,
            br#"{"lanServerAutoStart":true,"autoSyncEnabled":false,"intervalMinutes":15,"selection":{"policy_version":1,"dataset_ids":["chat.character.history"]}}"#,
        )
        .await
        .expect("write old config");

        let loaded = store.load_or_create_rule().await.expect("load old config");

        assert_eq!(loaded.legacy_lan_server_auto_start, Some(true));
        assert_eq!(loaded.rule.interval_minutes, 15);
        assert!(loaded.rewrite_canonical);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test]
    async fn old_config_without_selection_is_migrated() {
        let default_user_dir = temp_default_user_dir();
        let store = SyncAutomationStore::new(default_user_dir.clone());
        let path = default_user_dir
            .join("user")
            .join("lan-sync")
            .join("automation.json");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .expect("create dir");
        tokio::fs::write(
            &path,
            br#"{"lan_server_auto_start":true,"auto_sync_enabled":false,"interval_minutes":15}"#,
        )
        .await
        .expect("write old config");

        let loaded = store.load_or_create_rule().await.expect("load old config");

        assert_eq!(loaded.rule.interval_minutes, 15);
        assert_eq!(
            loaded.rule.selection,
            ttsync_core::dataset::tauri_tavern_default_selection()
        );
        assert!(loaded.rewrite_canonical);

        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }
}
