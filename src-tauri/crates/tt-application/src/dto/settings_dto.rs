use serde::{Deserialize, Serialize};
use tt_domain::json_document::JsonDocument;
use tt_domain::models::settings::{
    AgentRunRetentionSettings, AgentSettings, AppUpdateSettings, ChatBackupSettings,
    ClaudeModelSettings, DevLoggingSettings, DynamicThemeSettings, ModelSettings, PromptCacheTtl,
    RequestProxySettings, SettingsSnapshot, TauriTavernSettings,
};
use tt_domain::models::update::UpdateChannel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernSettingsDto {
    pub updates: AppUpdateSettingsDto,
    pub perf_profile: String,
    pub panel_runtime_profile: String,
    pub embedded_runtime_profile: String,
    pub chat_virtualization_enabled: bool,
    pub cold_swipes_enabled: bool,
    pub codemirror_editor_enabled: bool,
    pub chat_backups: ChatBackupSettingsDto,
    pub close_to_tray_on_close: bool,
    pub generation_notifications_enabled: bool,
    pub request_proxy: RequestProxySettingsDto,
    pub allow_keys_exposure: bool,
    pub avatar_persona_original_images_enabled: bool,
    pub dev: DevLoggingSettingsDto,
    pub dynamic_theme: DynamicThemeSettingsDto,
    pub models: ModelSettingsDto,
    pub agent: AgentSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateSettingsDto {
    #[serde(default)]
    pub channel: Option<UpdateChannel>,
    pub popup_enabled: bool,
    pub dismissed_release_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAppUpdateSettingsDto {
    pub channel: Option<UpdateChannel>,
    pub popup_enabled: Option<bool>,
    pub dismissed_release_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTauriTavernSettingsDto {
    pub updates: Option<UpdateAppUpdateSettingsDto>,
    pub perf_profile: Option<String>,
    pub panel_runtime_profile: Option<String>,
    pub embedded_runtime_profile: Option<String>,
    pub chat_virtualization_enabled: Option<bool>,
    pub cold_swipes_enabled: Option<bool>,
    pub codemirror_editor_enabled: Option<bool>,
    pub chat_backups: Option<UpdateChatBackupSettingsDto>,
    pub close_to_tray_on_close: Option<bool>,
    pub generation_notifications_enabled: Option<bool>,
    pub avatar_persona_original_images_enabled: Option<bool>,
    pub dev: Option<UpdateDevLoggingSettingsDto>,
    pub dynamic_theme: Option<UpdateDynamicThemeSettingsDto>,
    pub models: Option<UpdateModelSettingsDto>,
    pub agent: Option<UpdateAgentSettingsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatBackupSettingsDto {
    pub automatic_enabled: bool,
    pub zstd_compression_enabled: bool,
    pub max_files_per_prefix: i64,
    pub max_total_files: i64,
    pub max_total_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatBackupSettingsDto {
    pub automatic_enabled: Option<bool>,
    pub zstd_compression_enabled: Option<bool>,
    pub max_files_per_prefix: Option<i64>,
    pub max_total_files: Option<i64>,
    pub max_total_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettingsDto {
    pub retention: AgentRunRetentionSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAgentSettingsDto {
    pub retention: Option<UpdateAgentRunRetentionSettingsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRetentionSettingsDto {
    pub auto_prune_enabled: bool,
    pub keep_recent_terminal_runs: u32,
    pub keep_full_recent_runs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAgentRunRetentionSettingsDto {
    pub auto_prune_enabled: Option<bool>,
    pub keep_recent_terminal_runs: Option<u32>,
    pub keep_full_recent_runs: Option<u32>,
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
    pub wallpaper_enabled: bool,
    pub day_wallpaper: String,
    pub night_wallpaper: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDynamicThemeSettingsDto {
    pub enabled: Option<bool>,
    pub day_theme: Option<String>,
    pub night_theme: Option<String>,
    pub wallpaper_enabled: Option<bool>,
    pub day_wallpaper: Option<String>,
    pub night_wallpaper: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestProxySettingsDto {
    pub enabled: bool,
    pub url: String,
    pub bypass: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeModelSettingsDto {
    pub prompt_cache_ttl: PromptCacheTtl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettingsDto {
    pub claude: ClaudeModelSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateClaudeModelSettingsDto {
    pub prompt_cache_ttl: Option<PromptCacheTtl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateModelSettingsDto {
    pub claude: Option<UpdateClaudeModelSettingsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsSaveResultDto {
    pub result: String,
    pub tauritavern_settings_revision: UserSettingsRevisionDto,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub persona_errors: std::collections::BTreeMap<String, String>,
}

pub use tt_domain::models::settings::revision::UserSettingsRevision as UserSettingsRevisionDto;

/// Already encoded legacy /api/settings/get response. The cache shares these bytes.
#[derive(Clone)]
pub struct SettingsJsonDto {
    pub bytes: std::sync::Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshotDto {
    pub date: i64,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SillyTavernSettingsResponseDto {
    pub settings: String,
    pub tauritavern_settings_revision: UserSettingsRevisionDto,
    pub koboldai_settings: Vec<String>,
    pub koboldai_setting_names: Vec<String>,
    pub world_names: Vec<String>,
    pub novelai_settings: Vec<String>,
    pub novelai_setting_names: Vec<String>,
    pub openai_settings: Vec<String>,
    pub openai_setting_names: Vec<String>,
    pub textgenerationwebui_presets: Vec<String>,
    pub textgenerationwebui_preset_names: Vec<String>,
    pub themes: Vec<JsonDocument>,
    #[serde(rename = "movingUIPresets")]
    pub moving_ui_presets: Vec<JsonDocument>,
    #[serde(rename = "quickReplyPresets")]
    pub quick_reply_presets: Vec<JsonDocument>,
    pub instruct: Vec<JsonDocument>,
    pub context: Vec<JsonDocument>,
    pub sysprompt: Vec<JsonDocument>,
    pub reasoning: Vec<JsonDocument>,
    pub enable_extensions: bool,
    pub enable_extensions_auto_update: bool,
    pub enable_accounts: bool,
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
            updates: AppUpdateSettingsDto::from(settings.updates),
            perf_profile: settings.perf_profile,
            panel_runtime_profile: settings.panel_runtime_profile,
            embedded_runtime_profile: settings.embedded_runtime_profile,
            chat_virtualization_enabled: settings.chat_virtualization_enabled,
            cold_swipes_enabled: settings.cold_swipes_enabled,
            codemirror_editor_enabled: settings.codemirror_editor_enabled,
            chat_backups: ChatBackupSettingsDto::from(settings.chat_backups),
            close_to_tray_on_close: settings.close_to_tray_on_close,
            generation_notifications_enabled: settings.generation_notifications_enabled,
            request_proxy: RequestProxySettingsDto::from(settings.request_proxy),
            allow_keys_exposure: settings.allow_keys_exposure,
            avatar_persona_original_images_enabled: settings.avatar_persona_original_images_enabled,
            dev: DevLoggingSettingsDto::from(settings.dev),
            dynamic_theme: DynamicThemeSettingsDto::from(settings.dynamic_theme),
            models: ModelSettingsDto::from(settings.models),
            agent: AgentSettingsDto::from(settings.agent),
        }
    }
}

impl From<ChatBackupSettings> for ChatBackupSettingsDto {
    fn from(settings: ChatBackupSettings) -> Self {
        Self {
            automatic_enabled: settings.automatic_enabled,
            zstd_compression_enabled: settings.zstd_compression_enabled,
            max_files_per_prefix: settings.max_files_per_prefix,
            max_total_files: settings.max_total_files,
            max_total_bytes: settings.max_total_bytes,
        }
    }
}

impl From<AgentSettings> for AgentSettingsDto {
    fn from(settings: AgentSettings) -> Self {
        Self {
            retention: AgentRunRetentionSettingsDto::from(settings.retention),
        }
    }
}

impl From<AgentRunRetentionSettings> for AgentRunRetentionSettingsDto {
    fn from(settings: AgentRunRetentionSettings) -> Self {
        Self {
            auto_prune_enabled: settings.auto_prune_enabled,
            keep_recent_terminal_runs: settings.keep_recent_terminal_runs,
            keep_full_recent_runs: settings.keep_full_recent_runs,
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
            wallpaper_enabled: settings.wallpaper_enabled,
            day_wallpaper: settings.day_wallpaper,
            night_wallpaper: settings.night_wallpaper,
        }
    }
}

impl From<ClaudeModelSettings> for ClaudeModelSettingsDto {
    fn from(settings: ClaudeModelSettings) -> Self {
        Self {
            prompt_cache_ttl: settings.prompt_cache_ttl,
        }
    }
}

impl From<ModelSettings> for ModelSettingsDto {
    fn from(settings: ModelSettings) -> Self {
        Self {
            claude: ClaudeModelSettingsDto::from(settings.claude),
        }
    }
}

impl From<AppUpdateSettings> for AppUpdateSettingsDto {
    fn from(settings: AppUpdateSettings) -> Self {
        Self {
            channel: settings.channel,
            popup_enabled: settings.popup_enabled,
            dismissed_release_token: settings.startup_popup.dismissed_release_token,
        }
    }
}
