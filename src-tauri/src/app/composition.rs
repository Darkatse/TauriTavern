mod adapters;
mod repositories;
mod services;

use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;

use crate::application::services::agent_profile_diagnostic_service::AgentProfileDiagnosticService;
use crate::application::services::agent_profile_service::AgentProfileService;
use crate::application::services::agent_run_history_service::AgentRunHistoryService;
use crate::application::services::agent_run_retention_automation_service::AgentRunRetentionAutomationService;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::asset_service::AssetService;
use crate::application::services::avatar_service::AvatarService;
use crate::application::services::background_service::BackgroundService;
use crate::application::services::character_service::CharacterService;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_service::ChatService;
use crate::application::services::content_service::ContentService;
use crate::application::services::data_archive_service::DataArchiveService;
use crate::application::services::extension_service::ExtensionService;
use crate::application::services::extension_store_service::ExtensionStoreService;
use crate::application::services::group_chat_service::GroupChatService;
use crate::application::services::group_service::GroupService;
use crate::application::services::image_metadata_service::ImageMetadataService;
use crate::application::services::lan_sync_service::LanSyncService;
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::application::services::native_regex_service::NativeRegexService;
use crate::application::services::preset_service::PresetService;
use crate::application::services::prompt_assembly_service::PromptAssemblyService;
use crate::application::services::provider_metadata_service::ProviderMetadataService;
use crate::application::services::quick_reply_service::QuickReplyService;
use crate::application::services::secret_service::SecretService;
use crate::application::services::settings_service::SettingsService;
use crate::application::services::skill_service::SkillService;
use crate::application::services::stable_diffusion_service::StableDiffusionService;
use crate::application::services::sync_automation_service::SyncAutomationService;
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
use crate::infrastructure::persistence::file_system::DataDirectory;

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
    services::build(app_handle, data_directory, startup_profile).await
}
