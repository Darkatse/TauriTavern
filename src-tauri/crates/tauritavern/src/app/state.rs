use std::sync::Arc;

use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use crate::infrastructure::paths::RuntimePaths;
use tt_application::services::agent_profile_diagnostic_service::AgentProfileDiagnosticService;
use tt_application::services::agent_profile_service::AgentProfileService;
use tt_application::services::agent_run_history_service::AgentRunHistoryService;
use tt_application::services::agent_run_retention_automation_service::AgentRunRetentionAutomationService;
use tt_application::services::agent_runtime_service::AgentRuntimeService;
use tt_application::services::asset_service::AssetService;
use tt_application::services::avatar_service::AvatarService;
use tt_application::services::background_service::BackgroundService;
use tt_application::services::character_service::CharacterService;
use tt_application::services::chat_completion_service::ChatCompletionService;
use tt_application::services::chat_history_coordinator::ChatHistoryCoordinator;
use tt_application::services::chat_payload_commit_service::ChatPayloadCommitService;
use tt_application::services::chat_service::ChatService;
use tt_application::services::content_service::ContentService;
use tt_application::services::data_archive_service::DataArchiveService;
use tt_application::services::extension_service::ExtensionService;
use tt_application::services::extension_store_service::ExtensionStoreService;
use tt_application::services::group_chat_service::GroupChatService;
use tt_application::services::group_service::GroupService;
use tt_application::services::image_metadata_service::ImageMetadataService;
use tt_application::services::lan_sync_service::LanSyncService;
use tt_application::services::llm_connection_service::LlmConnectionService;
use tt_application::services::native_regex_service::NativeRegexService;
use tt_application::services::preset_service::PresetService;
use tt_application::services::prompt_assembly_service::PromptAssemblyService;
use tt_application::services::provider_metadata_service::ProviderMetadataService;
use tt_application::services::quick_reply_service::QuickReplyService;
use tt_application::services::secret_service::SecretService;
use tt_application::services::settings_service::SettingsService;
use tt_application::services::skill_service::SkillService;
use tt_application::services::stable_diffusion_service::StableDiffusionService;
use tt_application::services::sync_automation_service::SyncAutomationService;
use tt_application::services::theme_service::ThemeService;
use tt_application::services::tokenization_service::TokenizationService;
use tt_application::services::translate_service::TranslateService;
use tt_application::services::tt_sync_service::TtSyncService;
use tt_application::services::tts_service::TtsService;
use tt_application::services::update_service::UpdateService;
use tt_application::services::user_directory_service::UserDirectoryService;
use tt_application::services::user_service::UserService;
use tt_application::services::world_info_service::WorldInfoService;
use tt_domain::errors::DomainError;
use tt_domain::ios_policy::IosPolicyActivationReport;

use super::{StartupProfile, composition};

pub struct AppState {
    pub(crate) services: AppServices,
    pub(crate) lifecycle: AppLifecycle,
    pub(crate) ios_policy: IosPolicyActivationReport,
}

pub(crate) struct AppLifecycle {
    pub(crate) sync_automation_cancel: CancellationToken,
    pub(crate) agent_run_retention_automation_cancel: CancellationToken,
    pub(crate) chat_history_cancel: CancellationToken,
}

impl AppLifecycle {
    fn new() -> Self {
        Self {
            sync_automation_cancel: CancellationToken::new(),
            agent_run_retention_automation_cancel: CancellationToken::new(),
            chat_history_cancel: CancellationToken::new(),
        }
    }
}

pub(crate) struct AppServices {
    pub(crate) character_service: Arc<CharacterService>,
    pub(crate) chat_service: Arc<ChatService>,
    pub(crate) group_chat_service: Arc<GroupChatService>,
    pub(crate) chat_history_coordinator: Arc<ChatHistoryCoordinator>,
    pub(crate) chat_payload_commit_service: Arc<ChatPayloadCommitService>,
    pub(crate) user_service: Arc<UserService>,
    pub(crate) settings_service: Arc<SettingsService>,
    pub(crate) user_directory_service: Arc<UserDirectoryService>,
    pub(crate) secret_service: Arc<SecretService>,
    pub(crate) skill_service: Arc<SkillService>,
    pub(crate) content_service: Arc<ContentService>,
    pub(crate) asset_service: Arc<AssetService>,
    pub(crate) extension_service: Arc<ExtensionService>,
    pub(crate) extension_store_service: Arc<ExtensionStoreService>,
    pub(crate) avatar_service: Arc<AvatarService>,
    pub(crate) group_service: Arc<GroupService>,
    pub(crate) background_service: Arc<BackgroundService>,
    pub(crate) image_metadata_service: Arc<ImageMetadataService>,
    pub(crate) theme_service: Arc<ThemeService>,
    pub(crate) preset_service: Arc<PresetService>,
    pub(crate) quick_reply_service: Arc<QuickReplyService>,
    pub(crate) agent_profile_service: Arc<AgentProfileService>,
    pub(crate) agent_profile_diagnostic_service: Arc<AgentProfileDiagnosticService>,
    pub(crate) prompt_assembly_service: Arc<PromptAssemblyService>,
    pub(crate) agent_run_history_service: Arc<AgentRunHistoryService>,
    pub(crate) agent_run_retention_automation_service: Arc<AgentRunRetentionAutomationService>,
    pub(crate) agent_runtime_service: Arc<AgentRuntimeService>,
    pub(crate) chat_completion_service: Arc<ChatCompletionService>,
    pub(crate) llm_connection_service: Arc<LlmConnectionService>,
    pub(crate) provider_metadata_service: Arc<ProviderMetadataService>,
    pub(crate) tokenization_service: Arc<TokenizationService>,
    pub(crate) stable_diffusion_service: Arc<StableDiffusionService>,
    pub(crate) translate_service: Arc<TranslateService>,
    pub(crate) tts_service: Arc<TtsService>,
    pub(crate) world_info_service: Arc<WorldInfoService>,
    pub(crate) lan_sync_service: Arc<LanSyncService>,
    pub(crate) tt_sync_service: Arc<TtSyncService>,
    pub(crate) sync_automation_service: Arc<SyncAutomationService>,
    pub(crate) data_archive_service: Arc<DataArchiveService>,
    pub(crate) update_service: Arc<UpdateService>,
    pub(crate) native_regex_service: Arc<NativeRegexService>,
}

impl AppState {
    pub(crate) async fn new(
        app_handle: AppHandle,
        runtime_paths: RuntimePaths,
        startup_profile: StartupProfile,
    ) -> Result<Self, DomainError> {
        tracing::info!(
            "Initializing application in {:?} mode with data root: {:?}",
            runtime_paths.mode,
            runtime_paths.data_root
        );

        let data_directory =
            composition::initialize_data_directory(&runtime_paths.data_root).await?;
        let services =
            composition::build_services(&app_handle, &data_directory, &startup_profile).await?;

        tracing::info!("Application initialized successfully");

        Ok(Self {
            services,
            lifecycle: AppLifecycle::new(),
            ios_policy: startup_profile.ios_policy,
        })
    }
}
