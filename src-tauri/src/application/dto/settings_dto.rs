use crate::domain::models::settings::{
    ChatHistoryMode, DevLoggingSettings, DynamicThemeSettings, RequestProxySettings,
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
    pub chat_history_mode: ChatHistoryMode,
    pub close_to_tray_on_close: bool,
    pub request_proxy: RequestProxySettingsDto,
    pub allow_keys_exposure: bool,
    pub avatar_persona_original_images_enabled: bool,
    pub dev: DevLoggingSettingsDto,
    pub dynamic_theme: DynamicThemeSettingsDto,
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
    pub chat_history_mode: Option<ChatHistoryMode>,
    pub close_to_tray_on_close: Option<bool>,
    pub request_proxy: Option<RequestProxySettingsDto>,
    pub allow_keys_exposure: Option<bool>,
    pub avatar_persona_original_images_enabled: Option<bool>,
    pub dev: Option<UpdateDevLoggingSettingsDto>,
    pub dynamic_theme: Option<UpdateDynamicThemeSettingsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevLoggingSettingsDto {
    pub frontend_console_capture: bool,
    pub llm_api_keep: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDevLoggingSettingsDto {
    pub frontend_console_capture: Option<bool>,
    pub llm_api_keep: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicThemeSettingsDto {
    pub enabled: bool,
    pub day_theme: String,
    pub night_theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDynamicThemeSettingsDto {
    pub enabled: Option<bool>,
    pub day_theme: Option<String>,
    pub night_theme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestProxySettingsDto {
    pub enabled: bool,
    pub url: String,
    pub bypass: Vec<String>,
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
            chat_history_mode: settings.chat_history_mode,
            close_to_tray_on_close: settings.close_to_tray_on_close,
            request_proxy: RequestProxySettingsDto::from(settings.request_proxy),
            allow_keys_exposure: settings.allow_keys_exposure,
            avatar_persona_original_images_enabled: settings.avatar_persona_original_images_enabled,
            dev: DevLoggingSettingsDto::from(settings.dev),
            dynamic_theme: DynamicThemeSettingsDto::from(settings.dynamic_theme),
        }
    }
}

impl From<DevLoggingSettings> for DevLoggingSettingsDto {
    fn from(settings: DevLoggingSettings) -> Self {
        Self {
            frontend_console_capture: settings.frontend_console_capture,
            llm_api_keep: settings.effective_llm_api_keep(),
        }
    }
}

impl From<RequestProxySettings> for RequestProxySettingsDto {
    fn from(settings: RequestProxySettings) -> Self {
        Self {
            enabled: settings.enabled,
            url: settings.url,
            bypass: settings.bypass,
        }
    }
}

impl From<RequestProxySettingsDto> for RequestProxySettings {
    fn from(dto: RequestProxySettingsDto) -> Self {
        Self {
            enabled: dto.enabled,
            url: dto.url,
            bypass: dto.bypass,
        }
    }
}

impl From<DynamicThemeSettings> for DynamicThemeSettingsDto {
    fn from(settings: DynamicThemeSettings) -> Self {
        Self {
            enabled: settings.enabled,
            day_theme: settings.day_theme,
            night_theme: settings.night_theme,
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
