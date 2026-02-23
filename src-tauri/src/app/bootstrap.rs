use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;

use crate::application::services::avatar_service::AvatarService;
use crate::application::services::background_service::BackgroundService;
use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::content_service::ContentService;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::group_service::GroupService;
use crate::application::services::preset_service::PresetService;
use crate::application::services::quick_reply_service::QuickReplyService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::settings_service::SettingsService;
use crate::application::services::theme_service::ThemeService;
use crate::application::services::tokenization_service::TokenizationService;
use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::services::user_service::UserService;
use crate::application::services::world_info_service::WorldInfoService;
use crate::domain::errors::DomainError;
use crate::domain::repositories::avatar_repository::AvatarRepository;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::domain::repositories::chat_completion_repository::ChatCompletionRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::content_repository::ContentRepository;
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::quick_reply_repository::QuickReplyRepository;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::domain::repositories::theme_repository::ThemeRepository;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;
use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::domain::repositories::user_repository::UserRepository;
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::apis::http_chat_completion_repository::HttpChatCompletionRepository;
use crate::infrastructure::apis::miktik_tokenizer_repository::MiktikTokenizerRepository;
use crate::infrastructure::persistence::file_system::DataDirectory;
use crate::infrastructure::repositories::file_avatar_repository::FileAvatarRepository;
use crate::infrastructure::repositories::file_background_repository::FileBackgroundRepository;
use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_content_repository::FileContentRepository;
use crate::infrastructure::repositories::file_extension_repository::FileExtensionRepository;
use crate::infrastructure::repositories::file_group_repository::FileGroupRepository;
use crate::infrastructure::repositories::file_preset_repository::FilePresetRepository;
use crate::infrastructure::repositories::file_quick_reply_repository::FileQuickReplyRepository;
use crate::infrastructure::repositories::file_secret_repository::FileSecretRepository;
use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;
use crate::infrastructure::repositories::file_theme_repository::FileThemeRepository;
use crate::infrastructure::repositories::file_user_directory_repository::FileUserDirectoryRepository;
use crate::infrastructure::repositories::file_user_repository::FileUserRepository;
use crate::infrastructure::repositories::file_world_info_repository::FileWorldInfoRepository;

pub(super) struct AppServices {
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
    pub quick_reply_service: Arc<QuickReplyService>,
    pub chat_completion_service: Arc<ChatCompletionService>,
    pub tokenization_service: Arc<TokenizationService>,
    pub world_info_service: Arc<WorldInfoService>,
}

struct AppRepositories {
    character_repository: Arc<dyn CharacterRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    user_repository: Arc<dyn UserRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    user_directory_repository: Arc<dyn UserDirectoryRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    content_repository: Arc<dyn ContentRepository>,
    extension_repository: Arc<dyn ExtensionRepository>,
    avatar_repository: Arc<dyn AvatarRepository>,
    group_repository: Arc<dyn GroupRepository>,
    background_repository: Arc<dyn BackgroundRepository>,
    theme_repository: Arc<dyn ThemeRepository>,
    preset_repository: Arc<dyn PresetRepository>,
    quick_reply_repository: Arc<dyn QuickReplyRepository>,
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    tokenizer_repository: Arc<dyn TokenizerRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
}

pub(super) async fn initialize_data_directory(
    data_root: &Path,
) -> Result<DataDirectory, DomainError> {
    let data_directory = DataDirectory::new(data_root.to_path_buf());
    data_directory.initialize().await?;
    Ok(data_directory)
}

pub(super) fn build_services(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
) -> Result<AppServices, DomainError> {
    let repositories = build_repositories(app_handle, data_directory)?;

    let content_service = Arc::new(ContentService::new(repositories.content_repository.clone()));
    let extension_service = Arc::new(ExtensionService::new(
        repositories.extension_repository.clone(),
    ));
    let avatar_service = Arc::new(AvatarService::new(repositories.avatar_repository.clone()));
    let group_service = Arc::new(GroupService::new(repositories.group_repository.clone()));
    let background_service = Arc::new(BackgroundService::new(
        repositories.background_repository.clone(),
    ));
    let theme_service = Arc::new(ThemeService::new(repositories.theme_repository.clone()));
    let preset_service = Arc::new(PresetService::new(repositories.preset_repository.clone()));
    let quick_reply_service = Arc::new(QuickReplyService::new(
        repositories.quick_reply_repository.clone(),
    ));
    let chat_completion_service = Arc::new(ChatCompletionService::new(
        repositories.chat_completion_repository,
        repositories.secret_repository.clone(),
    ));
    let tokenization_service =
        Arc::new(TokenizationService::new(repositories.tokenizer_repository));
    let world_info_service = Arc::new(WorldInfoService::new(
        repositories.world_info_repository.clone(),
    ));

    let character_service = Arc::new(CharacterService::new(
        repositories.character_repository.clone(),
        repositories.world_info_repository.clone(),
    ));
    let chat_service = Arc::new(ChatService::new(
        repositories.chat_repository,
        repositories.character_repository.clone(),
    ));
    let user_service = Arc::new(UserService::new(repositories.user_repository));
    let settings_service = Arc::new(SettingsService::new(repositories.settings_repository));
    let user_directory_service = Arc::new(UserDirectoryService::new(
        repositories.user_directory_repository,
    ));

    // Do not expose secrets by default; this can be enabled by configuration later.
    let secret_service = Arc::new(SecretService::new(repositories.secret_repository, false));

    Ok(AppServices {
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
        theme_service,
        preset_service,
        quick_reply_service,
        chat_completion_service,
        tokenization_service,
        world_info_service,
    })
}

