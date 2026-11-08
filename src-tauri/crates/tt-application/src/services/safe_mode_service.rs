use std::sync::Arc;

use tt_contracts::safe_mode::SafeModeReport;
use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionType;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::host_resource::UserCssStore;
use tt_ports::repositories::avatar_repository::AvatarRepository;
use tt_ports::repositories::extension_repository::ExtensionRepository;
use tt_ports::repositories::settings_repository::SettingsRepository;
use tt_ports::sync_automation::SyncAutomationRuleRepository;

use crate::errors::ApplicationError;
use crate::services::settings_service::snapshot_user_settings;

pub struct SafeModeService {
    settings: Arc<dyn SettingsRepository>,
    avatars: Arc<dyn AvatarRepository>,
    extensions: Arc<dyn ExtensionRepository>,
    user_css: Arc<dyn UserCssStore>,
    sync_rules: Arc<dyn SyncAutomationRuleRepository>,
    factory_user_css: Vec<u8>,
}

impl SafeModeService {
    pub fn new(
        settings: Arc<dyn SettingsRepository>,
        avatars: Arc<dyn AvatarRepository>,
        extensions: Arc<dyn ExtensionRepository>,
        user_css: Arc<dyn UserCssStore>,
        sync_rules: Arc<dyn SyncAutomationRuleRepository>,
        factory_user_css: Vec<u8>,
    ) -> Self {
        Self {
            settings,
            avatars,
            extensions,
            user_css,
            sync_rules,
            factory_user_css,
        }
    }

    /// Nothing is reset until the snapshot is written and scheduled upload is off, and every
    /// read precedes the first reset, so a failure leaves settings either intact or recoverable.
    pub async fn rescue(&self) -> Result<SafeModeReport, ApplicationError> {
        let snapshot =
            snapshot_user_settings(self.settings.as_ref(), self.avatars.as_ref()).await?;
        let auto_upload_paused = self.pause_auto_upload().await?;

        let third_party: Vec<String> = self
            .extensions
            .discover_extensions()
            .await?
            .into_iter()
            .filter(|extension| {
                matches!(
                    extension.extension_type,
                    ExtensionType::Local | ExtensionType::Global
                )
            })
            .map(|extension| extension.name)
            .collect();
        let chat_backups = self.kept_chat_backups().await?;

        self.settings.reset_user_settings(&third_party).await?;
        self.settings
            .reset_tauritavern_settings(chat_backups)
            .await?;
        self.user_css.reset_user_css(&self.factory_user_css).await?;

        tracing::info!(
            disabled_extensions = third_party.len(),
            auto_upload_paused,
            "Safe mode reset the settings; the previous ones are in {snapshot}"
        );
        Ok(SafeModeReport { auto_upload_paused })
    }

    /// Limits decide which chat backups survive, so they outlast the reset; limits that cannot be
    /// read or would fail startup become unlimited, which deletes nothing.
    async fn kept_chat_backups(&self) -> Result<ChatBackupSettings, ApplicationError> {
        match self.settings.load_tauritavern_settings().await {
            Ok(settings) if settings.chat_backups.validate().is_ok() => Ok(settings.chat_backups),
            Ok(_) | Err(DomainError::InvalidData(_)) => Ok(ChatBackupSettings {
                max_files_per_prefix: -1,
                max_total_files: -1,
                max_total_bytes: -1,
                ..ChatBackupSettings::default()
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// The scheduler skips a rule it cannot decode, so such a rule has nothing to pause.
    async fn pause_auto_upload(&self) -> Result<bool, ApplicationError> {
        let mut loaded = match self.sync_rules.load_or_create_rule().await {
            Ok(loaded) => loaded,
            Err(DomainError::InvalidData(_)) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if !loaded.rule.enabled {
            return Ok(false);
        }
        loaded.rule.enabled = false;
        self.sync_rules.save_rule(&loaded.rule).await?;
        Ok(true)
    }
}
