use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;

pub const SYNC_AUTOMATION_COLD_START_DELAY_SECS: u64 = 45;
pub const SYNC_AUTOMATION_MIN_INTERVAL_MINUTES: u16 = 5;
pub const SYNC_AUTOMATION_MAX_INTERVAL_MINUTES: u16 = 1440;
const LEGACY_USER_DATASET_ID: &str = "legacy.user";

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
    #[serde(default = "tauri_tavern_continuity_selection")]
    pub selection: DatasetSelection,
}

impl Default for SyncAutomationConfig {
    fn default() -> Self {
        Self {
            lan_server_auto_start: false,
            auto_sync_enabled: false,
            interval_minutes: default_interval_minutes(),
            target: None,
            selection: tauri_tavern_continuity_selection(),
        }
    }
}

pub fn tauri_tavern_continuity_selection() -> DatasetSelection {
    let mut selection = ttsync_core::dataset::tauri_tavern_default_selection();
    if !selection
        .dataset_ids
        .iter()
        .any(|id| id == LEGACY_USER_DATASET_ID)
    {
        selection.dataset_ids.push(LEGACY_USER_DATASET_ID.to_string());
    }
    selection
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncAutomationStatus {
    pub running: bool,
    pub next_run_at_ms: Option<u64>,
    pub last_attempt_at_ms: Option<u64>,
    pub last_success_at_ms: Option<u64>,
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

#[cfg(test)]
mod tests {
    use ttsync_core::dataset::ResolvedDatasetPolicy;

    use super::SyncAutomationConfig;

    #[test]
    fn default_config_includes_user_cache_without_sync_state() {
        let config = SyncAutomationConfig::default();
        let policy = ResolvedDatasetPolicy::from_selection(&config.selection)
            .expect("default automation selection should be valid");

        assert!(
            policy.contains_path("default-user/user/cache/chat_summary_index_v1.json"),
            "chat summary indexes are required for automated continuity sync"
        );
        assert!(
            !policy.contains_path("default-user/user/lan-sync/v2/identity.json"),
            "local sync identities must stay device-local"
        );
    }
}
