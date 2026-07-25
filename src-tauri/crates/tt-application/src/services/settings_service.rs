use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Mutex;

use super::settings_repair::repair_sillytavern_prompt_manager_settings;
use crate::dto::settings_dto::{
    SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateAgentSettingsDto, UpdateChatBackupSettingsDto, UpdateTauriTavernSettingsDto,
    UserSettingsDto, UserSettingsPatchDto, UserSettingsPatchOpDto, UserSettingsRevisionDto,
    UserSettingsSaveResultDto,
};
use crate::errors::ApplicationError;
use crate::services::hashing::hex_lower;
use tt_domain::models::settings::{
    AgentRunRetentionSettings, AgentSettings, ChatBackupSettings, DevLoggingSettings,
    RequestProxySettings, UserSettings,
};
use tt_ports::repositories::settings_repository::{
    SettingsAggregateSignature, SettingsRepository, UserSettingsRevision,
};
pub use tt_ports::settings::{ChatBackupRuntime, ChatBackupStorageStats, RequestProxyRuntime};

pub const USER_SETTINGS_HASH_ALGORITHM: &str = "tt-user-settings-stable-sha256-v1";

#[derive(Clone)]
struct SettingsAggregateCacheEntry {
    signature: SettingsAggregateSignature,
    response: SillyTavernSettingsResponseDto,
}

pub struct SettingsService {
    settings_repository: Arc<dyn SettingsRepository>,
    request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
    chat_backup_runtime: Arc<dyn ChatBackupRuntime>,
    pending_user_settings_repair_writeback: Arc<AtomicBool>,
    sillytavern_settings_cache: Arc<Mutex<Option<SettingsAggregateCacheEntry>>>,
    user_settings_save_lock: Arc<Mutex<()>>,
}

