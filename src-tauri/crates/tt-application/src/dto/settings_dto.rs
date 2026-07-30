use serde::{Deserialize, Serialize};
use serde_json::Value;
use tt_domain::models::settings::{
    AgentRunRetentionSettings, AgentSettings, ChatBackupSettings, ClaudeModelSettings,
    DevLoggingSettings, DynamicThemeSettings, ModelSettings, PromptCacheTtl, RequestProxySettings,
    SettingsSnapshot, StartupUpdatePopupSettings, TauriTavernSettings, TauriTavernUpdateSettings,
    UserSettings,
};
use tt_domain::models::update::UpdateChannel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernSettingsDto {
    pub updates: TauriTavernUpdateSettingsDto,
    pub perf_profile: String,
    pub panel_runtime_profile: String,
    pub embedded_runtime_profile: String,
    pub chat_virtualization_enabled: bool,
    pub chat_backups: ChatBackupSettingsDto,
    pub close_to_tray_on_close: bool,
    pub request_proxy: RequestProxySettingsDto,
    pub allow_keys_exposure: bool,
    pub avatar_persona_original_images_enabled: bool,
    pub native_regex_backend_enabled: bool,
    pub dev: DevLoggingSettingsDto,
    pub dynamic_theme: DynamicThemeSettingsDto,
    pub models: ModelSettingsDto,
    pub agent: AgentSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernUpdateSettingsDto {
    #[serde(default)]
    pub channel: Option<UpdateChannel>,
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
    pub chat_virtualization_enabled: Option<bool>,
    pub chat_backups: Option<UpdateChatBackupSettingsDto>,
    pub close_to_tray_on_close: Option<bool>,
    pub request_proxy: Option<RequestProxySettingsDto>,
    pub allow_keys_exposure: Option<bool>,
    pub avatar_persona_original_images_enabled: Option<bool>,
    pub native_regex_backend_enabled: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettingsDto {
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsPatchDto {
    pub hash_algorithm: String,
    pub base_hash: String,
    pub ops: Vec<UserSettingsPatchOpDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum UserSettingsPatchOpDto {
    Set { path: Vec<String>, value: Value },
    Delete { path: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsSaveResultDto {
    pub result: String,
    pub mode: String,
    pub hash_algorithm: String,
    pub settings_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsRevisionDto {
    pub hash_algorithm: String,
    pub settings_hash: String,
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
            chat_virtualization_enabled: settings.chat_virtualization_enabled,
            chat_backups: ChatBackupSettingsDto::from(settings.chat_backups),
            close_to_tray_on_close: settings.close_to_tray_on_close,
            request_proxy: RequestProxySettingsDto::from(settings.request_proxy),
            allow_keys_exposure: settings.allow_keys_exposure,
            avatar_persona_original_images_enabled: settings.avatar_persona_original_images_enabled,
            native_regex_backend_enabled: settings.native_regex_backend_enabled,
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

impl From<TauriTavernUpdateSettings> for TauriTavernUpdateSettingsDto {
    fn from(settings: TauriTavernUpdateSettings) -> Self {
        Self {
            channel: settings.channel,
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
