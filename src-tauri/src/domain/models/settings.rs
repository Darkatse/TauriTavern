use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn default_perf_profile() -> String {
    "auto".to_string()
}

fn default_panel_runtime_profile() -> String {
    "off".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TauriTavernMigrationState {
    /// One-time migration for legacy character cards whose `create_date` was stored as
    /// `YYYY-MM-DD HH:MM:SS UTC` (TauriTavern bug) instead of ISO 8601.
    #[serde(default)]
    pub character_create_date_iso_v1: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernSettings {
    pub updates: TauriTavernUpdateSettings,
    #[serde(default = "default_perf_profile")]
    pub perf_profile: String,
    #[serde(default = "default_panel_runtime_profile")]
    pub panel_runtime_profile: String,
    #[serde(default)]
    pub migrations: TauriTavernMigrationState,
}

impl Default for TauriTavernSettings {
    fn default() -> Self {
        Self {
            updates: TauriTavernUpdateSettings::default(),
            perf_profile: default_perf_profile(),
            panel_runtime_profile: default_panel_runtime_profile(),
            migrations: TauriTavernMigrationState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TauriTavernUpdateSettings {
    pub startup_popup: StartupUpdatePopupSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartupUpdatePopupSettings {
    pub dismissed_release_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub date: i64,
    pub name: String,
    pub size: u64,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            data: Value::Object(Map::new()),
        }
    }
}
