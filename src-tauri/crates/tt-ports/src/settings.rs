use async_trait::async_trait;
pub use tt_contracts::chat::ChatBackupStorageStats;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::{ChatBackupSettings, RequestProxySettings};

pub trait RequestProxyRuntime: Send + Sync {
    fn validate_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError>;

    fn apply_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError>;
}

#[async_trait]
pub trait ChatBackupRuntime: Send + Sync {
    async fn apply_chat_backup_settings(
        &self,
        settings: ChatBackupSettings,
    ) -> Result<(), DomainError>;

    async fn reconcile_chat_backups(&self) -> Result<(), DomainError>;

    async fn get_chat_backup_storage_stats(
        &self,
    ) -> Result<Option<ChatBackupStorageStats>, DomainError>;
}
