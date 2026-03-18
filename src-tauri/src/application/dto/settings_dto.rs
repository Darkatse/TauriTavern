use crate::domain::models::settings::{
    SettingsSnapshot, StartupUpdatePopupSettings, TauriTavernSettings, TauriTavernUpdateSettings,
    UserSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernSettingsDto {
    pub updates: TauriTavernUpdateSettingsDto,
    pub perf_profile: String,
    pub panel_runtime_profile: String,
    pub embedded_runtime_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernUpdateSettingsDto {
    pub startup_popup: StartupUpdatePopupSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupUpdatePopupSettingsDto {
    pub dismissed_release_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTauriTavernSettingsDto {
    pub updates: Option<TauriTavernUpdateSettingsDto>,
    pub perf_profile: Option<String>,
    pub panel_runtime_profile: Option<String>,
    pub embedded_runtime_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettingsDto {
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshotDto {
    pub date: i64,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SillyTavernSettingsResponseDto {
    pub settings: String,
    pub koboldai_settings: Vec<String>,
    pub koboldai_setting_names: Vec<String>,
    pub world_names: Vec<String>,
    pub novelai_settings: Vec<String>,
    pub novelai_setting_names: Vec<String>,
    pub openai_settings: Vec<String>,
    pub openai_setting_names: Vec<String>,
    pub textgenerationwebui_presets: Vec<String>,
    pub textgenerationwebui_preset_names: Vec<String>,
    pub themes: Vec<Value>,
    #[serde(rename = "movingUIPresets")]
    pub moving_ui_presets: Vec<Value>,
    #[serde(rename = "quickReplyPresets")]
    pub quick_reply_presets: Vec<Value>,
    pub instruct: Vec<Value>,
    pub context: Vec<Value>,
    pub sysprompt: Vec<Value>,
    pub reasoning: Vec<Value>,
    pub enable_extensions: bool,
    pub enable_extensions_auto_update: bool,
    pub enable_accounts: bool,
}

impl From<UserSettings> for UserSettingsDto {
    fn from(settings: UserSettings) -> Self {
        Self {
            data: settings.data,
        }
    }
}

impl From<UserSettingsDto> for UserSettings {
    fn from(dto: UserSettingsDto) -> Self {
        Self { data: dto.data }
    }
}

impl From<SettingsSnapshot> for SettingsSnapshotDto {
    fn from(snapshot: SettingsSnapshot) -> Self {
        Self {
            date: snapshot.date,
            name: snapshot.name,
            size: snapshot.size,
        }
    }
}

impl From<TauriTavernSettings> for TauriTavernSettingsDto {
    fn from(settings: TauriTavernSettings) -> Self {
        Self {
            updates: TauriTavernUpdateSettingsDto::from(settings.updates),
            perf_profile: settings.perf_profile,
            panel_runtime_profile: settings.panel_runtime_profile,
            embedded_runtime_profile: settings.embedded_runtime_profile,
        }
    }
}

impl From<TauriTavernUpdateSettings> for TauriTavernUpdateSettingsDto {
    fn from(settings: TauriTavernUpdateSettings) -> Self {
        Self {
            startup_popup: StartupUpdatePopupSettingsDto::from(settings.startup_popup),
        }
    }
}

impl From<StartupUpdatePopupSettings> for StartupUpdatePopupSettingsDto {
    fn from(settings: StartupUpdatePopupSettings) -> Self {
        Self {
            dismissed_release_token: settings.dismissed_release_token,
        }
    }
}
