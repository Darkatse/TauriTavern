use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::infrastructure::apis::github_update_repository::GitHubUpdateRepository;
use crate::infrastructure::logging::llm_api_logs::{
    LlmApiLogStore, LoggingChatCompletionRepository,
};
use crate::infrastructure::persistence::file_system::DataDirectory;
use crate::infrastructure::repositories::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_asset_repository::FileAssetRepository;
use crate::infrastructure::repositories::file_avatar_repository::FileAvatarRepository;
use crate::infrastructure::repositories::file_background_repository::FileBackgroundRepository;
use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_content_repository::FileContentRepository;
use crate::infrastructure::repositories::file_extension_repository::FileExtensionRepository;
use crate::infrastructure::repositories::file_extension_store_repository::FileExtensionStoreRepository;
use crate::infrastructure::repositories::file_group_repository::FileGroupRepository;
use crate::infrastructure::repositories::file_image_metadata_repository::FileImageMetadataRepository;
use crate::infrastructure::repositories::file_llm_connection_repository::FileLlmConnectionRepository;
use crate::infrastructure::repositories::file_preset_repository::FilePresetRepository;
use crate::infrastructure::repositories::file_prompt_cache_repository::FilePromptCacheRepository;
use crate::infrastructure::repositories::file_quick_reply_repository::FileQuickReplyRepository;
use crate::infrastructure::repositories::file_secret_repository::FileSecretRepository;
use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;
use crate::infrastructure::repositories::file_skill_repository::FileSkillRepository;
use crate::infrastructure::repositories::file_theme_repository::FileThemeRepository;
use crate::infrastructure::repositories::file_user_directory_repository::FileUserDirectoryRepository;
use crate::infrastructure::repositories::file_user_repository::FileUserRepository;
use crate::infrastructure::repositories::file_world_info_repository::FileWorldInfoRepository;
use tt_adapter_http::HttpClientPool;
use tt_adapter_provider_http::{
    HttpChatCompletionRepository, HttpProviderMetadataRepository, HttpStableDiffusionRepository,
    HttpTranslateRepository, HttpTtsRepository,
};
use tt_adapter_tokenization::MiktikTokenizerRepository;
use tt_domain::errors::DomainError;
use tt_ports::repositories::agent_invocation_repository::AgentInvocationRepository;
use tt_ports::repositories::agent_profile_repository::AgentProfileRepository;
use tt_ports::repositories::agent_profile_storage_health_repository::AgentProfileStorageHealthRepository;
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::agent_workspace_lifecycle_repository::AgentWorkspaceLifecycleRepository;
use tt_ports::repositories::asset_repository::AssetRepository;
use tt_ports::repositories::avatar_repository::AvatarRepository;
use tt_ports::repositories::background_repository::BackgroundRepository;
use tt_ports::repositories::character_repository::CharacterRepository;
use tt_ports::repositories::chat_completion_repository::ChatCompletionRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::checkpoint_repository::CheckpointRepository;
use tt_ports::repositories::content_repository::ContentRepository;
use tt_ports::repositories::extension_repository::ExtensionRepository;
use tt_ports::repositories::extension_store_repository::ExtensionStoreRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::group_repository::GroupRepository;
use tt_ports::repositories::image_metadata_repository::ImageMetadataRepository;
use tt_ports::repositories::llm_connection_repository::LlmConnectionRepository;
use tt_ports::repositories::preset_repository::PresetRepository;
use tt_ports::repositories::prompt_cache_repository::PromptCacheRepository;
use tt_ports::repositories::provider_metadata_repository::ProviderMetadataRepository;
use tt_ports::repositories::quick_reply_repository::QuickReplyRepository;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::settings_repository::SettingsRepository;
use tt_ports::repositories::skill_repository::SkillRepository;
use tt_ports::repositories::stable_diffusion_repository::StableDiffusionRepository;
use tt_ports::repositories::theme_repository::ThemeRepository;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;
use tt_ports::repositories::translate_repository::TranslateRepository;
use tt_ports::repositories::tts_repository::TtsRepository;
use tt_ports::repositories::update_repository::UpdateRepository;
use tt_ports::repositories::user_directory_repository::UserDirectoryRepository;
use tt_ports::repositories::user_repository::UserRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

