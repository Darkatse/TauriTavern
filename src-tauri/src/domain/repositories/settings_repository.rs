use crate::domain::errors::DomainError;
use crate::domain::models::settings::{AppSettings, SettingsSnapshot, UserSettings};
use async_trait::async_trait;
use std::path::PathBuf;

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    // 应用程序设置
    async fn save(&self, settings: &AppSettings) -> Result<(), DomainError>;
    async fn load(&self) -> Result<AppSettings, DomainError>;

    // 用户设置
    async fn save_user_settings(&self, settings: &UserSettings) -> Result<(), DomainError>;
    async fn load_user_settings(&self) -> Result<UserSettings, DomainError>;

    // 设置快照
    async fn create_snapshot(&self) -> Result<(), DomainError>;
    async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError>;
    async fn load_snapshot(&self, name: &str) -> Result<UserSettings, DomainError>;
    async fn restore_snapshot(&self, name: &str) -> Result<(), DomainError>;

    // 预设和主题
    async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError>;

    // AI 设置
    async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;

    // 世界信息
    async fn get_world_names(&self) -> Result<Vec<String>, DomainError>;
}
