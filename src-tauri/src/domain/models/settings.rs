use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

fn default_ios_policy_seed() -> Option<Value> {
    if !cfg!(target_os = "ios") {
        return None;
    }

    let profile = env!("TAURITAVERN_IOS_POLICY_PROFILE").trim();
    if profile.is_empty() {
        return None;
    }

    Some(json!({
        "version": crate::domain::ios_policy::IOS_POLICY_VERSION,
        "profile": profile,
    }))
}

fn default_perf_profile() -> String {
    "auto".to_string()
}

fn default_panel_runtime_profile() -> String {
    "off".to_string()
}

fn default_embedded_runtime_profile() -> String {
    "auto".to_string()
}

fn default_chat_history_mode() -> ChatHistoryMode {
    ChatHistoryMode::Windowed
}

fn default_llm_api_keep() -> u32 {
    5
}

fn default_avatar_persona_original_images_enabled() -> bool {
    false
}

fn default_model_settings() -> ModelSettings {
    ModelSettings::default()
}

pub const MIN_LLM_API_KEEP: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptCacheTtl {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl Default for PromptCacheTtl {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeModelSettings {
    #[serde(default)]
    pub prompt_cache_ttl: PromptCacheTtl,
}

impl Default for ClaudeModelSettings {
    fn default() -> Self {
        Self {
            prompt_cache_ttl: PromptCacheTtl::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    #[serde(default)]
    pub claude: ClaudeModelSettings,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            claude: ClaudeModelSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicThemeSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub day_theme: String,
    #[serde(default)]
    pub night_theme: String,
}

impl Default for DynamicThemeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            day_theme: String::new(),
            night_theme: String::new(),
        }
    }
}

fn default_close_to_tray_on_close() -> bool {
    cfg!(target_os = "windows")
}

fn default_request_proxy_bypass() -> Vec<String> {
    vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
        "10.0.0.0/8".to_string(),
        "172.16.0.0/12".to_string(),
        "192.168.0.0/16".to_string(),
        "169.254.0.0/16".to_string(),
        ".local".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatHistoryMode {
    Windowed,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestProxySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_request_proxy_bypass")]
    pub bypass: Vec<String>,
}

impl Default for RequestProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            bypass: default_request_proxy_bypass(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevLoggingSettings {
    #[serde(default)]
    pub frontend_console_capture: bool,
    #[serde(default = "default_llm_api_keep")]
    pub llm_api_keep: u32,
}

impl Default for DevLoggingSettings {
    fn default() -> Self {
        Self {
            frontend_console_capture: false,
            llm_api_keep: default_llm_api_keep(),
        }
    }
}

impl DevLoggingSettings {
    pub fn effective_llm_api_keep(&self) -> u32 {
        self.llm_api_keep.max(MIN_LLM_API_KEEP)
    }

    pub fn is_valid_llm_api_keep(value: u32) -> bool {
        value >= MIN_LLM_API_KEEP
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauriTavernSettings {
    pub updates: TauriTavernUpdateSettings,
    #[serde(default = "default_perf_profile")]
    pub perf_profile: String,
    #[serde(default = "default_panel_runtime_profile")]
    pub panel_runtime_profile: String,
    #[serde(default = "default_embedded_runtime_profile")]
    pub embedded_runtime_profile: String,
    #[serde(default = "default_chat_history_mode")]
    pub chat_history_mode: ChatHistoryMode,
    #[serde(default = "default_close_to_tray_on_close")]
    pub close_to_tray_on_close: bool,
    #[serde(default)]
    pub request_proxy: RequestProxySettings,
    #[serde(default)]
    pub allow_keys_exposure: bool,
    /// When enabled, `/thumbnail?type=avatar|persona` serves original images instead of
    /// cached/generated thumbnails. Background thumbnails are intentionally unaffected.
    #[serde(default = "default_avatar_persona_original_images_enabled")]
    pub avatar_persona_original_images_enabled: bool,
    #[serde(default)]
    pub dev: DevLoggingSettings,
    #[serde(default)]
    pub dynamic_theme: DynamicThemeSettings,
    #[serde(default = "default_model_settings")]
    pub models: ModelSettings,
    /// iOS-only distribution policy (profile + capability overrides).
    ///
    /// NOTE: This field is intentionally stored as raw JSON to ensure:
    /// - desktop builds can load settings exported from iOS even if the policy schema changes
    /// - iOS builds can validate the schema strictly at runtime (fail-fast) without forcing
    ///   non-iOS platforms to parse/apply it.
    #[serde(default)]
    pub ios_policy: Option<Value>,
}

impl Default for TauriTavernSettings {
    fn default() -> Self {
        Self {
            updates: TauriTavernUpdateSettings::default(),
            perf_profile: default_perf_profile(),
            panel_runtime_profile: default_panel_runtime_profile(),
            embedded_runtime_profile: default_embedded_runtime_profile(),
            chat_history_mode: default_chat_history_mode(),
            close_to_tray_on_close: default_close_to_tray_on_close(),
            request_proxy: RequestProxySettings::default(),
            allow_keys_exposure: false,
            avatar_persona_original_images_enabled: default_avatar_persona_original_images_enabled(
            ),
            dev: DevLoggingSettings::default(),
            dynamic_theme: DynamicThemeSettings::default(),
            models: default_model_settings(),
            ios_policy: default_ios_policy_seed(),
        }
    }
}

impl TauriTavernSettings {
    /// Deserializes settings while keeping backward compatibility with older
    /// `tauritavern-settings.json` schemas.
    pub fn from_json_str_with_compat(raw: &str) -> Result<Self, serde_json::Error> {
        let mut value: Value = serde_json::from_str(raw)?;

        if let Value::Object(map) = &mut value {
            // Migration: `avatar_persona_thumbnails_enabled` (legacy, default true) ->
            // `avatar_persona_original_images_enabled` (current, default false).
            //
            // The meaning is inverted: originals_enabled = !thumbnails_enabled.
            if !map.contains_key("avatar_persona_original_images_enabled") {
                let legacy_value = map.get("avatar_persona_thumbnails_enabled").cloned();
                if let Some(legacy_value) = legacy_value {
                    let thumbnails_enabled: bool = serde_json::from_value(legacy_value)?;
                    map.insert(
                        "avatar_persona_original_images_enabled".to_string(),
                        Value::Bool(!thumbnails_enabled),
                    );
                }
            }
        }

        serde_json::from_value(value)
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

#[cfg(test)]
mod tests {
    use super::{DevLoggingSettings, TauriTavernSettings};

    #[test]
    fn effective_llm_api_keep_has_minimum_of_one() {
        let settings = DevLoggingSettings {
            frontend_console_capture: false,
            llm_api_keep: 0,
        };

        assert_eq!(settings.effective_llm_api_keep(), 1);
    }

    #[test]
    fn llm_api_keep_validation_requires_positive_values() {
        assert!(!DevLoggingSettings::is_valid_llm_api_keep(0));
        assert!(DevLoggingSettings::is_valid_llm_api_keep(1));
    }

    #[test]
    fn avatar_persona_original_images_enabled_migrates_legacy_thumbnail_setting() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}},"avatar_persona_thumbnails_enabled":false}"#,
        )
        .expect("parse settings");

        assert!(settings.avatar_persona_original_images_enabled);
    }
}
