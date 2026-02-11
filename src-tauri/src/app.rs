use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::application::services::avatar_service::AvatarService;
use crate::application::services::background_service::BackgroundService;
use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::content_service::ContentService;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::group_service::GroupService;
use crate::application::services::preset_service::PresetService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::settings_service::SettingsService;
use crate::application::services::theme_service::ThemeService;
use crate::application::services::tokenization_service::TokenizationService;
use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::services::user_service::UserService;
use crate::application::services::world_info_service::WorldInfoService;
use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::DataDirectory;

mod bootstrap;

pub struct AppState {
    pub data_directory: DataDirectory,
    pub character_service: Arc<CharacterService>,
    pub chat_service: Arc<ChatService>,
    pub user_service: Arc<UserService>,
    pub settings_service: Arc<SettingsService>,
    pub user_directory_service: Arc<UserDirectoryService>,
    pub secret_service: Arc<SecretService>,
    pub content_service: Arc<ContentService>,
    pub extension_service: Arc<ExtensionService>,
    pub avatar_service: Arc<AvatarService>,
    pub group_service: Arc<GroupService>,
    pub background_service: Arc<BackgroundService>,
    pub theme_service: Arc<ThemeService>,
    pub preset_service: Arc<PresetService>,
    pub chat_completion_service: Arc<ChatCompletionService>,
    pub tokenization_service: Arc<TokenizationService>,
    pub world_info_service: Arc<WorldInfoService>,
}

impl AppState {
    pub async fn new(app_handle: AppHandle, data_root: &Path) -> Result<Self, DomainError> {
        tracing::info!("Initializing application with data root: {:?}", data_root);

        let data_directory = bootstrap::initialize_data_directory(data_root).await?;
        let services = bootstrap::build_services(&app_handle, &data_directory)?;

        tracing::info!("Application initialized successfully");

        Ok(Self {
            data_directory,
            character_service: services.character_service,
            chat_service: services.chat_service,
            user_service: services.user_service,
            settings_service: services.settings_service,
            user_directory_service: services.user_directory_service,
            secret_service: services.secret_service,
            content_service: services.content_service,
            extension_service: services.extension_service,
            avatar_service: services.avatar_service,
            group_service: services.group_service,
            background_service: services.background_service,
            theme_service: services.theme_service,
            preset_service: services.preset_service,
            chat_completion_service: services.chat_completion_service,
            tokenization_service: services.tokenization_service,
            world_info_service: services.world_info_service,
        })
    }
}

pub fn resolve_data_root(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let app_data_dir = app_handle.path().app_data_dir()?;
    tracing::info!("App data directory: {:?}", app_data_dir);

    let data_root = app_data_dir.join("data");
    tracing::info!("Data root directory: {:?}", data_root);

    std::fs::create_dir_all(&data_root)?;
    Ok(data_root)
}

pub fn spawn_initialization(app_handle: AppHandle, data_root: PathBuf) {
    tauri::async_runtime::spawn(async move {
        match AppState::new(app_handle.clone(), &data_root).await {
            Ok(state) => {
                app_handle.manage(Arc::new(state));

                let content_service = app_handle.state::<Arc<AppState>>().content_service.clone();
                if let Err(error) = content_service
                    .initialize_default_content("default-user")
                    .await
                {
                    tracing::warn!("Failed to initialize default content: {}", error);
                } else {
                    tracing::info!("Successfully initialized default content");
                }

                if let Err(error) = app_handle.emit("app-ready", ()) {
                    tracing::error!("Failed to emit app-ready event: {}", error);
                } else {
                    tracing::info!("Application is ready");
                }
            }
            Err(error) => {
                tracing::error!("Failed to initialize application state: {}", error);

                if let Err(emit_error) = app_handle.emit("app-error", error.to_string()) {
                    tracing::error!("Failed to emit app-error event: {}", emit_error);
                }
            }
        }
    });
}