pub(in crate::app::composition) struct AppRepositories {
    pub(in crate::app::composition) character_repository: Arc<dyn CharacterRepository>,
    pub(in crate::app::composition) chat_repository: Arc<dyn ChatRepository>,
    pub(in crate::app::composition) group_chat_repository: Arc<dyn GroupChatRepository>,
    pub(in crate::app::composition) user_repository: Arc<dyn UserRepository>,
    pub(in crate::app::composition) settings_repository: Arc<dyn SettingsRepository>,
    pub(in crate::app::composition) prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    pub(in crate::app::composition) user_directory_repository: Arc<dyn UserDirectoryRepository>,
    pub(in crate::app::composition) secret_repository: Arc<dyn SecretRepository>,
    pub(in crate::app::composition) skill_repository: Arc<dyn SkillRepository>,
    pub(in crate::app::composition) content_repository: Arc<dyn ContentRepository>,
    pub(in crate::app::composition) asset_repository: Arc<dyn AssetRepository>,
    pub(in crate::app::composition) extension_repository: Arc<dyn ExtensionRepository>,
    pub(in crate::app::composition) extension_store_repository: Arc<dyn ExtensionStoreRepository>,
    pub(in crate::app::composition) avatar_repository: Arc<dyn AvatarRepository>,
    pub(in crate::app::composition) group_repository: Arc<dyn GroupRepository>,
    pub(in crate::app::composition) background_repository: Arc<dyn BackgroundRepository>,
    pub(in crate::app::composition) image_metadata_repository: Arc<dyn ImageMetadataRepository>,
    pub(in crate::app::composition) theme_repository: Arc<dyn ThemeRepository>,
    pub(in crate::app::composition) preset_repository: Arc<dyn PresetRepository>,
    pub(in crate::app::composition) quick_reply_repository: Arc<dyn QuickReplyRepository>,
    pub(in crate::app::composition) agent_profile_repository: Arc<dyn AgentProfileRepository>,
    pub(in crate::app::composition) agent_profile_storage_health_repository:
        Arc<dyn AgentProfileStorageHealthRepository>,
    pub(in crate::app::composition) agent_run_repository: Arc<dyn AgentRunRepository>,
    pub(in crate::app::composition) agent_invocation_repository: Arc<dyn AgentInvocationRepository>,
    pub(in crate::app::composition) agent_workspace_lifecycle_repository:
        Arc<dyn AgentWorkspaceLifecycleRepository>,
    pub(in crate::app::composition) llm_connection_repository: Arc<dyn LlmConnectionRepository>,
    pub(in crate::app::composition) workspace_repository: Arc<dyn WorkspaceRepository>,
    pub(in crate::app::composition) checkpoint_repository: Arc<dyn CheckpointRepository>,
    pub(in crate::app::composition) chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    pub(in crate::app::composition) provider_metadata_repository:
        Arc<dyn ProviderMetadataRepository>,
    pub(in crate::app::composition) tokenizer_repository: Arc<dyn TokenizerRepository>,
    pub(in crate::app::composition) stable_diffusion_repository: Arc<dyn StableDiffusionRepository>,
    pub(in crate::app::composition) translate_repository: Arc<dyn TranslateRepository>,
    pub(in crate::app::composition) tts_repository: Arc<dyn TtsRepository>,
    pub(in crate::app::composition) world_info_repository: Arc<dyn WorldInfoRepository>,
    pub(in crate::app::composition) update_repository: Arc<dyn UpdateRepository>,
}

