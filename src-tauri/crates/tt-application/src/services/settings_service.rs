use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tt_domain::models::persona::{Personas, insert_personas, take_personas};
use tt_ports::repositories::avatar_repository::AvatarRepository;

use crate::dto::settings_dto::{
    SettingsJsonDto, SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateAgentSettingsDto, UpdateChatBackupSettingsDto, UpdateTauriTavernSettingsDto,
    UserSettingsDto, UserSettingsRevisionDto, UserSettingsSaveResultDto,
};
use crate::errors::ApplicationError;
use tt_contracts::byte_commit::CommitBegin;
use tt_domain::models::settings::{
    AgentRunRetentionSettings, AgentSettings, ChatBackupSettings, DevLoggingSettings,
    RequestProxySettings, UserSettings,
};
use tt_ports::repositories::settings_repository::{SettingsAggregateSignature, SettingsRepository};
pub use tt_ports::settings::{ChatBackupRuntime, ChatBackupStorageStats, RequestProxyRuntime};

#[derive(Clone)]
struct SettingsAggregateCacheEntry {
    signature: SettingsAggregateSignature,
    response: SettingsJsonDto,
}

pub struct SettingsService {
    avatars: Arc<dyn AvatarRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
    chat_backup_runtime: Arc<dyn ChatBackupRuntime>,
    sillytavern_settings_cache: Mutex<Option<SettingsAggregateCacheEntry>>,
    user_settings_save_lock: Mutex<()>,
}

impl SettingsService {
    pub fn new(
        avatars: Arc<dyn AvatarRepository>,
        settings_repository: Arc<dyn SettingsRepository>,
        request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
        chat_backup_runtime: Arc<dyn ChatBackupRuntime>,
    ) -> Self {
        Self {
            avatars,
            settings_repository,
            request_proxy_runtime,
            chat_backup_runtime,
            sillytavern_settings_cache: Mutex::new(None),
            user_settings_save_lock: Mutex::new(()),
        }
    }

    async fn clear_sillytavern_settings_cache(&self) {
        *self.sillytavern_settings_cache.lock().await = None;
    }

    pub async fn reload(&self) -> Result<(), tt_domain::errors::DomainError> {
        let _guard = self.user_settings_save_lock.lock().await;
        self.clear_sillytavern_settings_cache().await;
        self.settings_repository.load_user_settings().await?;
        let settings = self.settings_repository.load_tauritavern_settings().await?;
        self.chat_backup_runtime
            .apply_chat_backup_settings(settings.chat_backups)
            .await?;
        self.schedule_chat_backup_reconciliation();
        Ok(())
    }

