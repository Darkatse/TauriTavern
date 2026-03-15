use serde_json::Value;
use std::sync::Arc;

use crate::application::dto::settings_dto::{
    SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateTauriTavernSettingsDto, UserSettingsDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::settings_repository::SettingsRepository;

pub struct SettingsService {
    settings_repository: Arc<dyn SettingsRepository>,
}

impl SettingsService {
    pub fn new(settings_repository: Arc<dyn SettingsRepository>) -> Self {
        Self {
            settings_repository,
        }
    }

    pub async fn get_tauritavern_settings(
        &self,
    ) -> Result<TauriTavernSettingsDto, ApplicationError> {
        tracing::debug!("Getting TauriTavern settings");

        let settings = self.settings_repository.load_tauritavern_settings().await?;

        Ok(TauriTavernSettingsDto::from(settings))
    }

    pub async fn update_tauritavern_settings(
        &self,
        dto: UpdateTauriTavernSettingsDto,
    ) -> Result<TauriTavernSettingsDto, ApplicationError> {
        tracing::debug!("Updating TauriTavern settings");

        let mut settings = self.settings_repository.load_tauritavern_settings().await?;

        if let Some(updates) = dto.updates {
            settings.updates.startup_popup.dismissed_release_token =
                updates.startup_popup.dismissed_release_token;
        }

        if let Some(perf_profile) = dto.perf_profile {
            settings.perf_profile = perf_profile;
        }

        if let Some(panel_runtime_profile) = dto.panel_runtime_profile {
            settings.panel_runtime_profile = panel_runtime_profile;
        }

        self.settings_repository
            .save_tauritavern_settings(&settings)
            .await?;

        Ok(TauriTavernSettingsDto::from(settings))
    }

    pub async fn save_user_settings(
        &self,
        settings: UserSettingsDto,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Saving user settings");

        let user_settings = settings.into();
        self.settings_repository
            .save_user_settings(&user_settings)
            .await?;

        Ok(())
    }

    pub async fn get_sillytavern_settings(
        &self,
    ) -> Result<SillyTavernSettingsResponseDto, ApplicationError> {
        tracing::info!("Getting SillyTavern settings");

        let user_settings = self.settings_repository.load_user_settings().await?;
        let settings_json = serde_json::to_string(&user_settings.data).map_err(|e| {
            ApplicationError::InternalError(format!("Failed to serialize settings: {}", e))
        })?;

        let (koboldai_settings, koboldai_setting_names) =
            self.settings_repository.get_koboldai_settings().await?;

        let (novelai_settings, novelai_setting_names) =
            self.settings_repository.get_novelai_settings().await?;

        let (openai_settings, openai_setting_names) =
            self.settings_repository.get_openai_settings().await?;

        let (textgen_settings, textgen_setting_names) =
            self.settings_repository.get_textgen_settings().await?;

        let world_names = self.settings_repository.get_world_names().await?;

        let themes = self.settings_repository.get_themes().await?;
        let themes_json: Vec<Value> = themes.into_iter().map(|t| t.data).collect();

        let moving_ui_presets = self.settings_repository.get_moving_ui_presets().await?;
        let moving_ui_presets_json: Vec<Value> =
            moving_ui_presets.into_iter().map(|p| p.data).collect();

        let quick_reply_presets = self.settings_repository.get_quick_reply_presets().await?;
        let quick_reply_presets_json: Vec<Value> =
            quick_reply_presets.into_iter().map(|p| p.data).collect();

        let instruct_presets = self.settings_repository.get_instruct_presets().await?;
        let instruct_presets_json: Vec<Value> =
            instruct_presets.into_iter().map(|p| p.data).collect();

        let context_presets = self.settings_repository.get_context_presets().await?;
        let context_presets_json: Vec<Value> =
            context_presets.into_iter().map(|p| p.data).collect();

        let sysprompt_presets = self.settings_repository.get_sysprompt_presets().await?;
        let sysprompt_presets_json: Vec<Value> =
            sysprompt_presets.into_iter().map(|p| p.data).collect();

        let reasoning_presets = self.settings_repository.get_reasoning_presets().await?;
        let reasoning_presets_json: Vec<Value> =
            reasoning_presets.into_iter().map(|p| p.data).collect();

        let response = SillyTavernSettingsResponseDto {
            settings: settings_json,
            koboldai_settings,
            koboldai_setting_names,
            world_names,
            novelai_settings,
            novelai_setting_names,
            openai_settings,
            openai_setting_names,
            textgenerationwebui_presets: textgen_settings,
            textgenerationwebui_preset_names: textgen_setting_names,
            themes: themes_json,
            moving_ui_presets: moving_ui_presets_json,
            quick_reply_presets: quick_reply_presets_json,
            instruct: instruct_presets_json,
            context: context_presets_json,
            sysprompt: sysprompt_presets_json,
            reasoning: reasoning_presets_json,
            enable_extensions: true,
            enable_extensions_auto_update: true,
            enable_accounts: false,
        };

        Ok(response)
    }

    pub async fn create_snapshot(&self) -> Result<(), ApplicationError> {
        tracing::info!("Creating settings snapshot");

        self.settings_repository.create_snapshot().await?;

        Ok(())
    }

    pub async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshotDto>, ApplicationError> {
        tracing::info!("Getting settings snapshots");

        let snapshots = self.settings_repository.get_snapshots().await?;
        let snapshot_dtos = snapshots
            .into_iter()
            .map(SettingsSnapshotDto::from)
            .collect();

        Ok(snapshot_dtos)
    }

    pub async fn load_snapshot(&self, name: &str) -> Result<UserSettingsDto, ApplicationError> {
        tracing::info!("Loading settings snapshot: {}", name);

        let settings = self.settings_repository.load_snapshot(name).await?;

        Ok(UserSettingsDto::from(settings))
    }

    pub async fn restore_snapshot(&self, name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Restoring settings snapshot: {}", name);

        self.settings_repository.restore_snapshot(name).await?;

        Ok(())
    }
}
