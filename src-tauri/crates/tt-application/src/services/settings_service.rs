use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use super::settings_repair::repair_sillytavern_prompt_manager_settings;
use crate::dto::settings_dto::{
    SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateAgentSettingsDto, UpdateTauriTavernSettingsDto, UserSettingsDto,
};
use crate::errors::ApplicationError;
use tt_domain::models::settings::{
    AgentRunRetentionSettings, AgentSettings, DevLoggingSettings, RequestProxySettings,
};
use tt_ports::repositories::settings_repository::SettingsRepository;
pub use tt_ports::settings::RequestProxyRuntime;

pub struct SettingsService {
    settings_repository: Arc<dyn SettingsRepository>,
    request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
    pending_user_settings_repair_writeback: Arc<AtomicBool>,
}

impl SettingsService {
    pub fn new(
        settings_repository: Arc<dyn SettingsRepository>,
        request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
    ) -> Self {
        Self {
            settings_repository,
            request_proxy_runtime,
            pending_user_settings_repair_writeback: Arc::new(AtomicBool::new(false)),
        }
    }

    fn schedule_delayed_user_settings_repair_writeback(&self) {
        const DELAY: Duration = Duration::from_secs(20);

        if self
            .pending_user_settings_repair_writeback
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let settings_repository = Arc::clone(&self.settings_repository);
        let pending = Arc::clone(&self.pending_user_settings_repair_writeback);

        tokio::spawn(async move {
            tokio::time::sleep(DELAY).await;

            let result = async {
                let mut settings = settings_repository.load_user_settings().await?;
                let repair_report = repair_sillytavern_prompt_manager_settings(&mut settings);

                if !repair_report.changed() {
                    return Ok(());
                }

                tracing::warn!(
                    "Persisting delayed SillyTavern PromptManager settings repair: {}",
                    repair_report
                );
                settings_repository.save_user_settings(&settings).await
            }
            .await;

            if let Err(error) = result {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Failed delayed SillyTavern PromptManager settings repair: {}",
                    error
                );
            }

            pending.store(false, Ordering::Release);
        });
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

        let request_proxy_update = dto.request_proxy.clone().map(RequestProxySettings::from);
        if let Some(settings) = request_proxy_update.as_ref() {
            self.request_proxy_runtime
                .validate_request_proxy_settings(settings)?;
        }

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

        if let Some(embedded_runtime_profile) = dto.embedded_runtime_profile {
            settings.embedded_runtime_profile = embedded_runtime_profile;
        }

        if let Some(chat_history_mode) = dto.chat_history_mode {
            settings.chat_history_mode = chat_history_mode;
        }

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

        if let Some(native_regex_backend_enabled) = dto.native_regex_backend_enabled {
            settings.native_regex_backend_enabled = native_regex_backend_enabled;
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

        if let Some(models) = dto.models {
            if let Some(claude) = models.claude {
                if let Some(prompt_cache_ttl) = claude.prompt_cache_ttl {
                    settings.models.claude.prompt_cache_ttl = prompt_cache_ttl;
                }
            }
        }

        if let Some(agent) = dto.agent {
            Self::apply_agent_settings_update(&mut settings.agent, agent)?;
        }

        self.settings_repository
            .save_tauritavern_settings(&settings)
            .await?;

        if request_proxy_update.is_some() {
            self.request_proxy_runtime
                .apply_request_proxy_settings(&settings.request_proxy)?;
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

    pub async fn save_user_settings(
        &self,
        settings: UserSettingsDto,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Saving user settings");

        let mut user_settings = settings.into();
        let repair_report = repair_sillytavern_prompt_manager_settings(&mut user_settings);
        if repair_report.changed() {
            tracing::warn!(
                "Repaired SillyTavern PromptManager settings before save: {}",
                repair_report
            );
        }

        self.settings_repository
            .save_user_settings(&user_settings)
            .await?;

        Ok(())
    }

    pub async fn get_sillytavern_settings(
        &self,
    ) -> Result<SillyTavernSettingsResponseDto, ApplicationError> {
        tracing::info!("Getting SillyTavern settings");

        let mut user_settings = self.settings_repository.load_user_settings().await?;
        let repair_report = repair_sillytavern_prompt_manager_settings(&mut user_settings);
        if repair_report.changed() {
            tracing::warn!(
                "Repaired SillyTavern PromptManager settings while loading: {}",
                repair_report
            );
            self.schedule_delayed_user_settings_repair_writeback();
        }

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
    use crate::dto::settings_dto::{RequestProxySettingsDto, UpdateAgentRunRetentionSettingsDto};
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;
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
    fn agent_retention_update_applies_auto_prune_flag() {
        let mut settings = AgentSettings::default();

        SettingsService::apply_agent_settings_update(
            &mut settings,
            UpdateAgentSettingsDto {
                retention: Some(UpdateAgentRunRetentionSettingsDto {
                    auto_prune_enabled: Some(true),
                    keep_recent_terminal_runs: None,
                    keep_full_recent_runs: None,
                }),
            },
        )
        .expect("apply agent settings");

        assert!(settings.retention.auto_prune_enabled);
    }

    #[test]
    fn agent_retention_update_allows_zero_terminal_history() {
        let mut settings = AgentSettings::default();

        SettingsService::apply_agent_settings_update(
            &mut settings,
            UpdateAgentSettingsDto {
                retention: Some(UpdateAgentRunRetentionSettingsDto {
                    auto_prune_enabled: None,
                    keep_recent_terminal_runs: Some(0),
                    keep_full_recent_runs: Some(0),
                }),
            },
        )
        .expect("apply zero retention");

        assert_eq!(settings.retention.keep_recent_terminal_runs, 0);
        assert_eq!(settings.retention.keep_full_recent_runs, 0);
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

    #[tokio::test]
    async fn tauritavern_settings_update_applies_request_proxy_runtime() {
        let repository = Arc::new(TestSettingsRepository::default());
        let runtime = Arc::new(TestRequestProxyRuntime::default());
        let service = SettingsService::new(repository, runtime.clone());

        let updated = service
            .update_tauritavern_settings(UpdateTauriTavernSettingsDto {
                request_proxy: Some(RequestProxySettingsDto {
                    enabled: true,
                    url: "http://127.0.0.1:8080".to_string(),
                    bypass: vec!["localhost".to_string()],
                }),
                updates: None,
                perf_profile: None,
                panel_runtime_profile: None,
                embedded_runtime_profile: None,
                chat_history_mode: None,
                close_to_tray_on_close: None,
                allow_keys_exposure: None,
                avatar_persona_original_images_enabled: None,
                native_regex_backend_enabled: None,
                dev: None,
                dynamic_theme: None,
                models: None,
                agent: None,
            })
            .await
            .expect("update settings");

        assert!(updated.request_proxy.enabled);
        assert_eq!(
            runtime.applied.lock().unwrap().as_slice(),
            ["http://127.0.0.1:8080"]
        );
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

        async fn save_user_settings(&self, _settings: &UserSettings) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn create_snapshot(&self) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn load_snapshot(&self, _name: &str) -> Result<UserSettings, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn restore_snapshot(&self, _name: &str) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_world_names(&self) -> Result<Vec<String>, DomainError> {
            unimplemented!("not used by these tests")
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
}
