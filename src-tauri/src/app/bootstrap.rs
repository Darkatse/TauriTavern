use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

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
use crate::application::services::translate_service::TranslateService;
use crate::application::services::tt_sync_service::TtSyncService;
use crate::application::services::update_service::UpdateService;
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
use crate::domain::repositories::extension_store_repository::ExtensionStoreRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::prompt_cache_repository::PromptCacheRepository;
use crate::domain::repositories::quick_reply_repository::QuickReplyRepository;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::domain::repositories::stable_diffusion_repository::StableDiffusionRepository;
use crate::domain::repositories::theme_repository::ThemeRepository;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;
use crate::domain::repositories::translate_repository::TranslateRepository;
use crate::domain::repositories::update_repository::UpdateRepository;
use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::domain::repositories::user_repository::UserRepository;
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::apis::github_update_repository::GitHubUpdateRepository;
use crate::infrastructure::apis::http_chat_completion_repository::HttpChatCompletionRepository;
use crate::infrastructure::apis::http_stable_diffusion_repository::HttpStableDiffusionRepository;
use crate::infrastructure::apis::http_translate_repository::HttpTranslateRepository;
use crate::infrastructure::apis::miktik_tokenizer_repository::MiktikTokenizerRepository;
use crate::infrastructure::http_client_pool::HttpClientPool;
use crate::infrastructure::logging::llm_api_logs::{
    LlmApiLogStore, LoggingChatCompletionRepository,
};
use crate::infrastructure::persistence::file_system::DataDirectory;
use crate::infrastructure::repositories::file_avatar_repository::FileAvatarRepository;
use crate::infrastructure::repositories::file_background_repository::FileBackgroundRepository;
use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_content_repository::FileContentRepository;
use crate::infrastructure::repositories::file_extension_repository::FileExtensionRepository;
use crate::infrastructure::repositories::file_extension_store_repository::FileExtensionStoreRepository;
use crate::infrastructure::repositories::file_group_repository::FileGroupRepository;
use crate::infrastructure::repositories::file_preset_repository::FilePresetRepository;
use crate::infrastructure::repositories::file_prompt_cache_repository::FilePromptCacheRepository;
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

struct AppRepositories {
    character_repository: Arc<dyn CharacterRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    user_repository: Arc<dyn UserRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    user_directory_repository: Arc<dyn UserDirectoryRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    content_repository: Arc<dyn ContentRepository>,
    extension_repository: Arc<dyn ExtensionRepository>,
    extension_store_repository: Arc<dyn ExtensionStoreRepository>,
    avatar_repository: Arc<dyn AvatarRepository>,
    group_repository: Arc<dyn GroupRepository>,
    background_repository: Arc<dyn BackgroundRepository>,
    theme_repository: Arc<dyn ThemeRepository>,
    preset_repository: Arc<dyn PresetRepository>,
    quick_reply_repository: Arc<dyn QuickReplyRepository>,
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    tokenizer_repository: Arc<dyn TokenizerRepository>,
    stable_diffusion_repository: Arc<dyn StableDiffusionRepository>,
    translate_repository: Arc<dyn TranslateRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
    update_repository: Arc<dyn UpdateRepository>,
}

pub(super) async fn initialize_data_directory(
    data_root: &Path,
) -> Result<DataDirectory, DomainError> {
    let data_directory = DataDirectory::new(data_root.to_path_buf());
    data_directory.initialize().await?;
    Ok(data_directory)
}

pub(super) async fn build_services(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
) -> Result<AppServices, DomainError> {
    let repositories = build_repositories(app_handle, data_directory)?;
    let tauritavern_settings = repositories
        .settings_repository
        .load_tauritavern_settings()
        .await?;
    let ios_policy_scope = crate::domain::ios_policy::IosPolicyScope::for_current_platform();
    let ios_policy = if ios_policy_scope == crate::domain::ios_policy::IosPolicyScope::Ios {
        let raw_policy = crate::infrastructure::ios_policy_cache::resolve_effective_raw_policy(
            data_directory.root(),
            tauritavern_settings.ios_policy.as_ref(),
        )
        .await?;
        crate::domain::ios_policy::resolve_ios_policy_activation_report(
            ios_policy_scope,
            raw_policy.as_ref(),
        )?
    } else {
        crate::domain::ios_policy::resolve_ios_policy_activation_report(
            ios_policy_scope,
            tauritavern_settings.ios_policy.as_ref(),
        )?
    };

    let content_service = Arc::new(ContentService::new(repositories.content_repository.clone()));
    let extension_service = Arc::new(ExtensionService::new(
        repositories.extension_repository.clone(),
    ));
    let extension_store_service = Arc::new(ExtensionStoreService::new(
        repositories.extension_store_repository.clone(),
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
        repositories.settings_repository.clone(),
        repositories.prompt_cache_repository.clone(),
        ios_policy.clone(),
    ));
    let tokenization_service =
        Arc::new(TokenizationService::new(repositories.tokenizer_repository));
    let stable_diffusion_service = Arc::new(StableDiffusionService::new(
        repositories.stable_diffusion_repository,
    ));
    let translate_service = Arc::new(TranslateService::new(
        repositories.translate_repository,
        repositories.secret_repository.clone(),
    ));
    let world_info_service = Arc::new(WorldInfoService::new(
        repositories.world_info_repository.clone(),
    ));

    let update_service = Arc::new(UpdateService::new(repositories.update_repository));

    let character_service = Arc::new(CharacterService::new(
        repositories.character_repository.clone(),
        repositories.world_info_repository.clone(),
    ));
    let chat_service = Arc::new(ChatService::new(
        repositories.chat_repository,
        repositories.character_repository.clone(),
    ));
    let group_chat_service = Arc::new(GroupChatService::new(repositories.group_chat_repository));
    let user_service = Arc::new(UserService::new(repositories.user_repository));
    let settings_service = Arc::new(SettingsService::new(repositories.settings_repository));
    let user_directory_service = Arc::new(UserDirectoryService::new(
        repositories.user_directory_repository,
    ));
    let http_client_pool = app_handle.state::<Arc<HttpClientPool>>().inner().clone();
    let sync_permit = Arc::new(Semaphore::new(1));
    let lan_sync_service = Arc::new(LanSyncService::new(
        app_handle.clone(),
        data_directory.root().to_path_buf(),
        data_directory.default_user().to_path_buf(),
        http_client_pool,
        sync_permit.clone(),
    ));
    let tt_sync_service = Arc::new(TtSyncService::new(
        app_handle.clone(),
        data_directory.root().to_path_buf(),
        data_directory.default_user().to_path_buf(),
        sync_permit,
    ));

    let secret_service = Arc::new(SecretService::new(
        repositories.secret_repository,
        tauritavern_settings.allow_keys_exposure,
    ));

    Ok(AppServices {
        character_service,
        chat_service,
        group_chat_service,
        user_service,
        settings_service,
        user_directory_service,
        secret_service,
        content_service,
        extension_service,
        extension_store_service,
        avatar_service,
        group_service,
        background_service,
        theme_service,
        preset_service,
        quick_reply_service,
        chat_completion_service,
        tokenization_service,
        stable_diffusion_service,
        translate_service,
        world_info_service,
        lan_sync_service,
        tt_sync_service,
        update_service,
        ios_policy,
    })
}