    pub fn schedule_chat_backup_reconciliation(&self) {
        const RETRY_DELAY: Duration = Duration::from_secs(1);

        let runtime = Arc::clone(&self.chat_backup_runtime);
        tokio::spawn(async move {
            if let Err(first_error) = runtime.reconcile_chat_backups().await {
                tracing::warn!(
                    "Chat backup maintenance failed; retrying once: {}",
                    first_error
                );
                tokio::time::sleep(RETRY_DELAY).await;
                if let Err(error) = runtime.reconcile_chat_backups().await {
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Chat backup maintenance failed: {}",
                        error
                    );
                }
            }
        });
    }

    pub async fn get_tauritavern_settings(
        &self,
    ) -> Result<TauriTavernSettingsDto, ApplicationError> {
        tracing::debug!("Getting TauriTavern settings");

        let settings = self.settings_repository.load_tauritavern_settings().await?;

        Ok(TauriTavernSettingsDto::from(settings))
    }

    pub async fn get_chat_backup_storage_stats(
        &self,
    ) -> Result<Option<ChatBackupStorageStats>, ApplicationError> {
        match self
            .chat_backup_runtime
            .get_chat_backup_storage_stats()
            .await
        {
            Ok(stats) => Ok(stats),
            Err(error) => {
                tracing::warn!("Chat backup storage stats are unavailable: {error}");
                Ok(None)
            }
        }
    }

    pub async fn update_tauritavern_settings(
        &self,
        dto: UpdateTauriTavernSettingsDto,
    ) -> Result<TauriTavernSettingsDto, ApplicationError> {
        tracing::debug!("Updating TauriTavern settings");

        let request_proxy_update = dto.request_proxy.clone().map(RequestProxySettings::from);
        if let Some(settings) = request_proxy_update.as_ref() {
            self.request_proxy_runtime
                .validate_request_proxy_settings(settings)?;
        }

        let mut settings = self.settings_repository.load_tauritavern_settings().await?;

        if let Some(updates) = dto.updates {
            if let Some(channel) = updates.channel {
                settings.updates.channel = Some(channel);
            }
            settings.updates.startup_popup.dismissed_release_token =
                updates.startup_popup.dismissed_release_token;
        }

        if let Some(perf_profile) = dto.perf_profile {
            settings.perf_profile = perf_profile;
        }

        if let Some(panel_runtime_profile) = dto.panel_runtime_profile {
            settings.panel_runtime_profile = panel_runtime_profile;
        }

        if let Some(embedded_runtime_profile) = dto.embedded_runtime_profile {
            settings.embedded_runtime_profile = embedded_runtime_profile;
        }

        if let Some(cold_swipes_enabled) = dto.cold_swipes_enabled {
            settings.cold_swipes_enabled = cold_swipes_enabled;
        }

        if let Some(chat_virtualization_enabled) = dto.chat_virtualization_enabled {
            settings.chat_virtualization_enabled = chat_virtualization_enabled;
        }

        if let Some(codemirror_editor_enabled) = dto.codemirror_editor_enabled {
            settings.codemirror_editor_enabled = codemirror_editor_enabled;
        }

        let previous_chat_backups = settings.chat_backups;
        if let Some(chat_backups) = dto.chat_backups {
            Self::apply_chat_backup_settings_update(&mut settings.chat_backups, chat_backups)?;
        }
        let chat_backups_changed = settings.chat_backups != previous_chat_backups;
        let chat_backup_reconciliation_required = settings.chat_backups.zstd_compression_enabled
            != previous_chat_backups.zstd_compression_enabled
            || settings.chat_backups.max_files_per_prefix
                != previous_chat_backups.max_files_per_prefix
            || settings.chat_backups.max_total_files != previous_chat_backups.max_total_files
            || settings.chat_backups.max_total_bytes != previous_chat_backups.max_total_bytes;

        if let Some(close_to_tray_on_close) = dto.close_to_tray_on_close {
            settings.close_to_tray_on_close = close_to_tray_on_close;
        }

        if let Some(request_proxy) = dto.request_proxy {
            settings.request_proxy = request_proxy.into();
        }

        if let Some(allow_keys_exposure) = dto.allow_keys_exposure {
            settings.allow_keys_exposure = allow_keys_exposure;
        }

        if let Some(avatar_persona_original_images_enabled) =
            dto.avatar_persona_original_images_enabled
        {
            settings.avatar_persona_original_images_enabled =
                avatar_persona_original_images_enabled;
        }

        if let Some(dev) = dto.dev {
            if let Some(frontend_console_capture) = dev.frontend_console_capture {
                settings.dev.frontend_console_capture = frontend_console_capture;
            }

            if let Some(llm_api_keep) = dev.llm_api_keep {
                if !DevLoggingSettings::is_valid_llm_api_keep(llm_api_keep) {
                    return Err(ApplicationError::ValidationError(
                        "LLM API keep must be a positive number".to_string(),
                    ));
                }
                settings.dev.llm_api_keep = llm_api_keep;
            }
        }

        if let Some(dynamic_theme) = dto.dynamic_theme {
            if let Some(enabled) = dynamic_theme.enabled {
                settings.dynamic_theme.enabled = enabled;
            }

            if let Some(day_theme) = dynamic_theme.day_theme {
                settings.dynamic_theme.day_theme = day_theme;
            }

            if let Some(night_theme) = dynamic_theme.night_theme {
                settings.dynamic_theme.night_theme = night_theme;
            }

            if let Some(wallpaper_enabled) = dynamic_theme.wallpaper_enabled {
                settings.dynamic_theme.wallpaper_enabled = wallpaper_enabled;
            }

            if let Some(day_wallpaper) = dynamic_theme.day_wallpaper {
                settings.dynamic_theme.day_wallpaper = day_wallpaper;
            }

            if let Some(night_wallpaper) = dynamic_theme.night_wallpaper {
                settings.dynamic_theme.night_wallpaper = night_wallpaper;
            }

            if settings.dynamic_theme.enabled {
                if settings.dynamic_theme.day_theme.trim().is_empty() {
                    return Err(ApplicationError::ValidationError(
                        "Dynamic theme day theme is required".to_string(),
                    ));
                }

                if settings.dynamic_theme.night_theme.trim().is_empty() {
                    return Err(ApplicationError::ValidationError(
                        "Dynamic theme night theme is required".to_string(),
                    ));
                }
            }

            if settings.dynamic_theme.wallpaper_enabled {
                if settings.dynamic_theme.day_wallpaper.trim().is_empty() {
                    return Err(ApplicationError::ValidationError(
                        "Dynamic wallpaper day wallpaper is required".to_string(),
                    ));
                }

                if settings.dynamic_theme.night_wallpaper.trim().is_empty() {
                    return Err(ApplicationError::ValidationError(
                        "Dynamic wallpaper night wallpaper is required".to_string(),
                    ));
                }
            }
        }

        if let Some(models) = dto.models
            && let Some(claude) = models.claude
            && let Some(prompt_cache_ttl) = claude.prompt_cache_ttl
        {
            settings.models.claude.prompt_cache_ttl = prompt_cache_ttl;
        }

        if let Some(agent) = dto.agent {
            Self::apply_agent_settings_update(&mut settings.agent, agent)?;
        }

        self.settings_repository
            .save_tauritavern_settings(&settings)
            .await?;

        let request_proxy_result = request_proxy_update.is_some().then(|| {
            self.request_proxy_runtime
                .apply_request_proxy_settings(&settings.request_proxy)
        });
        let chat_backup_result = if chat_backups_changed {
            let result = self
                .chat_backup_runtime
                .apply_chat_backup_settings(settings.chat_backups)
                .await;
            if result.is_ok() && chat_backup_reconciliation_required {
                self.schedule_chat_backup_reconciliation();
            }
            Some(result)
        } else {
            None
        };
        if let Some(result) = request_proxy_result {
            result?;
        }
        if let Some(result) = chat_backup_result {
            result?;
        }

        Ok(TauriTavernSettingsDto::from(settings))
    }

    fn apply_agent_settings_update(
        settings: &mut AgentSettings,
        dto: UpdateAgentSettingsDto,
    ) -> Result<(), ApplicationError> {
        if let Some(retention) = dto.retention {
            let mut next = settings.retention.clone();

            if let Some(auto_prune_enabled) = retention.auto_prune_enabled {
                next.auto_prune_enabled = auto_prune_enabled;
            }

            if let Some(keep_recent_terminal_runs) = retention.keep_recent_terminal_runs {
                next.keep_recent_terminal_runs = keep_recent_terminal_runs;
            }

            if let Some(keep_full_recent_runs) = retention.keep_full_recent_runs {
                next.keep_full_recent_runs = keep_full_recent_runs;
            }

            validate_agent_retention_settings(&next)?;
            settings.retention = next;
        }

        Ok(())
    }

    fn apply_chat_backup_settings_update(
        settings: &mut ChatBackupSettings,
        dto: UpdateChatBackupSettingsDto,
    ) -> Result<(), ApplicationError> {
        let mut next = *settings;
        if let Some(automatic_enabled) = dto.automatic_enabled {
            next.automatic_enabled = automatic_enabled;
        }
        if let Some(zstd_compression_enabled) = dto.zstd_compression_enabled {
            next.zstd_compression_enabled = zstd_compression_enabled;
        }
        if let Some(max_files_per_prefix) = dto.max_files_per_prefix {
            next.max_files_per_prefix = max_files_per_prefix;
        }
        if let Some(max_total_files) = dto.max_total_files {
            next.max_total_files = max_total_files;
        }
        if let Some(max_total_bytes) = dto.max_total_bytes {
            next.max_total_bytes = max_total_bytes;
        }
        next.validate()
            .map_err(|error| ApplicationError::ValidationError(error.message()))?;
        *settings = next;
        Ok(())
    }

    pub async fn begin_commit(
        &self,
        expected: Option<UserSettingsRevisionDto>,
    ) -> Result<CommitBegin, ApplicationError> {
        Ok(self.settings_repository.begin_commit(expected).await?)
    }

    pub async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, ApplicationError> {
        Ok(self
            .settings_repository
            .append_commit(session_id, offset, bytes)
            .await?)
    }

    pub async fn finish_commit(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<UserSettingsSaveResultDto, ApplicationError> {
        let _guard = self.user_settings_save_lock.lock().await;
        self.clear_sillytavern_settings_cache().await;
        let result = self
            .settings_repository
            .finish_commit(session_id, expected_size)
            .await?;
        let persona_errors = self.save_personas(&result.personas).await;
        Ok(UserSettingsSaveResultDto {
            result: if persona_errors.is_empty() {
                "ok"
            } else {
                "partial"
            }
            .into(),
            tauritavern_settings_revision: result.revision,
            persona_errors,
        })
    }

    pub async fn abort_commit(&self, session_id: &str) -> Result<(), ApplicationError> {
        Ok(self.settings_repository.abort_commit(session_id).await?)
    }

    async fn save_personas(&self, personas: &Personas) -> BTreeMap<String, String> {
        let mut errors = BTreeMap::new();
        for (id, persona) in personas {
            if let Err(error) = self.avatars.save_persona(id, persona).await {
                errors.insert(id.clone(), error.to_string());
            }
        }
        if !personas.is_empty() {
            self.clear_sillytavern_settings_cache().await;
        }
        errors
    }

    pub async fn get_sillytavern_settings(&self) -> Result<SettingsJsonDto, ApplicationError> {
        tracing::info!("Getting SillyTavern settings");

        let signature = self
            .settings_repository
            .get_sillytavern_settings_signature()
            .await?;
        let mut cache = self.sillytavern_settings_cache.lock().await;
        if let Some(entry) = cache.as_ref()
            && entry.signature == signature
        {
            tracing::debug!("Using cached SillyTavern settings aggregate");
            return Ok(entry.response.clone());
        }

        *cache = None;
        let response = self.build_sillytavern_settings_response().await?;
        let bytes = tokio::task::spawn_blocking(move || serde_json::to_vec(&response))
            .await
            .map_err(|error| ApplicationError::InternalError(error.to_string()))?
            .map_err(|error| {
                ApplicationError::InternalError(format!(
                    "Failed to encode settings response: {error}"
                ))
            })?;
        let response = SettingsJsonDto {
            bytes: bytes.into(),
        };
        *cache = Some(SettingsAggregateCacheEntry {
            signature,
            response: response.clone(),
        });

        Ok(response)
    }

    async fn build_sillytavern_settings_response(
        &self,
    ) -> Result<SillyTavernSettingsResponseDto, ApplicationError> {
        let settings_json = async {
            let mut user_settings = self.settings_repository.load_user_settings().await?;
            let revision = UserSettingsRevisionDto::from_settings(&user_settings)?;
            insert_personas(&mut user_settings.data, &self.avatars.get_personas().await?);
            let settings_json = serde_json::to_string(&user_settings.data).map_err(|error| {
                ApplicationError::InternalError(format!("Failed to serialize settings: {}", error))
            })?;

            Ok::<_, ApplicationError>((settings_json, revision))
        };

        let ai_settings = async {
            let (koboldai, novelai, openai, textgen) = tokio::try_join!(
                self.settings_repository.get_koboldai_settings(),
                self.settings_repository.get_novelai_settings(),
                self.settings_repository.get_openai_settings(),
                self.settings_repository.get_textgen_settings(),
            )?;

            Ok::<_, ApplicationError>((koboldai, novelai, openai, textgen))
        };

        let presets = async {
            let (
                themes,
                moving_ui_presets,
                quick_reply_presets,
                instruct_presets,
                context_presets,
                sysprompt_presets,
                reasoning_presets,
            ) = tokio::try_join!(
                self.settings_repository.get_themes(),
                self.settings_repository.get_moving_ui_presets(),
                self.settings_repository.get_quick_reply_presets(),
                self.settings_repository.get_instruct_presets(),
                self.settings_repository.get_context_presets(),
                self.settings_repository.get_sysprompt_presets(),
                self.settings_repository.get_reasoning_presets(),
            )?;

            Ok::<_, ApplicationError>((
                themes,
                moving_ui_presets,
                quick_reply_presets,
                instruct_presets,
                context_presets,
                sysprompt_presets,
                reasoning_presets,
            ))
        };

        let world_names =
            async { Ok::<_, ApplicationError>(self.settings_repository.get_world_names().await?) };

        let (
            (settings_json, tauritavern_settings_revision),
            (
                (koboldai_settings, koboldai_setting_names),
                (novelai_settings, novelai_setting_names),
                (openai_settings, openai_setting_names),
                (textgen_settings, textgen_setting_names),
            ),
            world_names,
            (
                themes,
                moving_ui_presets,
                quick_reply_presets,
                instruct_presets,
                context_presets,
                sysprompt_presets,
                reasoning_presets,
            ),
        ) = tokio::try_join!(settings_json, ai_settings, world_names, presets)?;

        let themes_json = Self::settings_values(themes);
        let moving_ui_presets_json = Self::settings_values(moving_ui_presets);
        let quick_reply_presets_json = Self::settings_values(quick_reply_presets);
        let instruct_presets_json = Self::settings_values(instruct_presets);
        let context_presets_json = Self::settings_values(context_presets);
        let sysprompt_presets_json = Self::settings_values(sysprompt_presets);
        let reasoning_presets_json = Self::settings_values(reasoning_presets);

        let response = SillyTavernSettingsResponseDto {
            settings: settings_json,
            tauritavern_settings_revision,
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

    fn settings_values(settings: Vec<UserSettings>) -> Vec<Value> {
        settings.into_iter().map(|settings| settings.data).collect()
    }

    pub async fn create_snapshot(&self) -> Result<(), ApplicationError> {
        tracing::info!("Creating settings snapshot");

        let mut settings = self.settings_repository.load_user_settings().await?;
        insert_personas(&mut settings.data, &self.avatars.get_personas().await?);
        self.settings_repository.create_snapshot(settings).await?;

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

        let _guard = self.user_settings_save_lock.lock().await;
        let mut settings = self.settings_repository.load_snapshot(name).await?;
        if let Some(personas) = take_personas(&mut settings.data)? {
            self.avatars.import_personas(&personas).await?;
        }
        self.settings_repository
            .save_user_settings(settings)
            .await?;
        self.clear_sillytavern_settings_cache().await;

        Ok(())
    }
}

fn validate_agent_retention_settings(
    settings: &AgentRunRetentionSettings,
) -> Result<(), ApplicationError> {
    settings
        .validate()
        .map_err(|error| ApplicationError::ValidationError(error.message()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::settings_dto::UpdateAgentRunRetentionSettingsDto;
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Mutex;
    use tt_domain::errors::DomainError;
    use tt_domain::models::settings::{SettingsSnapshot, TauriTavernSettings, UserSettings};

    #[test]
    fn agent_retention_update_applies_partial_settings() {
        let mut settings = AgentSettings::default();

        SettingsService::apply_agent_settings_update(
            &mut settings,
            UpdateAgentSettingsDto {
                retention: Some(UpdateAgentRunRetentionSettingsDto {
                    auto_prune_enabled: None,
                    keep_recent_terminal_runs: Some(50),
                    keep_full_recent_runs: Some(10),
                }),
            },
        )
        .expect("apply agent settings");

        assert_eq!(settings.retention.keep_recent_terminal_runs, 50);
        assert_eq!(settings.retention.keep_full_recent_runs, 10);
        assert!(!settings.retention.auto_prune_enabled);
    }

    #[test]
    fn agent_retention_update_rejects_full_retention_outside_history_window() {
        let mut settings = AgentSettings::default();

        let error = SettingsService::apply_agent_settings_update(
            &mut settings,
            UpdateAgentSettingsDto {
                retention: Some(UpdateAgentRunRetentionSettingsDto {
                    auto_prune_enabled: None,
                    keep_recent_terminal_runs: Some(10),
                    keep_full_recent_runs: Some(11),
                }),
            },
        )
        .expect_err("reject invalid retention");

        assert!(matches!(
            error,
            ApplicationError::ValidationError(message)
                if message.contains("agent.retention_keep_full_recent_runs_invalid")
        ));
    }

    #[test]
    fn chat_backup_update_applies_partial_settings_and_validates_sentinels() {
        let mut settings = ChatBackupSettings::default();
        SettingsService::apply_chat_backup_settings_update(
            &mut settings,
            UpdateChatBackupSettingsDto {
                automatic_enabled: Some(false),
                zstd_compression_enabled: Some(true),
                max_files_per_prefix: Some(-1),
                max_total_files: Some(0),
                max_total_bytes: None,
            },
        )
        .expect("apply chat backup settings");

        assert!(!settings.automatic_enabled);
        assert!(settings.zstd_compression_enabled);
        assert_eq!(settings.max_files_per_prefix, -1);
        assert_eq!(settings.max_total_files, 0);

        let error = SettingsService::apply_chat_backup_settings_update(
            &mut settings,
            UpdateChatBackupSettingsDto {
                automatic_enabled: None,
                zstd_compression_enabled: None,
                max_files_per_prefix: None,
                max_total_files: None,
                max_total_bytes: Some(-2),
            },
        )
        .expect_err("reject invalid sentinel");
        assert!(matches!(error, ApplicationError::ValidationError(_)));
    }

    #[tokio::test]
    async fn tauritavern_settings_update_persists_and_applies_chat_backup_policy() {
        let repository = Arc::new(TestSettingsRepository::default());
        let backup_runtime = Arc::new(TestChatBackupRuntime::default());
        let service = SettingsService::new(
            Arc::new(TestAvatars),
            repository,
            Arc::new(TestRequestProxyRuntime::default()),
            backup_runtime.clone(),
        );

        let updated = service
            .update_tauritavern_settings(UpdateTauriTavernSettingsDto {
                updates: None,
                perf_profile: None,
                panel_runtime_profile: None,
                embedded_runtime_profile: None,
                chat_virtualization_enabled: None,
                cold_swipes_enabled: None,
                codemirror_editor_enabled: None,
                chat_backups: Some(UpdateChatBackupSettingsDto {
                    automatic_enabled: Some(false),
                    zstd_compression_enabled: Some(true),
                    max_files_per_prefix: Some(7),
                    max_total_files: Some(-1),
                    max_total_bytes: Some(0),
                }),
                close_to_tray_on_close: None,
                request_proxy: None,
                allow_keys_exposure: None,
                avatar_persona_original_images_enabled: None,
                dev: None,
                dynamic_theme: None,
                models: None,
                agent: None,
            })
            .await
            .expect("update chat backup policy");

        assert!(!updated.chat_backups.automatic_enabled);
        assert!(updated.chat_backups.zstd_compression_enabled);
        assert_eq!(updated.chat_backups.max_files_per_prefix, 7);
        assert_eq!(updated.chat_backups.max_total_files, -1);
        assert_eq!(updated.chat_backups.max_total_bytes, 0);
        assert_eq!(
            backup_runtime.applied.lock().unwrap().as_slice(),
            &[ChatBackupSettings {
                automatic_enabled: false,
                zstd_compression_enabled: true,
                max_files_per_prefix: 7,
                max_total_files: -1,
                max_total_bytes: 0,
            }]
        );
    }

    #[tokio::test]
    async fn compression_only_update_applies_and_schedules_format_reconciliation() {
        let repository = Arc::new(TestSettingsRepository::default());
        let backup_runtime = Arc::new(TestChatBackupRuntime::default());
        let service = SettingsService::new(
            Arc::new(TestAvatars),
            repository,
            Arc::new(TestRequestProxyRuntime::default()),
            backup_runtime.clone(),
        );

        let update = UpdateTauriTavernSettingsDto {
            chat_backups: Some(UpdateChatBackupSettingsDto {
                automatic_enabled: None,
                zstd_compression_enabled: Some(true),
                max_files_per_prefix: None,
                max_total_files: None,
                max_total_bytes: None,
            }),
            updates: None,
            perf_profile: None,
            panel_runtime_profile: None,
            embedded_runtime_profile: None,
            chat_virtualization_enabled: None,
            cold_swipes_enabled: None,
            codemirror_editor_enabled: None,
            close_to_tray_on_close: None,
            request_proxy: None,
            allow_keys_exposure: None,
            avatar_persona_original_images_enabled: None,
            dev: None,
            dynamic_theme: None,
            models: None,
            agent: None,
        };
        service
            .update_tauritavern_settings(update.clone())
            .await
            .expect("enable backup compression");

        assert!(backup_runtime.applied.lock().unwrap()[0].zstd_compression_enabled);
        tokio::time::timeout(Duration::from_millis(100), async {
            while backup_runtime.reconciliation_calls.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("background format reconciliation was scheduled");
        assert_eq!(
            backup_runtime.reconciliation_calls.load(Ordering::Acquire),
            1
        );

        service
            .update_tauritavern_settings(update)
            .await
            .expect("repeat unchanged backup compression setting");
        tokio::task::yield_now().await;
        assert_eq!(backup_runtime.applied.lock().unwrap().len(), 1);
        assert_eq!(
            backup_runtime.reconciliation_calls.load(Ordering::Acquire),
            1
        );
    }

    #[tokio::test]
    async fn chat_backup_cleanup_failure_does_not_fail_settings_update() {
        let repository = Arc::new(TestSettingsRepository::default());
        let backup_runtime = Arc::new(TestChatBackupRuntime {
            fail_reconciliation: AtomicBool::new(true),
            ..Default::default()
        });
        let service = SettingsService::new(
            Arc::new(TestAvatars),
            repository,
            Arc::new(TestRequestProxyRuntime::default()),
            backup_runtime.clone(),
        );

        service
            .update_tauritavern_settings(UpdateTauriTavernSettingsDto {
                chat_backups: Some(UpdateChatBackupSettingsDto {
                    automatic_enabled: Some(false),
                    zstd_compression_enabled: None,
                    max_files_per_prefix: Some(7),
                    max_total_files: None,
                    max_total_bytes: None,
                }),
                updates: None,
                perf_profile: None,
                panel_runtime_profile: None,
                embedded_runtime_profile: None,
                chat_virtualization_enabled: None,
                cold_swipes_enabled: None,
                codemirror_editor_enabled: None,
                close_to_tray_on_close: None,
                request_proxy: None,
                allow_keys_exposure: None,
                avatar_persona_original_images_enabled: None,
                dev: None,
                dynamic_theme: None,
                models: None,
                agent: None,
            })
            .await
            .expect("settings update must not await cleanup");

        tokio::time::timeout(Duration::from_millis(100), async {
            while backup_runtime.reconciliation_calls.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("background cleanup was scheduled");
    }

    struct TestAvatars;

    #[async_trait::async_trait]
    impl AvatarRepository for TestAvatars {
        async fn get_personas(&self) -> Result<Personas, DomainError> {
            unreachable!()
        }
        async fn save_persona(
            &self,
            _: &str,
            _: &tt_domain::models::persona::Persona,
        ) -> Result<(), DomainError> {
            unreachable!()
        }
        async fn import_personas(&self, _: &Personas) -> Result<(), DomainError> {
            unreachable!()
        }
        async fn get_avatars(&self) -> Result<Vec<tt_domain::models::avatar::Avatar>, DomainError> {
            unreachable!()
        }
        async fn delete_avatar(&self, _: &str) -> Result<(), DomainError> {
            unreachable!()
        }
        async fn upload_avatar(
            &self,
            _: &std::path::Path,
            _: Option<String>,
            _: Option<tt_domain::models::avatar::CropInfo>,
        ) -> Result<tt_domain::models::avatar::AvatarUploadResult, DomainError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct TestSettingsRepository {
        settings: Mutex<TauriTavernSettings>,
    }

    #[async_trait]
    impl SettingsRepository for TestSettingsRepository {
        async fn save_tauritavern_settings(
            &self,
            settings: &TauriTavernSettings,
        ) -> Result<(), DomainError> {
            *self.settings.lock().await = settings.clone();
            Ok(())
        }

        async fn load_tauritavern_settings(&self) -> Result<TauriTavernSettings, DomainError> {
            Ok(self.settings.lock().await.clone())
        }

        async fn save_user_settings(&self, _settings: UserSettings) -> Result<(), DomainError> {
            unreachable!()
        }

        async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
            unreachable!()
        }

        async fn begin_commit(
            &self,
            _: Option<tt_domain::models::settings::revision::UserSettingsRevision>,
        ) -> Result<tt_contracts::byte_commit::CommitBegin, DomainError> {
            unreachable!()
        }
        async fn append_commit(&self, _: &str, _: u64, _: &[u8]) -> Result<u64, DomainError> {
            unreachable!()
        }
        async fn finish_commit(
            &self,
            _: &str,
            _: u64,
        ) -> Result<tt_ports::repositories::settings_repository::SettingsCommitResult, DomainError>
        {
            unreachable!()
        }
        async fn abort_commit(&self, _: &str) -> Result<(), DomainError> {
            unreachable!()
        }

        async fn create_snapshot(&self, _settings: UserSettings) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn load_snapshot(&self, _name: &str) -> Result<UserSettings, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_sillytavern_settings_signature(
            &self,
        ) -> Result<SettingsAggregateSignature, DomainError> {
            unreachable!()
        }

        async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            Ok(Vec::new())
        }

        async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn get_world_names(&self) -> Result<Vec<String>, DomainError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct TestRequestProxyRuntime {
        applied: StdMutex<Vec<String>>,
    }

    impl RequestProxyRuntime for TestRequestProxyRuntime {
        fn validate_request_proxy_settings(
            &self,
            _settings: &RequestProxySettings,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn apply_request_proxy_settings(
            &self,
            settings: &RequestProxySettings,
        ) -> Result<(), DomainError> {
            self.applied.lock().unwrap().push(settings.url.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestChatBackupRuntime {
        applied: StdMutex<Vec<ChatBackupSettings>>,
        reconciliation_calls: AtomicUsize,
        fail_reconciliation: AtomicBool,
        fail_stats: AtomicBool,
    }

    #[async_trait]
    impl ChatBackupRuntime for TestChatBackupRuntime {
        async fn apply_chat_backup_settings(
            &self,
            settings: ChatBackupSettings,
        ) -> Result<(), DomainError> {
            self.applied.lock().unwrap().push(settings);
            Ok(())
        }

        async fn reconcile_chat_backups(&self) -> Result<(), DomainError> {
            self.reconciliation_calls.fetch_add(1, Ordering::AcqRel);
            if self.fail_reconciliation.load(Ordering::Acquire) {
                Err(DomainError::InternalError(
                    "simulated cleanup failure".into(),
                ))
            } else {
                Ok(())
            }
        }

        async fn get_chat_backup_storage_stats(
            &self,
        ) -> Result<Option<ChatBackupStorageStats>, DomainError> {
            if self.fail_stats.load(Ordering::Acquire) {
                Err(DomainError::InvalidData(
                    "simulated backup stats failure".into(),
                ))
            } else {
                Ok(None)
            }
        }
    }
}