impl SettingsService {
    pub fn new(
        settings_repository: Arc<dyn SettingsRepository>,
        request_proxy_runtime: Arc<dyn RequestProxyRuntime>,
        chat_backup_runtime: Arc<dyn ChatBackupRuntime>,
    ) -> Self {
        Self {
            settings_repository,
            request_proxy_runtime,
            chat_backup_runtime,
            pending_user_settings_repair_writeback: Arc::new(AtomicBool::new(false)),
            sillytavern_settings_cache: Arc::new(Mutex::new(None)),
            user_settings_save_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn clear_sillytavern_settings_cache(&self) {
        *self.sillytavern_settings_cache.lock().await = None;
    }

    pub async fn clear_cache(&self) {
        self.clear_sillytavern_settings_cache().await;
    }

    pub async fn reload_chat_backup_settings(&self) -> Result<(), tt_domain::errors::DomainError> {
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
        let settings_cache = Arc::clone(&self.sillytavern_settings_cache);
        let save_lock = Arc::clone(&self.user_settings_save_lock);

        tokio::spawn(async move {
            tokio::time::sleep(DELAY).await;

            let result: Result<bool, tt_domain::errors::DomainError> = async {
                let _guard = save_lock.lock().await;
                let mut settings = settings_repository.load_user_settings().await?;
                let repair_report = repair_sillytavern_prompt_manager_settings(&mut settings);

                if !repair_report.changed() {
                    return Ok(false);
                }

                tracing::warn!(
                    "Persisting delayed SillyTavern PromptManager settings repair: {}",
                    repair_report
                );
                settings_repository.save_user_settings(&settings).await?;
                let settings_hash =
                    Self::stable_user_settings_hash(&settings.data).map_err(|error| {
                        tt_domain::errors::DomainError::InternalError(error.to_string())
                    })?;
                let revision = UserSettingsRevision {
                    hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                    settings_hash,
                };
                if let Err(error) = settings_repository
                    .save_user_settings_revision(&revision)
                    .await
                {
                    tracing::warn!(
                        "Failed to refresh user settings revision after delayed repair: {}",
                        error
                    );
                }
                Ok(true)
            }
            .await;

            match result {
                Ok(true) => {
                    *settings_cache.lock().await = None;
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed delayed SillyTavern PromptManager settings repair: {}",
                        error
                    );
                }
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

        if let Some(chat_virtualization_enabled) = dto.chat_virtualization_enabled {
            settings.chat_virtualization_enabled = chat_virtualization_enabled;
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

    pub async fn save_user_settings(
        &self,
        settings: UserSettingsDto,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Saving user settings");

        let mut user_settings = settings.into();
        Self::repair_user_settings_before_save(&mut user_settings, "before save");
        let next_hash = Self::stable_user_settings_hash(&user_settings.data)?;

        let _guard = self.user_settings_save_lock.lock().await;
        if self.cached_user_settings_hash_matches(&next_hash).await? {
            tracing::debug!("Skipping unchanged user settings save from revision cache");
            return Ok(());
        }

        let current_settings = self.settings_repository.load_user_settings().await?;
        let current_hash = Self::stable_user_settings_hash(&current_settings.data)?;
        if current_hash == next_hash {
            tracing::debug!("Skipping unchanged user settings save");
            self.refresh_user_settings_revision(&next_hash).await;
            return Ok(());
        }

        self.persist_user_settings(&user_settings, &next_hash)
            .await?;

        Ok(())
    }

    pub async fn save_user_settings_patch(
        &self,
        patch: UserSettingsPatchDto,
    ) -> Result<UserSettingsSaveResultDto, ApplicationError> {
        tracing::info!("Saving user settings patch");

        Self::validate_user_settings_patch(&patch)?;

        let _guard = self.user_settings_save_lock.lock().await;
        let mut current_settings = self.settings_repository.load_user_settings().await?;
        let current_repaired =
            Self::repair_user_settings_before_save(&mut current_settings, "before patch save");

        let current_hash = Self::stable_user_settings_hash(&current_settings.data)?;
        if current_hash != patch.base_hash {
            return Err(ApplicationError::Conflict(format!(
                "Settings changed since patch baseline: expected {}, current {}",
                patch.base_hash, current_hash
            )));
        }

        let mut patched_settings = current_settings;
        Self::apply_user_settings_patch(&mut patched_settings.data, &patch.ops)?;
        let patched_repaired =
            Self::repair_user_settings_before_save(&mut patched_settings, "after patch save");
        let patched_hash = Self::stable_user_settings_hash(&patched_settings.data)?;

        if patched_hash == current_hash && !current_repaired && !patched_repaired {
            tracing::debug!("Skipping unchanged user settings patch save");
            self.refresh_user_settings_revision(&patched_hash).await;
            return Ok(Self::user_settings_save_result("patch-noop", patched_hash));
        }

        let mode = if patched_hash == current_hash {
            "patch-noop"
        } else {
            "patch"
        };
        self.persist_user_settings(&patched_settings, &patched_hash)
            .await?;

        Ok(Self::user_settings_save_result(mode, patched_hash))
    }

    async fn persist_user_settings(
        &self,
        user_settings: &UserSettings,
        settings_hash: &str,
    ) -> Result<(), ApplicationError> {
        self.settings_repository
            .save_user_settings(user_settings)
            .await?;
        self.refresh_user_settings_revision(settings_hash).await;
        self.clear_sillytavern_settings_cache().await;

        Ok(())
    }

    async fn cached_user_settings_hash_matches(
        &self,
        settings_hash: &str,
    ) -> Result<bool, ApplicationError> {
        let Some(revision) = self
            .settings_repository
            .load_user_settings_revision()
            .await?
        else {
            return Ok(false);
        };

        Ok(revision.hash_algorithm == USER_SETTINGS_HASH_ALGORITHM
            && revision.settings_hash == settings_hash)
    }

    async fn refresh_user_settings_revision(&self, settings_hash: &str) {
        let revision = UserSettingsRevision {
            hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
            settings_hash: settings_hash.to_string(),
        };

        if let Err(error) = self
            .settings_repository
            .save_user_settings_revision(&revision)
            .await
        {
            tracing::warn!("Failed to refresh user settings revision: {}", error);
        }
    }

    fn repair_user_settings_before_save(settings: &mut UserSettings, context: &str) -> bool {
        let repair_report = repair_sillytavern_prompt_manager_settings(settings);
        let changed = repair_report.changed();
        if changed {
            tracing::warn!(
                "Repaired SillyTavern PromptManager settings {}: {}",
                context,
                repair_report
            );
        }
        changed
    }

    fn user_settings_save_result(
        mode: impl Into<String>,
        settings_hash: String,
    ) -> UserSettingsSaveResultDto {
        UserSettingsSaveResultDto {
            result: "ok".to_string(),
            mode: mode.into(),
            hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
            settings_hash,
        }
    }

    fn validate_user_settings_patch(patch: &UserSettingsPatchDto) -> Result<(), ApplicationError> {
        if patch.hash_algorithm != USER_SETTINGS_HASH_ALGORITHM {
            return Err(ApplicationError::ValidationError(format!(
                "Unsupported settings hash algorithm: {}",
                patch.hash_algorithm
            )));
        }

        Self::validate_user_settings_hash("base_hash", &patch.base_hash)?;

        Ok(())
    }

    fn validate_user_settings_hash(label: &str, value: &str) -> Result<(), ApplicationError> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Ok(());
        }

        Err(ApplicationError::ValidationError(format!(
            "Invalid settings {label}"
        )))
    }

    fn apply_user_settings_patch(
        settings: &mut Value,
        ops: &[UserSettingsPatchOpDto],
    ) -> Result<(), ApplicationError> {
        for op in ops {
            match op {
                UserSettingsPatchOpDto::Set { path, value } => {
                    Self::apply_user_settings_patch_set(settings, path, value.clone())?;
                }
                UserSettingsPatchOpDto::Delete { path } => {
                    Self::apply_user_settings_patch_delete(settings, path)?;
                }
            }
        }

        Ok(())
    }

    fn apply_user_settings_patch_set(
        settings: &mut Value,
        path: &[String],
        value: Value,
    ) -> Result<(), ApplicationError> {
        if path.is_empty() {
            *settings = value;
            return Ok(());
        }

        let mut cursor = settings;
        for segment in &path[..path.len() - 1] {
            cursor = cursor
                .as_object_mut()
                .and_then(|object| object.get_mut(segment))
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Invalid settings patch path: {}",
                        Self::format_patch_path(path)
                    ))
                })?;
        }

        let parent = cursor.as_object_mut().ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Invalid settings patch parent: {}",
                Self::format_patch_path(path)
            ))
        })?;
        let last = path.last().expect("non-empty path has last segment");
        parent.insert(last.clone(), value);

        Ok(())
    }

    fn apply_user_settings_patch_delete(
        settings: &mut Value,
        path: &[String],
    ) -> Result<(), ApplicationError> {
        if path.is_empty() {
            return Err(ApplicationError::ValidationError(
                "Settings patch cannot delete the root".to_string(),
            ));
        }

        let mut cursor = settings;
        for segment in &path[..path.len() - 1] {
            cursor = cursor
                .as_object_mut()
                .and_then(|object| object.get_mut(segment))
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Invalid settings patch path: {}",
                        Self::format_patch_path(path)
                    ))
                })?;
        }

        let parent = cursor.as_object_mut().ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Invalid settings patch parent: {}",
                Self::format_patch_path(path)
            ))
        })?;
        let last = path.last().expect("non-empty path has last segment");
        if parent.remove(last).is_none() {
            return Err(ApplicationError::ValidationError(format!(
                "Settings patch delete path does not exist: {}",
                Self::format_patch_path(path)
            )));
        }

        Ok(())
    }

    fn format_patch_path(path: &[String]) -> String {
        if path.is_empty() {
            "$".to_string()
        } else {
            format!("$.{}", path.join("."))
        }
    }

    fn stable_user_settings_hash(value: &Value) -> Result<String, ApplicationError> {
        let mut canonical = Vec::new();
        Self::write_canonical_json(value, &mut canonical)?;
        let digest = Sha256::digest(&canonical);
        Ok(hex_lower(&digest))
    }

    fn write_canonical_json<W: Write>(
        value: &Value,
        writer: &mut W,
    ) -> Result<(), ApplicationError> {
        match value {
            Value::Array(items) => {
                writer.write_all(b"[").map_err(Self::canonical_json_error)?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        writer.write_all(b",").map_err(Self::canonical_json_error)?;
                    }
                    Self::write_canonical_json(item, writer)?;
                }
                writer.write_all(b"]").map_err(Self::canonical_json_error)?;
            }
            Value::Object(object) => {
                writer.write_all(b"{").map_err(Self::canonical_json_error)?;
                let mut entries = object.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| *key);
                for (index, (key, nested)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        writer.write_all(b",").map_err(Self::canonical_json_error)?;
                    }
                    serde_json::to_writer(&mut *writer, key)
                        .map_err(Self::canonical_json_serialize_error)?;
                    writer.write_all(b":").map_err(Self::canonical_json_error)?;
                    Self::write_canonical_json(nested, writer)?;
                }
                writer.write_all(b"}").map_err(Self::canonical_json_error)?;
            }
            _ => {
                serde_json::to_writer(writer, value)
                    .map_err(Self::canonical_json_serialize_error)?;
            }
        }

        Ok(())
    }

    fn canonical_json_error(error: std::io::Error) -> ApplicationError {
        ApplicationError::InternalError(format!("Failed to canonicalize settings: {}", error))
    }

    fn canonical_json_serialize_error(error: serde_json::Error) -> ApplicationError {
        ApplicationError::InternalError(format!("Failed to serialize settings: {}", error))
    }

    pub async fn get_sillytavern_settings(
        &self,
    ) -> Result<SillyTavernSettingsResponseDto, ApplicationError> {
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

        let response = self.build_sillytavern_settings_response().await?;
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
            let repair_report = repair_sillytavern_prompt_manager_settings(&mut user_settings);
            let repaired = repair_report.changed();
            if repaired {
                tracing::warn!(
                    "Repaired SillyTavern PromptManager settings while loading: {}",
                    repair_report
                );
                self.schedule_delayed_user_settings_repair_writeback();
            }

            let cached_revision = if repaired {
                None
            } else {
                self.settings_repository
                    .load_user_settings_revision()
                    .await?
            };
            let revision = if let Some(revision) = cached_revision
                && revision.hash_algorithm == USER_SETTINGS_HASH_ALGORITHM
                && Self::validate_user_settings_hash("settings_hash", &revision.settings_hash)
                    .is_ok()
            {
                UserSettingsRevisionDto {
                    hash_algorithm: revision.hash_algorithm,
                    settings_hash: revision.settings_hash,
                }
            } else {
                UserSettingsRevisionDto {
                    hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                    settings_hash: Self::stable_user_settings_hash(&user_settings.data)?,
                }
            };
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

        let _guard = self.user_settings_save_lock.lock().await;
        self.settings_repository.restore_snapshot(name).await?;
        let settings = self.settings_repository.load_user_settings().await?;
        let settings_hash = Self::stable_user_settings_hash(&settings.data)?;
        self.refresh_user_settings_revision(&settings_hash).await;
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
    use crate::dto::settings_dto::{RequestProxySettingsDto, UpdateAgentRunRetentionSettingsDto};
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::AtomicUsize;
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
    async fn tauritavern_settings_update_applies_request_proxy_runtime() {
        let repository = Arc::new(TestSettingsRepository::default());
        let runtime = Arc::new(TestRequestProxyRuntime::default());
        let service = SettingsService::new(
            repository,
            runtime.clone(),
            Arc::new(TestChatBackupRuntime::default()),
        );

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
                chat_virtualization_enabled: None,
                chat_backups: None,
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

    #[tokio::test]
    async fn tauritavern_settings_update_persists_chat_virtualization_switch() {
        let service = SettingsService::new(
            Arc::new(TestSettingsRepository::default()),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let updated = service
            .update_tauritavern_settings(UpdateTauriTavernSettingsDto {
                updates: None,
                perf_profile: None,
                panel_runtime_profile: None,
                embedded_runtime_profile: None,
                chat_virtualization_enabled: Some(true),
                chat_backups: None,
                close_to_tray_on_close: None,
                request_proxy: None,
                allow_keys_exposure: None,
                avatar_persona_original_images_enabled: None,
                native_regex_backend_enabled: None,
                dev: None,
                dynamic_theme: None,
                models: None,
                agent: None,
            })
            .await
            .expect("enable chat virtualization");

        assert!(updated.chat_virtualization_enabled);
    }

    #[tokio::test]
    async fn unavailable_chat_backup_storage_stats_do_not_block_settings() {
        let backup_runtime = Arc::new(TestChatBackupRuntime::default());
        backup_runtime.fail_stats.store(true, Ordering::Release);
        let service = SettingsService::new(
            Arc::new(TestSettingsRepository::default()),
            Arc::new(TestRequestProxyRuntime::default()),
            backup_runtime,
        );

        assert_eq!(
            service
                .get_chat_backup_storage_stats()
                .await
                .expect("optional backup statistics should stay non-fatal"),
            None
        );
    }

    #[tokio::test]
    async fn tauritavern_settings_update_persists_and_applies_chat_backup_policy() {
        let repository = Arc::new(TestSettingsRepository::default());
        let backup_runtime = Arc::new(TestChatBackupRuntime::default());
        let service = SettingsService::new(
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
                native_regex_backend_enabled: None,
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
            close_to_tray_on_close: None,
            request_proxy: None,
            allow_keys_exposure: None,
            avatar_persona_original_images_enabled: None,
            native_regex_backend_enabled: None,
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
                close_to_tray_on_close: None,
                request_proxy: None,
                allow_keys_exposure: None,
                avatar_persona_original_images_enabled: None,
                native_regex_backend_enabled: None,
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

    #[tokio::test]
    async fn sillytavern_settings_aggregate_uses_cache_until_signature_changes() {
        let repository = Arc::new(TestSettingsRepository::default());
        repository
            .store_user_settings(json!({"username": "one"}))
            .await;
        repository.store_signature(test_signature("one")).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let first = service
            .get_sillytavern_settings()
            .await
            .expect("load settings aggregate");
        let second = service
            .get_sillytavern_settings()
            .await
            .expect("load cached settings aggregate");

        assert_eq!(repository.load_user_settings_count().await, 1);
        assert_eq!(settings_value(&first), json!({"username": "one"}));
        assert_eq!(
            first.tauritavern_settings_revision.hash_algorithm,
            USER_SETTINGS_HASH_ALGORITHM
        );
        assert_eq!(
            first.tauritavern_settings_revision.settings_hash,
            SettingsService::stable_user_settings_hash(&json!({"username": "one"}))
                .expect("hash settings")
        );
        assert_eq!(settings_value(&second), json!({"username": "one"}));

        repository
            .store_user_settings(json!({"username": "two"}))
            .await;
        repository.store_signature(test_signature("two")).await;

        let third = service
            .get_sillytavern_settings()
            .await
            .expect("reload settings aggregate");

        assert_eq!(repository.load_user_settings_count().await, 2);
        assert_eq!(settings_value(&third), json!({"username": "two"}));
    }

    #[tokio::test]
    async fn clear_cache_drops_settings_aggregate_cache_even_when_signature_is_stable() {
        let repository = Arc::new(TestSettingsRepository::default());
        repository
            .store_user_settings(json!({"username": "one"}))
            .await;
        repository.store_signature(test_signature("stable")).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let first = service
            .get_sillytavern_settings()
            .await
            .expect("prime settings aggregate cache");
        assert_eq!(settings_value(&first), json!({"username": "one"}));

        repository
            .store_user_settings(json!({"username": "two"}))
            .await;

        service.clear_cache().await;

        let second = service
            .get_sillytavern_settings()
            .await
            .expect("reload settings aggregate after explicit clear");

        assert_eq!(repository.load_user_settings_count().await, 2);
        assert_eq!(settings_value(&second), json!({"username": "two"}));
    }

    #[tokio::test]
    async fn save_user_settings_skips_unchanged_payload() {
        let repository = Arc::new(TestSettingsRepository::default());
        repository
            .store_user_settings(json!({"username": "same"}))
            .await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        service
            .save_user_settings(UserSettingsDto {
                data: json!({"username": "same"}),
            })
            .await
            .expect("save unchanged settings");

        assert_eq!(repository.save_user_settings_count().await, 0);
        assert_eq!(repository.load_user_settings_count().await, 1);
    }

    #[test]
    fn user_settings_hash_is_stable_for_object_key_order() {
        let left = json!({
            "b": 1,
            "a": {
                "y": 2,
                "x": [true, null]
            }
        });
        let right = json!({
            "a": {
                "x": [true, null],
                "y": 2
            },
            "b": 1
        });

        let left_hash =
            SettingsService::stable_user_settings_hash(&left).expect("hash left settings");
        let right_hash =
            SettingsService::stable_user_settings_hash(&right).expect("hash right settings");

        assert_eq!(left_hash, right_hash);

        let unicode_key_order = json!({
            "b": 1,
            "a": 2,
            "\u{10000}": 3,
            "\u{e000}": 4
        });
        let unicode_key_order_hash = SettingsService::stable_user_settings_hash(&unicode_key_order)
            .expect("hash unicode key settings");

        assert_eq!(
            unicode_key_order_hash,
            "b80f11d0d2b9f8a24fa66a8d485776a5def012c48b9d3da47d626f42c199569a"
        );
    }

    #[tokio::test]
    async fn save_user_settings_skips_disk_read_when_revision_cache_matches() {
        let repository = Arc::new(TestSettingsRepository::default());
        let data = json!({"username": "same"});
        let settings_hash =
            SettingsService::stable_user_settings_hash(&data).expect("hash settings");
        repository.store_user_settings(data.clone()).await;
        repository.store_user_settings_revision(settings_hash).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        service
            .save_user_settings(UserSettingsDto { data })
            .await
            .expect("save unchanged settings");

        assert_eq!(repository.save_user_settings_count().await, 0);
        assert_eq!(repository.load_user_settings_count().await, 0);
    }

    #[tokio::test]
    async fn save_user_settings_patch_applies_set_and_delete() {
        let repository = Arc::new(TestSettingsRepository::default());
        let current = json!({
            "profile": {
                "age": 1,
                "name": "old"
            },
            "obsolete": true
        });
        let next = json!({
            "profile": {
                "age": 1,
                "name": "new"
            }
        });
        let base_hash =
            SettingsService::stable_user_settings_hash(&current).expect("hash current settings");
        let next_hash =
            SettingsService::stable_user_settings_hash(&next).expect("hash next settings");
        repository.store_user_settings(current).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let result = service
            .save_user_settings_patch(UserSettingsPatchDto {
                hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                base_hash,
                ops: vec![
                    UserSettingsPatchOpDto::Set {
                        path: vec!["profile".to_string(), "name".to_string()],
                        value: json!("new"),
                    },
                    UserSettingsPatchOpDto::Delete {
                        path: vec!["obsolete".to_string()],
                    },
                ],
            })
            .await
            .expect("save settings patch");

        assert_eq!(result.mode, "patch");
        assert_eq!(result.settings_hash, next_hash);
        assert_eq!(repository.save_user_settings_count().await, 1);
        assert_eq!(repository.user_settings_data().await, next);
        assert_eq!(repository.save_user_settings_revision_count().await, 1);
    }

    #[tokio::test]
    async fn save_user_settings_patch_rejects_stale_base_hash() {
        let repository = Arc::new(TestSettingsRepository::default());
        let baseline = json!({"username": "old"});
        let current = json!({"username": "current"});
        let base_hash =
            SettingsService::stable_user_settings_hash(&baseline).expect("hash baseline");
        repository.store_user_settings(current).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let error = service
            .save_user_settings_patch(UserSettingsPatchDto {
                hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                base_hash,
                ops: vec![UserSettingsPatchOpDto::Set {
                    path: vec!["username".to_string()],
                    value: json!("next"),
                }],
            })
            .await
            .expect_err("stale patch should conflict");

        assert!(matches!(error, ApplicationError::Conflict(_)));
        assert_eq!(repository.save_user_settings_count().await, 0);
    }

    #[tokio::test]
    async fn save_user_settings_patch_persists_repaired_noop() {
        let repository = Arc::new(TestSettingsRepository::default());
        let current = json!({
            "oai_settings": {
                "prompts": [
                    { "identifier": "main" },
                    null
                ],
                "prompt_order": [
                    {
                        "character_id": 100001,
                        "order": [
                            null,
                            { "identifier": "main", "enabled": true }
                        ]
                    }
                ]
            }
        });
        let repaired = json!({
            "oai_settings": {
                "prompts": [
                    { "identifier": "main" }
                ],
                "prompt_order": [
                    {
                        "character_id": 100001,
                        "order": [
                            { "identifier": "main", "enabled": true }
                        ]
                    }
                ]
            }
        });
        let base_hash =
            SettingsService::stable_user_settings_hash(&repaired).expect("hash repaired current");
        repository.store_user_settings(current).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let result = service
            .save_user_settings_patch(UserSettingsPatchDto {
                hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                base_hash: base_hash.clone(),
                ops: Vec::new(),
            })
            .await
            .expect("save repaired noop patch");

        assert_eq!(result.mode, "patch-noop");
        assert_eq!(result.settings_hash, base_hash);
        assert_eq!(repository.save_user_settings_count().await, 1);
        assert_eq!(repository.user_settings_data().await, repaired);
        assert_eq!(repository.save_user_settings_revision_count().await, 1);
    }

    #[tokio::test]
    async fn save_user_settings_clears_settings_aggregate_cache_when_payload_changes() {
        let repository = Arc::new(TestSettingsRepository::default());
        repository
            .store_user_settings(json!({"username": "old"}))
            .await;
        repository.store_signature(test_signature("stable")).await;
        let service = SettingsService::new(
            repository.clone(),
            Arc::new(TestRequestProxyRuntime::default()),
            Arc::new(TestChatBackupRuntime::default()),
        );

        let first = service
            .get_sillytavern_settings()
            .await
            .expect("prime settings aggregate cache");
        assert_eq!(settings_value(&first), json!({"username": "old"}));

        service
            .save_user_settings(UserSettingsDto {
                data: json!({"username": "new"}),
            })
            .await
            .expect("save changed settings");

        let second = service
            .get_sillytavern_settings()
            .await
            .expect("reload settings aggregate after save");

        assert_eq!(repository.save_user_settings_count().await, 1);
        assert_eq!(repository.load_user_settings_count().await, 3);
        assert_eq!(settings_value(&second), json!({"username": "new"}));
    }

    #[derive(Default)]
    struct TestSettingsRepository {
        settings: Mutex<TauriTavernSettings>,
        user_settings: Mutex<UserSettings>,
        user_settings_revision: Mutex<Option<UserSettingsRevision>>,
        settings_signature: Mutex<SettingsAggregateSignature>,
        save_user_settings_count: Mutex<u32>,
        load_user_settings_count: Mutex<u32>,
        save_user_settings_revision_count: Mutex<u32>,
    }

    impl TestSettingsRepository {
        async fn store_user_settings(&self, data: Value) {
            *self.user_settings.lock().await = UserSettings { data };
        }

        async fn store_signature(&self, signature: SettingsAggregateSignature) {
            *self.settings_signature.lock().await = signature;
        }

        async fn store_user_settings_revision(&self, settings_hash: String) {
            *self.user_settings_revision.lock().await = Some(UserSettingsRevision {
                hash_algorithm: USER_SETTINGS_HASH_ALGORITHM.to_string(),
                settings_hash,
            });
        }

        async fn user_settings_data(&self) -> Value {
            self.user_settings.lock().await.data.clone()
        }

        async fn save_user_settings_count(&self) -> u32 {
            *self.save_user_settings_count.lock().await
        }

        async fn load_user_settings_count(&self) -> u32 {
            *self.load_user_settings_count.lock().await
        }

        async fn save_user_settings_revision_count(&self) -> u32 {
            *self.save_user_settings_revision_count.lock().await
        }
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

        async fn save_user_settings(&self, settings: &UserSettings) -> Result<(), DomainError> {
            *self.user_settings.lock().await = settings.clone();
            *self.save_user_settings_count.lock().await += 1;
            Ok(())
        }

        async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
            *self.load_user_settings_count.lock().await += 1;
            Ok(self.user_settings.lock().await.clone())
        }

        async fn load_user_settings_revision(
            &self,
        ) -> Result<Option<UserSettingsRevision>, DomainError> {
            Ok(self.user_settings_revision.lock().await.clone())
        }

        async fn save_user_settings_revision(
            &self,
            revision: &UserSettingsRevision,
        ) -> Result<(), DomainError> {
            *self.user_settings_revision.lock().await = Some(revision.clone());
            *self.save_user_settings_revision_count.lock().await += 1;
            Ok(())
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

        async fn get_sillytavern_settings_signature(
            &self,
        ) -> Result<SettingsAggregateSignature, DomainError> {
            Ok(self.settings_signature.lock().await.clone())
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

    fn test_signature(label: &str) -> SettingsAggregateSignature {
        SettingsAggregateSignature::from_revision(label)
    }

    fn settings_value(response: &SillyTavernSettingsResponseDto) -> Value {
        serde_json::from_str(&response.settings).expect("settings should be JSON")
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
