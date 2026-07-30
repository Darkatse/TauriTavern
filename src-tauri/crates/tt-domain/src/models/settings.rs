use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::models::update::UpdateChannel;

fn default_ios_policy_seed() -> Option<Value> {
    if !cfg!(target_os = "ios") {
        return None;
    }

    let profile = env!("TAURITAVERN_IOS_POLICY_PROFILE").trim();
    if profile.is_empty() {
        return None;
    }

    Some(json!({
        "version": crate::ios_policy::IOS_POLICY_VERSION,
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

fn default_llm_api_keep() -> u32 {
    5
}

fn default_avatar_persona_original_images_enabled() -> bool {
    false
}

fn default_native_regex_backend_enabled() -> bool {
    true
}

fn default_model_settings() -> ModelSettings {
    ModelSettings::default()
}

pub const MIN_LLM_API_KEEP: u32 = 1;
pub const DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS: u32 = 100;
pub const DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS: u32 = 20;
pub const MAX_AGENT_RETENTION_KEEP_RUNS: u32 = 10_000;
pub const DEFAULT_CHAT_BACKUP_MAX_FILES_PER_PREFIX: i64 = 20;
pub const DEFAULT_CHAT_BACKUP_MAX_TOTAL_FILES: i64 = 500;
pub const DEFAULT_CHAT_BACKUP_MAX_TOTAL_BYTES: i64 = 1024 * 1024 * 1024;

fn default_agent_retention_keep_recent_terminal_runs() -> u32 {
    DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS
}

fn default_agent_retention_keep_full_recent_runs() -> u32 {
    DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PromptCacheTtl {
    #[serde(rename = "off")]
    #[default]
    Off,
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelSettings {
    #[serde(default)]
    pub claude: ClaudeModelSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DynamicThemeSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub day_theme: String,
    #[serde(default)]
    pub night_theme: String,
    #[serde(default)]
    pub wallpaper_enabled: bool,
    #[serde(default)]
    pub day_wallpaper: String,
    #[serde(default)]
    pub night_wallpaper: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSettings {
    #[serde(default)]
    pub retention: AgentRunRetentionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRetentionSettings {
    #[serde(default)]
    pub auto_prune_enabled: bool,
    #[serde(default = "default_agent_retention_keep_recent_terminal_runs")]
    pub keep_recent_terminal_runs: u32,
    #[serde(default = "default_agent_retention_keep_full_recent_runs")]
    pub keep_full_recent_runs: u32,
}

impl Default for AgentRunRetentionSettings {
    fn default() -> Self {
        Self {
            auto_prune_enabled: false,
            keep_recent_terminal_runs: DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS,
            keep_full_recent_runs: DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunRetentionSettingsValidationError {
    KeepRecentTerminalRunsOutOfRange,
    KeepFullRecentRunsOutOfRange,
}

impl AgentRunRetentionSettingsValidationError {
    pub fn message(self) -> String {
        match self {
            Self::KeepRecentTerminalRunsOutOfRange => format!(
                "agent.retention_keep_recent_terminal_runs_invalid: keep_recent_terminal_runs must be between 0 and {MAX_AGENT_RETENTION_KEEP_RUNS}"
            ),
            Self::KeepFullRecentRunsOutOfRange => {
                "agent.retention_keep_full_recent_runs_invalid: keep_full_recent_runs must be between 0 and keep_recent_terminal_runs"
                    .to_string()
            }
        }
    }
}

impl AgentRunRetentionSettings {
    pub fn validate(&self) -> Result<(), AgentRunRetentionSettingsValidationError> {
        if !Self::is_valid_keep_runs(self.keep_recent_terminal_runs) {
            return Err(AgentRunRetentionSettingsValidationError::KeepRecentTerminalRunsOutOfRange);
        }

        if !Self::is_valid_full_retention(
            self.keep_full_recent_runs,
            self.keep_recent_terminal_runs,
        ) {
            return Err(AgentRunRetentionSettingsValidationError::KeepFullRecentRunsOutOfRange);
        }

        Ok(())
    }

    pub fn is_valid_keep_runs(value: u32) -> bool {
        value <= MAX_AGENT_RETENTION_KEEP_RUNS
    }

    pub fn is_valid_full_retention(
        keep_full_recent_runs: u32,
        keep_recent_terminal_runs: u32,
    ) -> bool {
        keep_full_recent_runs <= keep_recent_terminal_runs
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatBackupSettings {
    #[serde(default = "default_chat_backup_automatic_enabled")]
    pub automatic_enabled: bool,
    #[serde(default)]
    pub zstd_compression_enabled: bool,
    #[serde(default = "default_chat_backup_max_files_per_prefix")]
    pub max_files_per_prefix: i64,
    #[serde(default = "default_chat_backup_max_total_files")]
    pub max_total_files: i64,
    #[serde(default = "default_chat_backup_max_total_bytes")]
    pub max_total_bytes: i64,
}

impl Default for ChatBackupSettings {
    fn default() -> Self {
        Self {
            automatic_enabled: default_chat_backup_automatic_enabled(),
            zstd_compression_enabled: false,
            max_files_per_prefix: default_chat_backup_max_files_per_prefix(),
            max_total_files: default_chat_backup_max_total_files(),
            max_total_bytes: default_chat_backup_max_total_bytes(),
        }
    }
}

impl ChatBackupSettings {
    pub fn validate(&self) -> Result<(), ChatBackupSettingsValidationError> {
        for (field, value) in [
            ("max_files_per_prefix", self.max_files_per_prefix),
            ("max_total_files", self.max_total_files),
            ("max_total_bytes", self.max_total_bytes),
        ] {
            if value < -1 {
                return Err(ChatBackupSettingsValidationError { field });
            }
        }

        Ok(())
    }

    pub fn history_disabled(&self) -> bool {
        self.max_files_per_prefix == 0 || self.max_total_files == 0 || self.max_total_bytes == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatBackupSettingsValidationError {
    field: &'static str,
}

impl ChatBackupSettingsValidationError {
    pub fn message(self) -> String {
        format!(
            "chat_backups.{}_invalid: {} must be -1, 0, or a positive integer",
            self.field, self.field
        )
    }
}

fn default_chat_backup_automatic_enabled() -> bool {
    true
}

fn default_chat_backup_max_files_per_prefix() -> i64 {
    DEFAULT_CHAT_BACKUP_MAX_FILES_PER_PREFIX
}

fn default_chat_backup_max_total_files() -> i64 {
    DEFAULT_CHAT_BACKUP_MAX_TOTAL_FILES
}

fn default_chat_backup_max_total_bytes() -> i64 {
    DEFAULT_CHAT_BACKUP_MAX_TOTAL_BYTES
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
    #[serde(default)]
    pub chat_virtualization_enabled: bool,
    #[serde(default)]
    pub chat_backups: ChatBackupSettings,
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
    #[serde(default = "default_native_regex_backend_enabled")]
    pub native_regex_backend_enabled: bool,
    #[serde(default)]
    pub dev: DevLoggingSettings,
    #[serde(default)]
    pub dynamic_theme: DynamicThemeSettings,
    #[serde(default = "default_model_settings")]
    pub models: ModelSettings,
    #[serde(default)]
    pub agent: AgentSettings,
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
            chat_virtualization_enabled: false,
            chat_backups: ChatBackupSettings::default(),
            close_to_tray_on_close: default_close_to_tray_on_close(),
            request_proxy: RequestProxySettings::default(),
            allow_keys_exposure: false,
            avatar_persona_original_images_enabled: default_avatar_persona_original_images_enabled(
            ),
            native_regex_backend_enabled: default_native_regex_backend_enabled(),
            dev: DevLoggingSettings::default(),
            dynamic_theme: DynamicThemeSettings::default(),
            models: default_model_settings(),
            agent: AgentSettings::default(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<UpdateChannel>,
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
    use super::{
        AgentRunRetentionSettings, DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS,
        DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS,
        DEFAULT_CHAT_BACKUP_MAX_FILES_PER_PREFIX, DEFAULT_CHAT_BACKUP_MAX_TOTAL_BYTES,
        DEFAULT_CHAT_BACKUP_MAX_TOTAL_FILES, DevLoggingSettings, MAX_AGENT_RETENTION_KEEP_RUNS,
        TauriTavernSettings,
    };

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

    #[test]
    fn native_regex_backend_enabled_defaults_to_true() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}}}"#,
        )
        .expect("parse settings");

        assert!(settings.native_regex_backend_enabled);
    }

    #[test]
    fn chat_virtualization_defaults_to_disabled_and_accepts_enabled() {
        let older = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}}}"#,
        )
        .expect("parse older settings");
        assert!(!older.chat_virtualization_enabled);

        let enabled = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}},"chat_virtualization_enabled":true}"#,
        )
        .expect("parse enabled chat virtualization");
        assert!(enabled.chat_virtualization_enabled);

        let serialized = serde_json::to_value(enabled).expect("serialize chat virtualization");
        assert_eq!(serialized["chat_virtualization_enabled"], true);
    }

    #[test]
    fn removed_chat_history_mode_is_ignored() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}},"chat_history_mode":"windowed"}"#,
        )
        .expect("parse settings with removed key");

        let serialized = serde_json::to_value(settings).expect("serialize settings");
        assert!(serialized.get("chat_history_mode").is_none());
    }

    #[test]
    fn agent_retention_defaults_to_recent_terminal_history_policy() {
        let settings = TauriTavernSettings::default();

        assert!(!settings.agent.retention.auto_prune_enabled);
        assert_eq!(
            settings.agent.retention.keep_recent_terminal_runs,
            DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS
        );
        assert_eq!(
            settings.agent.retention.keep_full_recent_runs,
            DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS
        );
    }

    #[test]
    fn agent_settings_defaults_when_loading_older_settings() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}}}"#,
        )
        .expect("parse settings");

        assert!(!settings.agent.retention.auto_prune_enabled);
        assert_eq!(
            settings.agent.retention.keep_recent_terminal_runs,
            DEFAULT_AGENT_RETENTION_KEEP_RECENT_TERMINAL_RUNS
        );
        assert_eq!(
            settings.agent.retention.keep_full_recent_runs,
            DEFAULT_AGENT_RETENTION_KEEP_FULL_RECENT_RUNS
        );
    }

    #[test]
    fn chat_backup_settings_default_when_loading_older_settings() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{"updates":{"startup_popup":{"dismissed_release_token":null}}}"#,
        )
        .expect("parse settings");

        assert!(settings.chat_backups.automatic_enabled);
        assert!(!settings.chat_backups.zstd_compression_enabled);
        assert_eq!(
            settings.chat_backups.max_files_per_prefix,
            DEFAULT_CHAT_BACKUP_MAX_FILES_PER_PREFIX
        );
        assert_eq!(
            settings.chat_backups.max_total_files,
            DEFAULT_CHAT_BACKUP_MAX_TOTAL_FILES
        );
        assert_eq!(
            settings.chat_backups.max_total_bytes,
            DEFAULT_CHAT_BACKUP_MAX_TOTAL_BYTES
        );
    }

    #[test]
    fn chat_backup_settings_default_missing_nested_fields() {
        let settings = TauriTavernSettings::from_json_str_with_compat(
            r#"{
                "updates":{"startup_popup":{"dismissed_release_token":null}},
                "chat_backups":{"max_total_files":12}
            }"#,
        )
        .expect("parse settings");

        assert!(settings.chat_backups.automatic_enabled);
        assert!(!settings.chat_backups.zstd_compression_enabled);
        assert_eq!(
            settings.chat_backups.max_files_per_prefix,
            DEFAULT_CHAT_BACKUP_MAX_FILES_PER_PREFIX
        );
        assert_eq!(settings.chat_backups.max_total_files, 12);
        assert_eq!(
            settings.chat_backups.max_total_bytes,
            DEFAULT_CHAT_BACKUP_MAX_TOTAL_BYTES
        );
    }

    #[test]
    fn chat_backup_settings_accept_documented_sentinels_and_reject_lower_values() {
        let mut settings = TauriTavernSettings::default().chat_backups;

        settings.max_files_per_prefix = -1;
        settings.max_total_files = 0;
        settings.max_total_bytes = 1;
        settings
            .validate()
            .expect("accept -1, 0, and positive limits");
        assert!(settings.history_disabled());

        settings.max_total_files = -2;
        let error = settings.validate().expect_err("reject values below -1");
        assert!(error.message().contains("max_total_files"));
    }

    #[test]
    fn agent_retention_validation_caps_run_counts_and_requires_full_subset() {
        assert!(AgentRunRetentionSettings::is_valid_keep_runs(0));
        assert!(AgentRunRetentionSettings::is_valid_keep_runs(
            MAX_AGENT_RETENTION_KEEP_RUNS
        ));
        assert!(!AgentRunRetentionSettings::is_valid_keep_runs(
            MAX_AGENT_RETENTION_KEEP_RUNS + 1
        ));

        assert!(AgentRunRetentionSettings::is_valid_full_retention(20, 100));
        assert!(AgentRunRetentionSettings::is_valid_full_retention(0, 0));
        assert!(!AgentRunRetentionSettings::is_valid_full_retention(21, 20));
    }
}
