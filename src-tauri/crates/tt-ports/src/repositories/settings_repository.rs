use async_trait::async_trait;
use tt_contracts::byte_commit::CommitBegin;
use tt_domain::errors::DomainError;
use tt_domain::models::persona::Personas;
use tt_domain::models::settings::revision::UserSettingsRevision;
use tt_domain::models::settings::{SettingsSnapshot, TauriTavernSettings, UserSettings};

pub struct SettingsCommitResult {
    pub revision: UserSettingsRevision,
    pub personas: Personas,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsAggregateSignature(String);

impl SettingsAggregateSignature {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn save_tauritavern_settings(
        &self,
        settings: &TauriTavernSettings,
    ) -> Result<(), DomainError>;
    async fn load_tauritavern_settings(&self) -> Result<TauriTavernSettings, DomainError>;

    async fn save_user_settings(&self, settings: UserSettings) -> Result<(), DomainError>;
    async fn load_user_settings(&self) -> Result<UserSettings, DomainError>;

    async fn begin_commit(
        &self,
        expected: Option<UserSettingsRevision>,
    ) -> Result<CommitBegin, DomainError>;
    async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError>;
    async fn finish_commit(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<SettingsCommitResult, DomainError>;
    async fn abort_commit(&self, session_id: &str) -> Result<(), DomainError>;

    async fn create_snapshot(&self, settings: UserSettings) -> Result<(), DomainError>;
    async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError>;
    async fn load_snapshot(&self, name: &str) -> Result<UserSettings, DomainError>;

    async fn get_sillytavern_settings_signature(
        &self,
    ) -> Result<SettingsAggregateSignature, DomainError>;

    async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError>;
    async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError>;

    async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;
    async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError>;

    async fn get_world_names(&self) -> Result<Vec<String>, DomainError>;
}
