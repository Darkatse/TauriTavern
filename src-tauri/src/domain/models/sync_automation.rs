use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_core::dataset::tauri_tavern_default_selection;

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
    #[serde(default = "tauri_tavern_default_selection")]
    pub selection: DatasetSelection,
}

impl Default for SyncAutomationConfig {
    fn default() -> Self {
        Self {
            lan_server_auto_start: false,
            auto_sync_enabled: false,
            interval_minutes: default_interval_minutes(),
            target: None,
            selection: tauri_tavern_default_selection(),
        }
    }
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
