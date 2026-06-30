use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, oneshot};

use crate::application::services::agent_model_gateway::ChatCompletionAgentModelGateway;
use crate::application::services::agent_profile_diagnostic_service::AgentProfileDiagnosticService;
use crate::application::services::agent_profile_service::AgentProfileService;
use crate::application::services::agent_run_history_service::AgentRunHistoryService;
use crate::application::services::agent_run_retention_automation_service::AgentRunRetentionAutomationService;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_workspace_lifecycle_service::{
    AgentRunActivity, AgentWorkspaceLifecycleService,
};
use crate::application::services::asset_service::AssetService;
use crate::application::services::avatar_service::AvatarService;
use crate::application::services::background_service::BackgroundService;
use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::content_service::ContentService;
use crate::application::services::data_archive_service::{
    DataArchiveJobRegistry, DataArchiveService,
};
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::extension_store_service::ExtensionStoreService;
use crate::application::services::external_import_service::ExternalImportDownloader;
use crate::application::services::group_chat_service::GroupChatService;
use crate::application::services::group_service::GroupService;
use crate::application::services::image_metadata_service::ImageMetadataService;
use crate::application::services::lan_sync_service::{
    LanInboundService, LanPairingApprovalRequest, LanPeerRepository, LanServerControl,
    LanSyncRuntimeState, LanSyncService, LanSyncSettingsRepository, PairingApproval,
};
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::application::services::native_regex_service::NativeRegexService;
use crate::application::services::preset_service::PresetService;
use crate::application::services::prompt_assembly_service::PromptAssemblyService;
use crate::application::services::provider_metadata_service::ProviderMetadataService;
use crate::application::services::quick_reply_service::QuickReplyService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::settings_service::{RequestProxyRuntime, SettingsService};
use crate::application::services::skill_service::SkillService;
use crate::application::services::stable_diffusion_service::StableDiffusionService;
use crate::application::services::sync_automation_service::{
    SyncAutomationEndpointCatalog, SyncAutomationEventPublisher, SyncAutomationLanServerControl,
    SyncAutomationService,
};
use crate::application::services::sync_job_coordinator::{
    SyncJobCoordinator, SyncJobEventPublisher,
};
use crate::application::services::theme_service::ThemeService;
use crate::application::services::tokenization_service::TokenizationService;
use crate::application::services::translate_service::TranslateService;
use crate::application::services::tt_sync_service::TtSyncService;
use crate::application::services::tts_service::TtsService;
use crate::application::services::update_service::UpdateService;
use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::services::user_service::UserService;
use crate::application::services::world_info_service::WorldInfoService;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::LanSyncPairRequestEvent;
use crate::domain::models::sync::SyncJobEvent;
use crate::domain::models::sync_automation::{
    SyncAutomationStatus, SyncAutomationTarget, SyncAutomationToastEvent,
};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;
use crate::domain::repositories::agent_profile_repository::AgentProfileRepository;
use crate::domain::repositories::agent_profile_storage_health_repository::AgentProfileStorageHealthRepository;
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::agent_workspace_lifecycle_repository::AgentWorkspaceLifecycleRepository;
use crate::domain::repositories::asset_repository::AssetRepository;
use crate::domain::repositories::avatar_repository::AvatarRepository;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::domain::repositories::chat_completion_repository::ChatCompletionRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::content_repository::ContentRepository;
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::domain::repositories::extension_store_repository::ExtensionStoreRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::domain::repositories::image_metadata_repository::ImageMetadataRepository;
use crate::domain::repositories::llm_connection_repository::LlmConnectionRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::prompt_cache_repository::PromptCacheRepository;
use crate::domain::repositories::provider_metadata_repository::ProviderMetadataRepository;
use crate::domain::repositories::quick_reply_repository::QuickReplyRepository;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::domain::repositories::skill_repository::SkillRepository;
use crate::domain::repositories::stable_diffusion_repository::StableDiffusionRepository;
use crate::domain::repositories::theme_repository::ThemeRepository;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;
use crate::domain::repositories::translate_repository::TranslateRepository;
use crate::domain::repositories::tts_repository::TtsRepository;
use crate::domain::repositories::update_repository::UpdateRepository;
use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::domain::repositories::user_repository::UserRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::apis::github_update_repository::GitHubUpdateRepository;
use crate::infrastructure::apis::http_chat_completion_repository::HttpChatCompletionRepository;
use crate::infrastructure::apis::http_external_import_downloader::HttpExternalImportDownloader;
use crate::infrastructure::apis::http_provider_metadata_repository::HttpProviderMetadataRepository;
use crate::infrastructure::apis::http_stable_diffusion_repository::HttpStableDiffusionRepository;
use crate::infrastructure::apis::http_translate_repository::HttpTranslateRepository;
use crate::infrastructure::apis::http_tts_repository::HttpTtsRepository;
use crate::infrastructure::apis::miktik_tokenizer_repository::MiktikTokenizerRepository;
use crate::infrastructure::http_client_pool::HttpClientPool;
use crate::infrastructure::lan_sync::store::LanSyncStore;
use crate::infrastructure::logging::llm_api_logs::{
    LlmApiLogStore, LoggingChatCompletionRepository,
};
use crate::infrastructure::persistence::data_archive_adapters::{
    DataDirectoryDataRootInitializer, FileDataArchiveExecutor, TauriDataArchiveFileGateway,
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
use crate::infrastructure::sync::http_client::HttpTtPairingClient;
use crate::infrastructure::sync::job_executor::InfrastructureSyncJobExecutor;
use crate::infrastructure::sync::lan::client::HttpLanPairingClient;
use crate::infrastructure::sync::lan::control::AxumLanServerControl;
use crate::infrastructure::sync::lan::discovery::LocalLanAddressDiscovery;
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::sync_automation_store::SyncAutomationStore;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use ttsync_contract::sync::SyncMode;

use super::StartupProfile;

pub(super) struct AppServices {
    pub character_service: Arc<CharacterService>,
    pub chat_service: Arc<ChatService>,
    pub group_chat_service: Arc<GroupChatService>,
    pub user_service: Arc<UserService>,
    pub settings_service: Arc<SettingsService>,
    pub user_directory_service: Arc<UserDirectoryService>,
    pub secret_service: Arc<SecretService>,
    pub skill_service: Arc<SkillService>,
    pub content_service: Arc<ContentService>,
    pub asset_service: Arc<AssetService>,
    pub extension_service: Arc<ExtensionService>,
    pub extension_store_service: Arc<ExtensionStoreService>,
    pub avatar_service: Arc<AvatarService>,
    pub group_service: Arc<GroupService>,
    pub background_service: Arc<BackgroundService>,
    pub image_metadata_service: Arc<ImageMetadataService>,
    pub theme_service: Arc<ThemeService>,
    pub preset_service: Arc<PresetService>,
    pub quick_reply_service: Arc<QuickReplyService>,
    pub agent_profile_service: Arc<AgentProfileService>,
    pub agent_profile_diagnostic_service: Arc<AgentProfileDiagnosticService>,
    pub prompt_assembly_service: Arc<PromptAssemblyService>,
    pub agent_run_history_service: Arc<AgentRunHistoryService>,
    pub agent_run_retention_automation_service: Arc<AgentRunRetentionAutomationService>,
    pub agent_runtime_service: Arc<AgentRuntimeService>,
    pub chat_completion_service: Arc<ChatCompletionService>,
    pub llm_connection_service: Arc<LlmConnectionService>,
    pub provider_metadata_service: Arc<ProviderMetadataService>,
    pub tokenization_service: Arc<TokenizationService>,
    pub stable_diffusion_service: Arc<StableDiffusionService>,
    pub translate_service: Arc<TranslateService>,
    pub tts_service: Arc<TtsService>,
    pub world_info_service: Arc<WorldInfoService>,
    pub lan_sync_service: Arc<LanSyncService>,
    pub tt_sync_service: Arc<TtSyncService>,
    pub sync_automation_service: Arc<SyncAutomationService>,
    pub data_archive_service: Arc<DataArchiveService>,
    pub update_service: Arc<UpdateService>,
    pub native_regex_service: Arc<NativeRegexService>,
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
    skill_repository: Arc<dyn SkillRepository>,
    content_repository: Arc<dyn ContentRepository>,
    asset_repository: Arc<dyn AssetRepository>,
    extension_repository: Arc<dyn ExtensionRepository>,
    extension_store_repository: Arc<dyn ExtensionStoreRepository>,
    avatar_repository: Arc<dyn AvatarRepository>,
    group_repository: Arc<dyn GroupRepository>,
    background_repository: Arc<dyn BackgroundRepository>,
    image_metadata_repository: Arc<dyn ImageMetadataRepository>,
    theme_repository: Arc<dyn ThemeRepository>,
    preset_repository: Arc<dyn PresetRepository>,
    quick_reply_repository: Arc<dyn QuickReplyRepository>,
    agent_profile_repository: Arc<dyn AgentProfileRepository>,
    agent_profile_storage_health_repository: Arc<dyn AgentProfileStorageHealthRepository>,
    agent_run_repository: Arc<dyn AgentRunRepository>,
    agent_invocation_repository: Arc<dyn AgentInvocationRepository>,
    agent_workspace_lifecycle_repository: Arc<dyn AgentWorkspaceLifecycleRepository>,
    llm_connection_repository: Arc<dyn LlmConnectionRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    checkpoint_repository: Arc<dyn CheckpointRepository>,
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    provider_metadata_repository: Arc<dyn ProviderMetadataRepository>,
    tokenizer_repository: Arc<dyn TokenizerRepository>,
    stable_diffusion_repository: Arc<dyn StableDiffusionRepository>,
    translate_repository: Arc<dyn TranslateRepository>,
    tts_repository: Arc<dyn TtsRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
    update_repository: Arc<dyn UpdateRepository>,
}

struct ServiceCacheReconciler {
    character_service: Arc<CharacterService>,
    chat_service: Arc<ChatService>,
    group_chat_service: Arc<GroupChatService>,
    group_service: Arc<GroupService>,
    secret_service: Arc<SecretService>,
}

#[async_trait]
impl DataChangeReconciler for ServiceCacheReconciler {
    async fn reconcile(&self, reason: &str) -> Result<(), DomainError> {
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

struct TauriSyncAutomationEventPublisher {
    app_handle: AppHandle,
}

impl SyncAutomationEventPublisher for TauriSyncAutomationEventPublisher {
    fn publish_status(&self, status: SyncAutomationStatus) {
        if let Err(error) = self.app_handle.emit("sync_auto:status", status) {
            tracing::warn!("Failed to emit sync automation status: {}", error);
        }
    }

    fn publish_toast(&self, event: SyncAutomationToastEvent) {
        if let Err(error) = self.app_handle.emit("sync_auto:toast", event) {
            tracing::warn!("Failed to emit sync automation toast: {}", error);
        }
    }
}

struct TauriSyncJobEventPublisher {
    app_handle: AppHandle,
}

impl SyncJobEventPublisher for TauriSyncJobEventPublisher {
    fn publish_sync_job(&self, event: SyncJobEvent) {
        if let Err(error) = self.app_handle.emit("sync:job", event) {
            tracing::warn!("Failed to emit Sync job event: {}", error);
        }
    }
}

struct TauriPairingApproval {
    app_handle: AppHandle,
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl TauriPairingApproval {
    fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl PairingApproval for TauriPairingApproval {
    async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request.request_id.clone(), tx);
        }

        if let Err(error) = self.app_handle.emit(
            "lan_sync:pair_request",
            LanSyncPairRequestEvent {
                request_id: request.request_id.clone(),
                peer_device_id: request.peer_device_id,
                peer_device_name: request.peer_device_name,
                peer_ip: request.peer_ip,
            },
        ) {
            self.pending.lock().await.remove(&request.request_id);
            return Err(DomainError::InternalError(error.to_string()));
        }

        let timeout = Duration::from_millis(request.expires_at_ms.saturating_sub(now_ms()));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(accepted)) => Ok(accepted),
            Ok(Err(_)) => Err(DomainError::cancelled("Pairing request cancelled")),
            Err(_) => {
                self.pending.lock().await.remove(&request.request_id);
                Err(DomainError::AuthenticationError(
                    "Pairing expired".to_string(),
                ))
            }
        }
    }

    async fn confirm(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        let tx = self
            .pending
            .lock()
            .await
            .remove(request_id)
            .ok_or_else(|| {
                DomainError::NotFound(format!("Pair request not found: {}", request_id))
            })?;

        tx.send(accept).map_err(|_| {
            DomainError::InternalError("Pairing decision receiver dropped".to_string())
        })
    }

    async fn cancel_all(&self) {
        self.pending.lock().await.clear();
    }
}