pub(super) fn build(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
) -> Result<AppRepositories, DomainError> {
    let http_client_pool = app_handle.state::<Arc<HttpClientPool>>().inner().clone();
    let data_root = data_directory.root().to_path_buf();
    let default_user_dir = data_directory.default_user().to_path_buf();
    let chat_aliases = new_shared_chat_alias_store_for_user_dir(data_directory.default_user());

    let character_repository: Arc<dyn CharacterRepository> =
        Arc::new(FileCharacterRepository::with_chat_aliases(
            data_directory.characters().to_path_buf(),
            data_directory.chats().to_path_buf(),
            data_directory
                .default_user()
                .join("thumbnails")
                .join("avatar"),
            data_directory.default_avatar().to_path_buf(),
            chat_aliases.clone(),
        ));

    let file_chat_repository = Arc::new(FileChatRepository::with_chat_aliases(
        data_directory.characters().to_path_buf(),
        data_directory.chats().to_path_buf(),
        data_directory.group_chats().to_path_buf(),
        data_directory.backups().to_path_buf(),
        chat_aliases,
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
    let skill_repository: Arc<dyn SkillRepository> = Arc::new(FileSkillRepository::new(
        data_root.join("_tauritavern").join("skills"),
    ));

    let content_repository: Arc<dyn ContentRepository> = Arc::new(FileContentRepository::new(
        app_handle.clone(),
        data_root.clone(),
        default_user_dir.clone(),
    ));

    let asset_repository: Arc<dyn AssetRepository> = Arc::new(FileAssetRepository::new(
        default_user_dir.clone(),
        default_user_dir.join("assets"),
        default_user_dir.join("characters"),
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
    let image_metadata_repository: Arc<dyn ImageMetadataRepository> =
        Arc::new(FileImageMetadataRepository::new(
            default_user_dir.clone(),
            data_directory.default_user().join("backgrounds"),
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

    let agent_profile_file_repository = Arc::new(FileAgentProfileRepository::new(
        data_root.join("_tauritavern").join("agent-profiles"),
    ));
    let agent_profile_repository: Arc<dyn AgentProfileRepository> =
        agent_profile_file_repository.clone();
    let agent_profile_storage_health_repository: Arc<dyn AgentProfileStorageHealthRepository> =
        agent_profile_file_repository;

    let llm_connection_repository: Arc<dyn LlmConnectionRepository> = Arc::new(
        FileLlmConnectionRepository::new(data_root.join("_tauritavern").join("llm-connections")),
    );

    let file_agent_repository = Arc::new(FileAgentRepository::new(
        data_root.join("_tauritavern").join("agent-workspaces"),
    ));
    let agent_run_repository: Arc<dyn AgentRunRepository> = file_agent_repository.clone();
    let agent_invocation_repository: Arc<dyn AgentInvocationRepository> =
        file_agent_repository.clone();
    let workspace_repository: Arc<dyn WorkspaceRepository> = file_agent_repository.clone();
    let checkpoint_repository: Arc<dyn CheckpointRepository> = file_agent_repository.clone();
    let agent_workspace_lifecycle_repository: Arc<dyn AgentWorkspaceLifecycleRepository> =
        file_agent_repository;

    let llm_api_log_store = app_handle.state::<Arc<LlmApiLogStore>>().inner().clone();
    let chat_completion_repository: Arc<dyn ChatCompletionRepository> =
        Arc::new(LoggingChatCompletionRepository::new(
            Arc::new(HttpChatCompletionRepository::new(http_client_pool.clone())),
            llm_api_log_store,
        ));
    let provider_metadata_repository: Arc<dyn ProviderMetadataRepository> = Arc::new(
        HttpProviderMetadataRepository::new(http_client_pool.clone()),
    );

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
    let tts_repository: Arc<dyn TtsRepository> =
        Arc::new(HttpTtsRepository::new(http_client_pool.clone()));

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
        skill_repository,
        content_repository,
        asset_repository,
        extension_repository,
        extension_store_repository,
        avatar_repository,
        group_repository,
        background_repository,
        image_metadata_repository,
        theme_repository,
        preset_repository,
        quick_reply_repository,
        agent_profile_repository,
        agent_profile_storage_health_repository,
        agent_run_repository,
        agent_invocation_repository,
        agent_workspace_lifecycle_repository,
        llm_connection_repository,
        workspace_repository,
        checkpoint_repository,
        chat_completion_repository,
        provider_metadata_repository,
        tokenizer_repository,
        stable_diffusion_repository,
        translate_repository,
        tts_repository,
        world_info_repository,
        update_repository,
    })
}