fn build_repositories(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
) -> Result<AppRepositories, DomainError> {
    let http_client_pool = app_handle.state::<Arc<HttpClientPool>>().inner().clone();
    let data_root = data_directory.root().to_path_buf();
    let default_user_dir = data_directory.default_user().to_path_buf();

    let character_repository: Arc<dyn CharacterRepository> =
        Arc::new(FileCharacterRepository::new(
            data_directory.characters().to_path_buf(),
            data_directory.chats().to_path_buf(),
            data_directory.default_avatar().to_path_buf(),
        ));

    let file_chat_repository = Arc::new(FileChatRepository::new(
        data_directory.characters().to_path_buf(),
        data_directory.chats().to_path_buf(),
        data_directory.group_chats().to_path_buf(),
        data_directory.backups().to_path_buf(),
    ));
    let chat_repository: Arc<dyn ChatRepository> = file_chat_repository.clone();
    let group_chat_repository: Arc<dyn GroupChatRepository> = file_chat_repository;

    let user_repository: Arc<dyn UserRepository> = Arc::new(FileUserRepository::new(
        data_directory.user_data().to_path_buf(),
    ));

    let settings_repository: Arc<dyn SettingsRepository> = Arc::new(FileSettingsRepository::new(
        data_directory.settings().to_path_buf(),
    ));

    let prompt_cache_repository: Arc<dyn PromptCacheRepository> = Arc::new(
        FilePromptCacheRepository::new(data_root.join("_tauritavern").join("prompt-cache")),
    );

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
            data_directory.global_extensions().to_path_buf(),
            data_directory.extension_sources().to_path_buf(),
            http_client_pool.clone(),
        )?);

    let extension_store_repository: Arc<dyn ExtensionStoreRepository> = Arc::new(
        FileExtensionStoreRepository::new(data_root.join("_tauritavern").join("extension-store")),
    );

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

    let llm_api_log_store = app_handle.state::<Arc<LlmApiLogStore>>().inner().clone();
    let chat_completion_repository: Arc<dyn ChatCompletionRepository> =
        Arc::new(LoggingChatCompletionRepository::new(
            Arc::new(HttpChatCompletionRepository::new(http_client_pool.clone())),
            llm_api_log_store,
        ));
    let tokenizer_cache_dir = data_root.join("_cache").join("tokenizers");
    let tokenizer_repository: Arc<dyn TokenizerRepository> = Arc::new(
        MiktikTokenizerRepository::new(tokenizer_cache_dir, http_client_pool.clone()),
    );

    let stable_diffusion_repository: Arc<dyn StableDiffusionRepository> =
        Arc::new(HttpStableDiffusionRepository::new(
            http_client_pool.clone(),
            default_user_dir.join("user").join("workflows"),
        ));

    let translate_repository: Arc<dyn TranslateRepository> =
        Arc::new(HttpTranslateRepository::new(http_client_pool.clone()));

    let world_info_repository: Arc<dyn WorldInfoRepository> = Arc::new(
        FileWorldInfoRepository::new(data_directory.default_user().join("worlds")),
    );

    let update_repository: Arc<dyn UpdateRepository> =
        Arc::new(GitHubUpdateRepository::new(http_client_pool.clone()));

    Ok(AppRepositories {
        character_repository,
        chat_repository,
        group_chat_repository,
        user_repository,
        settings_repository,
        prompt_cache_repository,
        user_directory_repository,
        secret_repository,
        content_repository,
        extension_repository,
        extension_store_repository,
        avatar_repository,
        group_repository,
        background_repository,
        theme_repository,
        preset_repository,
        quick_reply_repository,
        chat_completion_repository,
        tokenizer_repository,
        stable_diffusion_repository,
        translate_repository,
        world_info_repository,
        update_repository,
    })
}