fn build_repositories(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
) -> Result<AppRepositories, DomainError> {
    let data_root = data_directory.root().to_path_buf();
    let default_user_dir = data_directory.default_user().to_path_buf();

    let character_repository: Arc<dyn CharacterRepository> =
        Arc::new(FileCharacterRepository::new(
            data_directory.characters().to_path_buf(),
            data_directory.chats().to_path_buf(),
            data_directory.default_avatar().to_path_buf(),
        ));

    let chat_repository: Arc<dyn ChatRepository> = Arc::new(FileChatRepository::new(
        data_directory.characters().to_path_buf(),
        data_directory.chats().to_path_buf(),
        data_directory.group_chats().to_path_buf(),
        data_directory.backups().to_path_buf(),
    ));

    let user_repository: Arc<dyn UserRepository> = Arc::new(FileUserRepository::new(
        data_directory.user_data().to_path_buf(),
    ));

    let settings_repository: Arc<dyn SettingsRepository> = Arc::new(FileSettingsRepository::new(
        data_directory.settings().to_path_buf(),
    ));

    let user_directory_repository: Arc<dyn UserDirectoryRepository> =
        Arc::new(FileUserDirectoryRepository::new(data_root.clone()));

    let secret_repository: Arc<dyn SecretRepository> = Arc::new(FileSecretRepository::new(
        default_user_dir.join("secrets.json"),
    ));

    let content_repository: Arc<dyn ContentRepository> = Arc::new(FileContentRepository::new(
        app_handle.clone(),
        default_user_dir.clone(),
    ));

    let extension_repository: Arc<dyn ExtensionRepository> =
        Arc::new(FileExtensionRepository::new(
            default_user_dir.join("extensions"),
            data_root.join("extensions").join("third-party"),
        ));

    let avatar_repository: Arc<dyn AvatarRepository> = Arc::new(FileAvatarRepository::new(
        default_user_dir.join("User Avatars"),
    ));

    let group_repository: Arc<dyn GroupRepository> = Arc::new(FileGroupRepository::new(
        data_directory.groups().to_path_buf(),
        data_directory.group_chats().to_path_buf(),
    ));

    let background_repository: Arc<dyn BackgroundRepository> =
        Arc::new(FileBackgroundRepository::new(
            data_directory.default_user().join("backgrounds"),
            data_directory.default_user().join("thumbnails/bg"),
        ));

    let theme_repository: Arc<dyn ThemeRepository> =
        Arc::new(FileThemeRepository::new(default_user_dir.join("themes")));

    let preset_repository: Arc<dyn PresetRepository> = Arc::new(FilePresetRepository::new(
        app_handle.clone(),
        default_user_dir.clone(),
        content_repository.clone(),
    ));
    let quick_reply_repository: Arc<dyn QuickReplyRepository> = Arc::new(
        FileQuickReplyRepository::new(data_directory.default_user().join("QuickReplies")),
    );

    let chat_completion_repository: Arc<dyn ChatCompletionRepository> =
        Arc::new(HttpChatCompletionRepository::new()?);
    let tokenizer_cache_dir = data_root.join("_cache").join("tokenizers");
    let tokenizer_repository: Arc<dyn TokenizerRepository> =
        Arc::new(MiktikTokenizerRepository::new(tokenizer_cache_dir)?);
    let world_info_repository: Arc<dyn WorldInfoRepository> = Arc::new(
        FileWorldInfoRepository::new(data_directory.default_user().join("worlds")),
    );

    Ok(AppRepositories {
        character_repository,
        chat_repository,
        user_repository,
        settings_repository,
        user_directory_repository,
        secret_repository,
        content_repository,
        extension_repository,
        avatar_repository,
        group_repository,
        background_repository,
        theme_repository,
        preset_repository,
        quick_reply_repository,
        chat_completion_repository,
        tokenizer_repository,
        world_info_repository,
    })
}