struct ServiceSyncAutomationLanServerControl {
    lan_sync_service: Arc<LanSyncService>,
    lan_sync_allowed: bool,
}

#[async_trait]
impl SyncAutomationLanServerControl for ServiceSyncAutomationLanServerControl {
    fn validate_allowed(&self) -> Result<(), DomainError> {
        if !self.lan_sync_allowed {
            return Err(DomainError::InvalidData(
                "LAN Sync is not allowed by the current platform policy".to_string(),
            ));
        }
        Ok(())
    }

    async fn start(&self) -> Result<(), DomainError> {
        self.validate_allowed()?;
        self.lan_sync_service.start_server().await.map(|_| ())
    }

    async fn ensure_running(&self) -> Result<(), DomainError> {
        self.validate_allowed()?;
        if !self.lan_sync_service.get_status().await?.running {
            return Err(DomainError::InvalidData(
                "LAN Sync server is not running. Start the sync port before using LAN auto upload."
                    .to_string(),
            ));
        }
        Ok(())
    }
}

struct ServiceSyncAutomationEndpointCatalog {
    lan_sync_service: Arc<LanSyncService>,
    tt_sync_service: Arc<TtSyncService>,
    lan_sync_allowed: bool,
}

#[async_trait]
impl SyncAutomationEndpointCatalog for ServiceSyncAutomationEndpointCatalog {
    async fn validate_target(
        &self,
        target: &SyncAutomationTarget,
        mode: SyncMode,
    ) -> Result<(), DomainError> {
        match target {
            SyncAutomationTarget::Lan { device_id } => {
                if !self.lan_sync_allowed {
                    return Err(DomainError::InvalidData(
                        "LAN Sync is not allowed by the current platform policy".to_string(),
                    ));
                }

                let devices = self.lan_sync_service.list_paired_devices().await?;
                let device = devices
                    .iter()
                    .find(|device| device.device_id == *device_id)
                    .ok_or_else(|| {
                        DomainError::NotFound(format!("LAN Sync device not found: {device_id}"))
                    })?;
                if device.last_known_address.is_none() {
                    return Err(DomainError::InvalidData(
                        "LAN auto upload requires a paired LAN Sync device with an address"
                            .to_string(),
                    ));
                }
            }
            SyncAutomationTarget::Tt { server_device_id } => {
                let servers = self.tt_sync_service.list_servers().await?;
                let server = servers
                    .iter()
                    .find(|server| server.server_device_id.as_str() == server_device_id.as_str())
                    .ok_or_else(|| {
                        DomainError::NotFound(format!(
                            "TT-Sync server not found: {server_device_id}"
                        ))
                    })?;
                if !server.permissions.write {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant write permission".to_string(),
                    ));
                }
                if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant mirror_delete permission".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
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
    startup_profile: &StartupProfile,
) -> Result<AppServices, DomainError> {
    let repositories = build_repositories(app_handle, data_directory)?;
    let tauritavern_settings = &startup_profile.tauritavern_settings;
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
    let extension_service = Arc::new(ExtensionService::new(
        repositories.extension_repository.clone(),
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
    let agent_profile_service = Arc::new(AgentProfileService::new(
        repositories.agent_profile_repository.clone(),
        repositories.agent_profile_storage_health_repository.clone(),
        repositories.preset_repository.clone(),
    ));
    let agent_profile_diagnostic_service = Arc::new(AgentProfileDiagnosticService::new(
        agent_profile_service.clone(),
        repositories.preset_repository.clone(),
        llm_connection_service.clone(),
    ));
    let prompt_assembly_service = Arc::new(PromptAssemblyService::new(
        agent_profile_service.clone(),
        repositories.preset_repository.clone(),
        llm_connection_service.clone(),
    ));
    let chat_completion_service = Arc::new(ChatCompletionService::new(
        repositories.chat_completion_repository,
        repositories.secret_repository.clone(),
        repositories.settings_repository.clone(),
        repositories.prompt_cache_repository.clone(),
        ios_policy.clone(),
    ));
    let provider_metadata_service = Arc::new(ProviderMetadataService::new(
        repositories.provider_metadata_repository,
        repositories.secret_repository.clone(),
        ios_policy.clone(),
    ));
    let agent_runtime_service = Arc::new(AgentRuntimeService::new_with_prompt_assembly_service(
        repositories.agent_run_repository.clone(),
        repositories.agent_invocation_repository.clone(),
        repositories.workspace_repository.clone(),
        repositories.checkpoint_repository.clone(),
        repositories.chat_repository.clone(),
        repositories.group_chat_repository.clone(),
        skill_service.clone(),
        Arc::new(ChatCompletionAgentModelGateway::new(
            chat_completion_service.clone(),
        )),
        agent_profile_service.clone(),
        llm_connection_service.clone(),
        prompt_assembly_service.clone(),
    ));
    let agent_run_history_service = Arc::new(AgentRunHistoryService::new(
        repositories.agent_run_repository.clone(),
        repositories.settings_repository.clone(),
        agent_runtime_service.clone() as Arc<dyn AgentRunActivity>,
    ));
    let agent_run_retention_automation_service = Arc::new(AgentRunRetentionAutomationService::new(
        repositories.settings_repository.clone(),
        agent_run_history_service.clone(),
    ));
    let agent_workspace_lifecycle_service = Arc::new(AgentWorkspaceLifecycleService::new(
        repositories.agent_workspace_lifecycle_repository.clone(),
        agent_runtime_service.clone() as Arc<dyn AgentRunActivity>,
    ));
    let tokenization_service =
        Arc::new(TokenizationService::new(repositories.tokenizer_repository));
    let native_regex_service = Arc::new(NativeRegexService::new());
    let stable_diffusion_service = Arc::new(StableDiffusionService::new(
        repositories.stable_diffusion_repository,
        repositories.secret_repository.clone(),
    ));
    let translate_service = Arc::new(TranslateService::new(
        repositories.translate_repository,
        repositories.secret_repository.clone(),
    ));
    let tts_service = Arc::new(TtsService::new(
        repositories.tts_repository,
        repositories.secret_repository.clone(),
    ));
    let world_info_service = Arc::new(WorldInfoService::new(
        repositories.world_info_repository.clone(),
    ));

    let update_service = Arc::new(UpdateService::new(repositories.update_repository));

    let group_service = Arc::new(GroupService::new(
        repositories.group_repository.clone(),
        agent_workspace_lifecycle_service.clone(),
    ));
    let character_service = Arc::new(CharacterService::new(
        repositories.character_repository.clone(),
        repositories.chat_repository.clone(),
        repositories.world_info_repository.clone(),
        agent_workspace_lifecycle_service.clone(),
    ));
    let chat_service = Arc::new(ChatService::new(
        repositories.chat_repository,
        repositories.character_repository.clone(),
        agent_workspace_lifecycle_service.clone(),
    ));
    let group_chat_service = Arc::new(GroupChatService::new(
        repositories.group_chat_repository,
        agent_workspace_lifecycle_service,
    ));
    let secret_service = Arc::new(SecretService::new(
        repositories.secret_repository.clone(),
        tauritavern_settings.allow_keys_exposure,
    ));
    let user_service = Arc::new(UserService::new(repositories.user_repository));
    let settings_service = Arc::new(SettingsService::new(
        repositories.settings_repository,
        request_proxy_runtime,
    ));
    let user_directory_service = Arc::new(UserDirectoryService::new(
        repositories.user_directory_repository,
    ));
    let data_change_reconciler: Arc<dyn DataChangeReconciler> = Arc::new(ServiceCacheReconciler {
        character_service: character_service.clone(),
        chat_service: chat_service.clone(),
        group_chat_service: group_chat_service.clone(),
        group_service: group_service.clone(),
        secret_service: secret_service.clone(),
    });
    let data_archive_job_registry = Arc::new(DataArchiveJobRegistry::new());
    let data_archive_service = Arc::new(DataArchiveService::new(
        data_archive_job_registry,
        tauri::async_runtime::handle().inner().clone(),
        Arc::new(FileDataArchiveExecutor),
        Arc::new(TauriDataArchiveFileGateway::new(app_handle.clone())),
        Arc::new(DataDirectoryDataRootInitializer),
        data_change_reconciler.clone(),
    ));
    let lan_runtime_state = Arc::new(LanSyncRuntimeState::new());
    let lan_settings_store = Arc::new(LanSyncStore::new(
        data_directory.default_user().to_path_buf(),
    ));
    let lan_peer_store = LanPeerStore::new(data_directory.default_user().to_path_buf());
    let lan_settings_repository: Arc<dyn LanSyncSettingsRepository> = lan_settings_store.clone();
    let lan_peer_repository: Arc<dyn LanPeerRepository> = Arc::new(lan_peer_store.clone());
    let sync_job_events: Arc<dyn SyncJobEventPublisher> = Arc::new(TauriSyncJobEventPublisher {
        app_handle: app_handle.clone(),
    });
    let pairing_approval: Arc<dyn PairingApproval> =
        Arc::new(TauriPairingApproval::new(app_handle.clone()));
    let tt_runtime = Arc::new(TtSyncRuntime::new(
        data_directory.root().to_path_buf(),
        data_directory.default_user().to_path_buf(),
    ));
    let sync_job_executor = Arc::new(InfrastructureSyncJobExecutor::new(
        data_directory.root().to_path_buf(),
        sync_job_events.clone(),
        lan_peer_store.clone(),
        tt_runtime.clone(),
    ));
    let sync_job_coordinator = Arc::new(SyncJobCoordinator::new(
        sync_job_executor,
        data_change_reconciler.clone(),
        sync_job_events,
    ));
    let lan_inbound_service = Arc::new(LanInboundService::new(
        lan_runtime_state.clone(),
        lan_settings_repository.clone(),
        lan_peer_repository.clone(),
        sync_job_coordinator.clone(),
        pairing_approval.clone(),
    ));
    let lan_server_control: Arc<dyn LanServerControl> = Arc::new(AxumLanServerControl::new(
        data_directory.root().to_path_buf(),
        lan_peer_store.clone(),
        lan_inbound_service.clone(),
    ));
    let lan_sync_service = Arc::new(LanSyncService::new(
        lan_runtime_state,
        lan_settings_repository,
        lan_peer_repository,
        lan_server_control,
        Arc::new(LocalLanAddressDiscovery),
        Arc::new(HttpLanPairingClient),
        pairing_approval,
        sync_job_coordinator.clone(),
    ));
    let tt_sync_service = Arc::new(TtSyncService::new(
        tt_runtime.clone(),
        Arc::new(HttpTtPairingClient),
        sync_job_coordinator.clone(),
    ));
    let sync_automation_events = Arc::new(TauriSyncAutomationEventPublisher {
        app_handle: app_handle.clone(),
    });
    let sync_automation_rules = Arc::new(SyncAutomationStore::new(
        data_directory.default_user().to_path_buf(),
    ));
    let sync_automation_lan_settings = lan_settings_store.clone();
    let sync_automation_endpoint_catalog = Arc::new(ServiceSyncAutomationEndpointCatalog {
        lan_sync_service: lan_sync_service.clone(),
        tt_sync_service: tt_sync_service.clone(),
        lan_sync_allowed: ios_policy.capabilities.sync.lan,
    });
    let sync_automation_lan_server = Arc::new(ServiceSyncAutomationLanServerControl {
        lan_sync_service: lan_sync_service.clone(),
        lan_sync_allowed: ios_policy.capabilities.sync.lan,
    });
    let sync_automation_service = Arc::new(SyncAutomationService::new(
        sync_automation_events,
        sync_automation_rules,
        sync_automation_lan_settings,
        sync_automation_endpoint_catalog,
        sync_automation_lan_server,
        sync_job_coordinator.clone(),
    ));

    Ok(AppServices {
        character_service,
        chat_service,
        group_chat_service,
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
        agent_profile_service,
        agent_profile_diagnostic_service,
        prompt_assembly_service,
        agent_run_history_service,
        agent_run_retention_automation_service,
        agent_runtime_service,
        chat_completion_service,
        llm_connection_service,
        provider_metadata_service,
        tokenization_service,
        stable_diffusion_service,
        translate_service,
        tts_service,
        world_info_service,
        lan_sync_service,
        tt_sync_service,
        sync_automation_service,
        data_archive_service,
        update_service,
        native_regex_service,
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
