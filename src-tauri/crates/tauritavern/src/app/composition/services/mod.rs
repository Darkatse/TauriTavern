mod agent;
mod archive;
mod sync;

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

use crate::app::{AppServices, StartupProfile};
use crate::infrastructure::apis::http_external_import_downloader::HttpExternalImportDownloader;
use tt_adapter_http::HttpClientPool;
use tt_adapter_storage_core::file_system::DataDirectory;
use tt_application::services::asset_service::AssetService;
use tt_application::services::avatar_service::AvatarService;
use tt_application::services::background_service::BackgroundService;
use tt_application::services::character_service::CharacterService;
use tt_application::services::chat_completion_service::ChatCompletionService;
use tt_application::services::chat_history_coordinator::ChatHistoryCoordinator;
use tt_application::services::chat_payload_commit_service::ChatPayloadCommitService;
use tt_application::services::chat_service::ChatService;
use tt_application::services::content_service::ContentService;
use tt_application::services::extension_service::ExtensionService;
use tt_application::services::extension_store_service::ExtensionStoreService;
use tt_application::services::external_import_service::ExternalImportDownloader;
use tt_application::services::group_chat_service::GroupChatService;
use tt_application::services::group_service::GroupService;
use tt_application::services::image_metadata_service::ImageMetadataService;
use tt_application::services::llm_connection_service::LlmConnectionService;
use tt_application::services::native_regex_service::NativeRegexService;
use tt_application::services::preset_service::PresetService;
use tt_application::services::provider_metadata_service::ProviderMetadataService;
use tt_application::services::quick_reply_service::QuickReplyService;
use tt_application::services::secret_service::SecretService;
use tt_application::services::settings_service::{RequestProxyRuntime, SettingsService};
use tt_application::services::skill_service::SkillService;
use tt_application::services::stable_diffusion_service::StableDiffusionService;
use tt_application::services::theme_service::ThemeService;
use tt_application::services::tokenization_service::TokenizationService;
use tt_application::services::translate_service::TranslateService;
use tt_application::services::tts_service::TtsService;
use tt_application::services::update_service::UpdateService;
use tt_application::services::user_directory_service::UserDirectoryService;
use tt_application::services::user_service::UserService;
use tt_application::services::world_info_service::WorldInfoService;
use tt_domain::errors::DomainError;

use super::{adapters, repositories};

