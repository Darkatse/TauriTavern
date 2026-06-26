use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_contract::sync::SyncMode;
use ttsync_core::dataset::{ResolvedDatasetPolicy, tauri_tavern_default_selection};

use crate::domain::errors::DomainError;

pub const SYNC_AUTOMATION_COLD_START_DELAY_SECS: u64 = 45;
pub const SYNC_AUTOMATION_MIN_INTERVAL_MINUTES: u16 = 5;
pub const SYNC_AUTOMATION_MAX_INTERVAL_MINUTES: u16 = 1440;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncAutomationTarget {
    Lan { device_id: String },
    Tt { server_device_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAutomationConfig {
    #[serde(default)]
    pub lan_server_auto_start: bool,
    #[serde(default)]
    pub auto_sync_enabled: bool,
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u16,
    #[serde(default)]
    pub target: Option<SyncAutomationTarget>,
    #[serde(default)]
    pub sync_mode: SyncMode,
    pub selection: DatasetSelection,
}

impl Default for SyncAutomationConfig {
    fn default() -> Self {
        Self {
            lan_server_auto_start: false,
            auto_sync_enabled: false,
            interval_minutes: default_interval_minutes(),
            target: None,
            sync_mode: SyncMode::Incremental,
            selection: tauri_tavern_default_selection(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledSyncRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u16,
    #[serde(default)]
    pub target: Option<SyncAutomationTarget>,
    #[serde(default)]
    pub sync_mode: SyncMode,
    pub selection: DatasetSelection,
    #[serde(default = "default_require_bundle_zstd")]
    pub require_bundle_zstd: bool,
}

impl Default for ScheduledSyncRule {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_interval_minutes(),
            target: None,
            sync_mode: SyncMode::Incremental,
            selection: tauri_tavern_default_selection(),
            require_bundle_zstd: default_require_bundle_zstd(),
        }
    }
}

impl SyncAutomationConfig {
    pub fn from_parts(lan_server_auto_start: bool, rule: ScheduledSyncRule) -> Self {
        Self {
            lan_server_auto_start,
            auto_sync_enabled: rule.enabled,
            interval_minutes: rule.interval_minutes,
            target: rule.target,
            sync_mode: rule.sync_mode,
            selection: rule.selection,
        }
    }

    pub fn into_rule(self) -> ScheduledSyncRule {
        ScheduledSyncRule {
            enabled: self.auto_sync_enabled,
            interval_minutes: self.interval_minutes,
            target: self.target,
            sync_mode: self.sync_mode,
            selection: self.selection,
            require_bundle_zstd: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncAutomationStatus {
    pub running: bool,
    pub next_run_at_ms: Option<u64>,
    pub last_attempt_at_ms: Option<u64>,
    pub last_success_at_ms: Option<u64>,
    pub last_request_accepted_at_ms: Option<u64>,
    pub last_error_at_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncAutomationToastLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncAutomationToastEvent {
    pub level: SyncAutomationToastLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at_ms: Option<u64>,
}

fn default_interval_minutes() -> u16 {
    30
}

fn default_require_bundle_zstd() -> bool {
    true
}

pub fn validate_scheduled_sync_rule(rule: &ScheduledSyncRule) -> Result<(), DomainError> {
    if rule.interval_minutes < SYNC_AUTOMATION_MIN_INTERVAL_MINUTES
        || rule.interval_minutes > SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
    {
        return Err(DomainError::InvalidData(format!(
            "Auto sync interval must be between {} and {} minutes",
            SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
        )));
    }

    if rule.enabled && rule.target.is_none() {
        return Err(DomainError::InvalidData(
            "Auto sync target is required when auto sync is enabled".to_string(),
        ));
    }

    ResolvedDatasetPolicy::from_selection(&rule.selection)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;

    Ok(())
}

pub fn validate_sync_automation_config(config: &SyncAutomationConfig) -> Result<(), DomainError> {
    validate_scheduled_sync_rule(&config.clone().into_rule())
}
