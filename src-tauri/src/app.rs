use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::application::services::avatar_service::AvatarService;
use crate::application::services::background_service::BackgroundService;
use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::content_service::ContentService;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::extension_store_service::ExtensionStoreService;
use crate::application::services::group_chat_service::GroupChatService;
use crate::application::services::group_service::GroupService;
use crate::application::services::lan_sync_service::LanSyncService;
use crate::application::services::preset_service::PresetService;
use crate::application::services::quick_reply_service::QuickReplyService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::settings_service::SettingsService;
use crate::application::services::stable_diffusion_service::StableDiffusionService;
use crate::application::services::theme_service::ThemeService;
use crate::application::services::tokenization_service::TokenizationService;
use crate::application::services::tt_sync_service::TtSyncService;
use crate::application::services::translate_service::TranslateService;
use crate::application::services::update_service::UpdateService;
use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::services::user_service::UserService;
use crate::application::services::world_info_service::WorldInfoService;
use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use crate::infrastructure::paths::RuntimePaths;

mod bootstrap;

pub struct AppState {
    pub character_service: Arc<CharacterService>,
    pub chat_service: Arc<ChatService>,
    pub group_chat_service: Arc<GroupChatService>,
    pub user_service: Arc<UserService>,
    pub settings_service: Arc<SettingsService>,
    pub user_directory_service: Arc<UserDirectoryService>,
    pub secret_service: Arc<SecretService>,
    pub content_service: Arc<ContentService>,
    pub extension_service: Arc<ExtensionService>,
    pub extension_store_service: Arc<ExtensionStoreService>,
    pub avatar_service: Arc<AvatarService>,
    pub group_service: Arc<GroupService>,
    pub background_service: Arc<BackgroundService>,
    pub theme_service: Arc<ThemeService>,
    pub preset_service: Arc<PresetService>,
    pub quick_reply_service: Arc<QuickReplyService>,
    pub chat_completion_service: Arc<ChatCompletionService>,
    pub tokenization_service: Arc<TokenizationService>,
    pub stable_diffusion_service: Arc<StableDiffusionService>,
    pub translate_service: Arc<TranslateService>,
    pub world_info_service: Arc<WorldInfoService>,
    pub lan_sync_service: Arc<LanSyncService>,
    pub tt_sync_service: Arc<TtSyncService>,
    pub update_service: Arc<UpdateService>,
    pub ios_policy: crate::domain::ios_policy::IosPolicyActivationReport,
}

impl AppState {
    pub async fn new(
        app_handle: AppHandle,
        runtime_paths: RuntimePaths,
    ) -> Result<Self, DomainError> {
        tracing::info!(
            "Initializing application in {:?} mode with data root: {:?}",
            runtime_paths.mode,
            runtime_paths.data_root
        );

        let data_directory = bootstrap::initialize_data_directory(&runtime_paths.data_root).await?;

        let services = bootstrap::build_services(&app_handle, &data_directory).await?;

        tracing::info!("Application initialized successfully");

        Ok(Self {
            character_service: services.character_service,
            chat_service: services.chat_service,
            group_chat_service: services.group_chat_service,
            user_service: services.user_service,
            settings_service: services.settings_service,
            user_directory_service: services.user_directory_service,
            secret_service: services.secret_service,
            content_service: services.content_service,
            extension_service: services.extension_service,
            extension_store_service: services.extension_store_service,
            avatar_service: services.avatar_service,
            group_service: services.group_service,
            background_service: services.background_service,
            theme_service: services.theme_service,
            preset_service: services.preset_service,
            quick_reply_service: services.quick_reply_service,
            chat_completion_service: services.chat_completion_service,
            tokenization_service: services.tokenization_service,
            stable_diffusion_service: services.stable_diffusion_service,
            translate_service: services.translate_service,
            world_info_service: services.world_info_service,
            lan_sync_service: services.lan_sync_service,
            tt_sync_service: services.tt_sync_service,
            update_service: services.update_service,
            ios_policy: services.ios_policy,
        })
    }

    pub async fn refresh_after_external_data_change(
        &self,
        reason: &str,
    ) -> Result<(), DomainError> {
        tracing::info!(
            reason = reason,
            "Refreshing runtime caches after external data change"
        );

        self.character_service.clear_cache().await?;
        self.chat_service.clear_cache().await?;
        self.group_chat_service.clear_cache().await?;
        self.group_service.clear_cache().await?;
        self.secret_service.clear_cache().await?;

        Ok(())
    }
}

pub fn spawn_initialization(app_handle: AppHandle, runtime_paths: RuntimePaths) {
    tauri::async_runtime::spawn(async move {
        match AppState::new(app_handle.clone(), runtime_paths).await {
            Ok(state) => {
                app_handle.manage(Arc::new(state));

                let content_service = app_handle.state::<Arc<AppState>>().content_service.clone();
                match content_service
                    .initialize_default_content("default-user")
                    .await
                {
                    Ok(_) => tracing::debug!("Successfully initialized default content"),
                    Err(error) => tracing::warn!("Failed to initialize default content: {}", error),
                }

                match app_handle.emit("app-ready", ()) {
                    Ok(_) => tracing::debug!("Application is ready"),
                    Err(error) => tracing::error!("Failed to emit app-ready event: {}", error),
                }
            }
            Err(error) => {
                logger::error(&format!(
                    "Failed to initialize application state: {}",
                    error
                ));

                match app_handle.emit("app-error", error.to_string()) {
                    Ok(_) => {}
                    Err(emit_error) => {
                        tracing::error!("Failed to emit app-error event: {}", emit_error);
                    }
                }
            }
        }
    });
}
