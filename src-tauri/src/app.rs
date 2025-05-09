use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;

use crate::domain::repositories::character_repository::CharacterRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::user_repository::UserRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::content_repository::ContentRepository;
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::domain::repositories::avatar_repository::AvatarRepository;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::domain::repositories::background_repository::BackgroundRepository;

use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_user_repository::FileUserRepository;
use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;
use crate::infrastructure::repositories::file_user_directory_repository::FileUserDirectoryRepository;
use crate::infrastructure::repositories::file_secret_repository::FileSecretRepository;
use crate::infrastructure::repositories::file_content_repository::FileContentRepository;
use crate::infrastructure::repositories::file_extension_repository::FileExtensionRepository;
use crate::infrastructure::repositories::file_avatar_repository::FileAvatarRepository;
use crate::infrastructure::repositories::file_group_repository::FileGroupRepository;
use crate::infrastructure::repositories::file_background_repository::FileBackgroundRepository;

use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::user_service::UserService;
use crate::application::services::settings_service::SettingsService;
use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::content_service::ContentService;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::avatar_service::AvatarService;
use crate::application::services::group_service::GroupService;
use crate::application::services::background_service::BackgroundService;

use crate::infrastructure::persistence::file_system::DataDirectory;
use crate::infrastructure::logging::logger;

use crate::domain::errors::DomainError;

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
}

impl AppState {
    pub async fn new(app_handle: AppHandle, data_root: &Path) -> Result<Self, DomainError> {
        tracing::info!("{}", &format!("Initializing application with data root: {:?}", data_root));

        // Create data directory structure
        let data_directory = DataDirectory::new(data_root.to_path_buf());
        data_directory.initialize().await?;

        // Create repositories
        // 现在角色和聊天文件夹位于default-user目录下
        let character_repository: Arc<dyn CharacterRepository> = Arc::new(
            FileCharacterRepository::new(
                data_directory.characters().to_path_buf(),
                data_directory.chats().to_path_buf(),
                data_directory.default_avatar().to_path_buf()
            )
        );

        let chat_repository: Arc<dyn ChatRepository> = Arc::new(
            FileChatRepository::new(data_directory.chats().to_path_buf())
        );

        // user_data现在等同于default-user
        let user_repository: Arc<dyn UserRepository> = Arc::new(
            FileUserRepository::new(data_directory.user_data().to_path_buf())
        );

        // settings.json位于default-user目录下
        let settings_repository: Arc<dyn SettingsRepository> = Arc::new(
            FileSettingsRepository::new(data_directory.settings().to_path_buf())
        );

        let user_directory_repository: Arc<dyn UserDirectoryRepository> = Arc::new(
            FileUserDirectoryRepository::new(app_handle.clone())
        );

        // 创建密钥仓库，使用default-user目录
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(
            FileSecretRepository::new(app_handle.clone())
        );

        // 创建内容仓库，用于复制默认文件
        let content_repository: Arc<dyn ContentRepository> = Arc::new(
            FileContentRepository::new(
                app_handle.clone(),
                data_directory.default_user().to_path_buf()
            )
        );

        // 创建扩展仓库
        let extension_repository: Arc<dyn ExtensionRepository> = Arc::new(
            FileExtensionRepository::new(app_handle.clone())
        );

        // 创建头像仓库
        let avatar_repository: Arc<dyn AvatarRepository> = Arc::new(
            FileAvatarRepository::new(app_handle.clone())
        );

        // 创建群组仓库
        let group_repository: Arc<dyn GroupRepository> = Arc::new(
            FileGroupRepository::new(
                data_directory.groups().to_path_buf(),
                data_directory.group_chats().to_path_buf()
            )
        );

        // 创建背景仓库
        let background_repository: Arc<dyn BackgroundRepository> = Arc::new(
            FileBackgroundRepository::new(
                app_handle.clone(),
                data_directory.default_user().join("backgrounds").to_path_buf()
            )
        );

        // 创建内容服务
        let content_service = Arc::new(
            ContentService::new(content_repository.clone())
        );

        // 创建扩展服务
        let extension_service = Arc::new(
            ExtensionService::new(extension_repository.clone())
        );

        // 创建头像服务
        let avatar_service = Arc::new(
            AvatarService::new(avatar_repository.clone())
        );

        // 创建群组服务
        let group_service = Arc::new(
            GroupService::new(group_repository.clone())
        );

        // 创建背景服务
        let background_service = Arc::new(
            BackgroundService::new(background_repository.clone())
        );

        // Create services
        let character_service = Arc::new(
            CharacterService::new(character_repository.clone())
        );

        let chat_service = Arc::new(
            ChatService::new(chat_repository, character_repository.clone())
        );

        let user_service = Arc::new(
            UserService::new(user_repository.clone())
        );

        let settings_service = Arc::new(
            SettingsService::new(settings_repository.clone())
        );

        let user_directory_service = Arc::new(
            UserDirectoryService::new(user_directory_repository.clone())
        );

        // 默认不允许暴露密钥，可以通过配置文件修改
        let secret_service = Arc::new(
            SecretService::new(secret_repository.clone(), false)
        );

        tracing::info!("Application initialized successfully");

        Ok(Self {
            data_directory,
            character_service,
            chat_service,
            user_service,
            settings_service,
            user_directory_service,
            secret_service,
            content_service,
            extension_service,
            avatar_service,
            group_service,
            background_service,
        })
    }
}
