use async_trait::async_trait;
use tt_contracts::sync_automation::{
    ScheduledSyncRule, SyncAutomationStatus, SyncAutomationTarget, SyncAutomationToastEvent,
};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::LanServerSettings;
use ttsync_contract::sync::SyncMode;

#[async_trait]
pub trait SyncAutomationRuleRepository: Send + Sync {
    async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError>;
    async fn save_rule(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError>;
}

#[async_trait]
pub trait SyncAutomationLanSettingsRepository: Send + Sync {
    async fn load_or_create_server_settings(&self) -> Result<LoadedLanServerSettings, DomainError>;
    async fn save_server_settings(&self, settings: &LanServerSettings) -> Result<(), DomainError>;
    async fn load_manual_default_sync_mode(&self) -> Result<SyncMode, DomainError>;
}

#[async_trait]
pub trait SyncAutomationEndpointCatalog: Send + Sync {
    async fn validate_target(
        &self,
        target: &SyncAutomationTarget,
        mode: SyncMode,
    ) -> Result<(), DomainError>;
}

#[async_trait]
pub trait SyncAutomationLanServerControl: Send + Sync {
    fn validate_allowed(&self) -> Result<(), DomainError>;
    async fn start(&self) -> Result<(), DomainError>;
    async fn ensure_running(&self) -> Result<(), DomainError>;
}

pub trait SyncAutomationEventPublisher: Send + Sync {
    fn publish_status(&self, status: SyncAutomationStatus);
    fn publish_toast(&self, event: SyncAutomationToastEvent);
}

pub struct LoadedScheduledSyncRule {
    pub rule: ScheduledSyncRule,
    pub legacy_lan_server_auto_start: Option<bool>,
    pub legacy_missing_sync_mode: bool,
    pub rewrite_canonical: bool,
}

pub struct LoadedLanServerSettings {
    pub settings: LanServerSettings,
    pub created: bool,
}

impl LoadedScheduledSyncRule {
    pub fn new(rule: ScheduledSyncRule) -> Self {
        Self {
            rule,
            legacy_lan_server_auto_start: None,
            legacy_missing_sync_mode: false,
            rewrite_canonical: false,
        }
    }
}