pub(super) async fn build(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
    startup_profile: &StartupProfile,
) -> Result<AppServices, DomainError> {
    let tauritavern_settings = &startup_profile.tauritavern_settings;
    let repositories = repositories::build(
        app_handle,
        data_directory,
        tauritavern_settings.chat_backups,
    )
    .await?;
    let ios_policy = startup_profile.ios_policy.clone();

    let http_client_pool = app_handle.state::<Arc<HttpClientPool>>().inner().clone();
    let external_import_downloader: Arc<dyn ExternalImportDownloader> =
        Arc::new(HttpExternalImportDownloader::new(http_client_pool.clone()));
    let request_proxy_runtime: Arc<dyn RequestProxyRuntime> = http_client_pool;

    let content_service = Arc::new(ContentService::new(
        repositories.content_repository.clone(),
        external_import_downloader.clone(),
    ));
    let asset_service = Arc::new(AssetService::new(
        repositories.asset_repository.clone(),
        external_import_downloader.clone(),
    ));
    let local_mutation_gate = Arc::new(Semaphore::new(1));
    let extension_service = Arc::new(ExtensionService::new(
        repositories.extension_repository.clone(),
        local_mutation_gate.clone(),
    ));
    let extension_store_service = Arc::new(ExtensionStoreService::new(
        repositories.extension_store_repository.clone(),
    ));
    let avatar_service = Arc::new(AvatarService::new(repositories.avatar_repository.clone()));
    let image_metadata_service = Arc::new(ImageMetadataService::new(
        repositories.image_metadata_repository.clone(),
    ));
    let background_service = Arc::new(BackgroundService::new(
        repositories.background_repository.clone(),
        repositories.image_metadata_repository.clone(),
    ));
    let theme_service = Arc::new(ThemeService::new(repositories.theme_repository.clone()));
    let preset_service = Arc::new(PresetService::new(repositories.preset_repository.clone()));
    let quick_reply_service = Arc::new(QuickReplyService::new(
        repositories.quick_reply_repository.clone(),
    ));
    let skill_service = Arc::new(SkillService::with_external_import_downloader(
        repositories.skill_repository.clone(),
        external_import_downloader,
    ));
    let llm_connection_service = Arc::new(LlmConnectionService::new(
        repositories.llm_connection_repository.clone(),
    ));
    let chat_completion_service = Arc::new(ChatCompletionService::new(
        repositories.chat_completion_repository.clone(),
        repositories.secret_repository.clone(),
        repositories.settings_repository.clone(),
        repositories.prompt_cache_repository.clone(),
        ios_policy.clone(),
    ));
    let provider_metadata_service = Arc::new(ProviderMetadataService::new(
        repositories.provider_metadata_repository.clone(),
        repositories.secret_repository.clone(),
        ios_policy.clone(),
    ));
    let agent_services = agent::build(
        &repositories,
        skill_service.clone(),
        chat_completion_service.clone(),
        llm_connection_service.clone(),
    );
    let tokenization_service = Arc::new(TokenizationService::new(
        repositories.tokenizer_repository.clone(),
    ));
    let native_regex_service = Arc::new(NativeRegexService::new());
    let stable_diffusion_service = Arc::new(StableDiffusionService::new(
        repositories.stable_diffusion_repository.clone(),
        repositories.secret_repository.clone(),
    ));
    let translate_service = Arc::new(TranslateService::new(
        repositories.translate_repository.clone(),
        repositories.secret_repository.clone(),
    ));
    let tts_service = Arc::new(TtsService::new(
        repositories.tts_repository.clone(),
        repositories.secret_repository.clone(),
    ));
    let world_info_service = Arc::new(WorldInfoService::new(
        repositories.world_info_repository.clone(),
    ));
    let update_service = Arc::new(UpdateService::new(
        repositories.update_repository.clone(),
        crate::product::VERSION,
        crate::product::optional_build_value(crate::product::GIT_REVISION),
    ));

    let chat_history_coordinator = Arc::new(ChatHistoryCoordinator::new(
        repositories.chat_repository.clone(),
        repositories.group_chat_repository.clone(),
    ));
    let chat_payload_commit_service = Arc::new(ChatPayloadCommitService::new(
        repositories.chat_payload_commit_repository.clone(),
        chat_history_coordinator.clone(),
    ));

    let group_service = Arc::new(GroupService::new(
        repositories.group_repository.clone(),
        agent_services.agent_workspace_lifecycle_service.clone(),
        chat_history_coordinator.clone(),
    ));
    let character_service = Arc::new(CharacterService::new(
        repositories.character_repository.clone(),
        repositories.chat_repository.clone(),
        repositories.world_info_repository.clone(),
        agent_services.agent_workspace_lifecycle_service.clone(),
        chat_history_coordinator.clone(),
    ));
    let chat_service = Arc::new(ChatService::new(
        repositories.chat_repository.clone(),
        repositories.character_repository.clone(),
        agent_services.agent_workspace_lifecycle_service.clone(),
        chat_history_coordinator.clone(),
    ));
    let group_chat_service = Arc::new(GroupChatService::new(
        repositories.group_chat_repository.clone(),
        agent_services.agent_workspace_lifecycle_service,
        chat_history_coordinator.clone(),
    ));
    let secret_service = Arc::new(SecretService::new(
        repositories.secret_repository.clone(),
        tauritavern_settings.allow_keys_exposure,
    ));
    let user_service = Arc::new(UserService::new(repositories.user_repository.clone()));
    let settings_service = Arc::new(SettingsService::new(
        repositories.settings_repository.clone(),
        request_proxy_runtime,
        repositories.chat_backup_runtime.clone(),
    ));
    let user_directory_service = Arc::new(UserDirectoryService::new(
        repositories.user_directory_repository.clone(),
    ));
    let data_change_reconciler = adapters::data_change_reconciler(
        character_service.clone(),
        chat_service.clone(),
        group_chat_service.clone(),
        group_service.clone(),
        secret_service.clone(),
        settings_service.clone(),
        chat_history_coordinator.clone(),
    );
    let data_archive_service = archive::build(app_handle, data_change_reconciler.clone());
    let sync_services = sync::build(
        app_handle,
        data_directory,
        data_change_reconciler,
        &ios_policy,
        local_mutation_gate,
    );

    Ok(AppServices {
        character_service,
        chat_service,
        group_chat_service,
        chat_history_coordinator,
        chat_payload_commit_service,
        user_service,
        settings_service,
        user_directory_service,
        secret_service,
        skill_service,
        content_service,
        asset_service,
        extension_service,
        extension_store_service,
        avatar_service,
        group_service,
        background_service,
        image_metadata_service,
        theme_service,
        preset_service,
        quick_reply_service,
        agent_profile_service: agent_services.agent_profile_service,
        agent_profile_diagnostic_service: agent_services.agent_profile_diagnostic_service,
        prompt_assembly_service: agent_services.prompt_assembly_service,
        agent_run_history_service: agent_services.agent_run_history_service,
        agent_run_retention_automation_service: agent_services
            .agent_run_retention_automation_service,
        agent_runtime_service: agent_services.agent_runtime_service,
        chat_completion_service,
        llm_connection_service,
        provider_metadata_service,
        tokenization_service,
        stable_diffusion_service,
        translate_service,
        tts_service,
        world_info_service,
        lan_sync_service: sync_services.lan_sync_service,
        tt_sync_service: sync_services.tt_sync_service,
        sync_automation_service: sync_services.sync_automation_service,
        data_archive_service,
        update_service,
        native_regex_service,
    })
}
