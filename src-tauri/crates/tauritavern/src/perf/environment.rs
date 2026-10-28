//! The host the replay runs in: Tauri's mock runtime, the repository's own bundled resources, and no page, peer,
//! archive or background worker. The backend is ready as the app's startup leaves it: state built and the
//! default user's content in place.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, Manager};

use crate::app::{AppState, HostEnvironment, StartupProfile};
use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::infrastructure::assets::{
    BundledResources, map_resource_error, normalize_resource_relative_path, read_resource_text,
};
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use crate::infrastructure::paths::{RuntimeMode, RuntimePaths};
use crate::presentation::commands as c;
use tt_adapter_http::HttpClientPool;
use tt_adapter_storage_core::commit_stage::PageGeneration;
use tt_application::services::bundled_template_service::BundledTemplateService;
use tt_contracts::lan_discovery::LanDiscoveredDevice;
use tt_contracts::sync::SyncJobEvent;
use tt_contracts::sync_automation::{SyncAutomationStatus, SyncAutomationToastEvent};
use tt_domain::errors::DomainError;
use tt_ports::bundled_template::BundledTemplateStore;
use tt_ports::data_archive::{
    DataArchiveFileGateway, ExportArchiveExecutionRequest, ImportArchiveExecutionRequest,
    UserBackupArchiveTarget,
};
use tt_ports::lan_discovery::LanDiscoveryHost;
use tt_ports::lan_sync::{LanPairingApprovalRequest, LanServerEvents, PairingApproval};
use tt_ports::sync::SyncJobEventPublisher;
use tt_ports::sync_automation::SyncAutomationEventPublisher;

/// `root` holds the data root at `root/data`; `repo` is the checkout whose bundled resources the app ships.
pub(super) fn mock_app(root: &Path, repo: &Path) -> Result<App<MockRuntime>, String> {
    let resources = Arc::new(RepositoryResources::load(repo)?);
    let app = mock_builder()
        .manage(Arc::new(BundledTemplateService::new(resources.clone())))
        .invoke_handler(tauri::generate_handler![
            c::settings_commands::get_tauritavern_settings,
            c::bridge::get_client_version,
            c::bootstrap_commands::get_bootstrap_metadata,
            c::settings_commands::get_sillytavern_settings,
            c::extension_commands::get_extensions,
            c::resource_bridge_commands::read_frontend_template,
            c::background_commands::get_all_backgrounds,
            c::image_metadata_commands::get_background_folders,
            c::background_commands::get_all_background_metadata,
            c::extension_store_commands::open_extension_store_entry,
            c::agent_commands::list_agent_profiles,
            c::skill_commands::list_skills,
            c::mcp_commands::list_mcp_servers,
            c::mcp_commands::create_mcp_server,
            c::character_commands::get_character_chats_by_id,
            c::character_commands::get_character,
            c::chat_commands::get_chat_payload_path,
            c::agent_commands::list_active_agent_runs,
            c::world_info_commands::get_world_info,
            c::tokenizer_commands::count_openai_tokens_batch,
            c::avatar_commands::get_personas,
            c::quick_reply_commands::save_quick_reply_set,
            c::settings_commands::begin_settings_commit,
            c::settings_commands::append_settings_commit_chunk,
            c::settings_commands::finish_settings_commit,
            c::secret_commands::read_secret_state,
            c::chat_commands::chat_history_generation_started,
            c::chat_commit_commands::begin_chat_commit,
            c::chat_commit_commands::append_chat_commit_chunk,
            c::chat_commit_commands::finish_chat_commit,
            c::chat_commands::chat_history_generation_finished,
            c::world_info_commands::begin_world_info_commit,
            c::world_info_commands::append_world_info_commit_chunk,
            c::world_info_commands::finish_world_info_commit,
        ])
        .build(mock_context(noop_assets()))
        .map_err(|error| error.to_string())?;

    let runtime_paths = RuntimePaths::new(RuntimeMode::Standard, root.to_path_buf());
    let startup_profile =
        StartupProfile::load(&runtime_paths.data_root).map_err(|error| error.to_string())?;
    let environment = HostEnvironment {
        http_client_pool: Arc::new(HttpClientPool::new(crate::product::USER_AGENT)),
        page_generation: Arc::new(PageGeneration::default()),
        llm_api_log_store: Arc::new(LlmApiLogStore::new(
            app.handle().clone(),
            runtime_paths.log_root.clone(),
        )),
        agent_extension_tools: Arc::new(AgentExtensionTools::default()),
        bundled_resources: resources,
        generation_background_runtime: None,
        data_archive_file_gateway: Arc::new(Detached),
        sync_job_events: Arc::new(Detached),
        pairing_approval: Arc::new(Detached),
        lan_discovery_host: Arc::new(Detached),
        lan_server_events: Arc::new(Detached),
        sync_automation_events: Arc::new(Detached),
    };
    let state = tauri::async_runtime::block_on(async {
        let state = AppState::new(environment, runtime_paths, startup_profile).await?;
        state
            .services
            .content_service
            .initialize_default_content("default-user")
            .await?;
        Ok::<_, DomainError>(state)
    })
    .map_err(|error| error.to_string())?;
    app.manage(Arc::new(state));
    Ok(app)
}

/// Bundled resources read from the checkout through the bundle's own source-to-resource mapping.
struct RepositoryResources {
    mappings: Vec<(String, PathBuf)>,
}

impl RepositoryResources {
    fn load(repo: &Path) -> Result<Self, String> {
        let config_dir = repo.join("src-tauri/crates/tauritavern");
        let config_path = config_dir.join("tauri.conf.json");
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&config_path)
                .map_err(|error| format!("{}: {error}", config_path.display()))?,
        )
        .map_err(|error| format!("{}: {error}", config_path.display()))?;
        let mappings = config["bundle"]["resources"]
            .as_object()
            .ok_or_else(|| format!("{} maps no bundle resources", config_path.display()))?
            .iter()
            .map(|(source, target)| {
                let target = target
                    .as_str()
                    .ok_or("bundle resource target is not a string")?;
                Ok((target.to_string(), config_dir.join(source)))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { mappings })
    }
}

impl BundledResources for RepositoryResources {
    fn read_bytes(&self, relative_path: &str) -> Result<Vec<u8>, DomainError> {
        let normalized = normalize_resource_relative_path(relative_path)?;
        let path = self
            .mappings
            .iter()
            .find_map(|(target, source)| Some(source.join(normalized.strip_prefix(target)?)))
            .ok_or_else(|| DomainError::NotFound(format!("Resource not found: {normalized}")))?;
        std::fs::read(path).map_err(|error| map_resource_error(&normalized, error))
    }
}

impl BundledTemplateStore for RepositoryResources {
    fn read_text(&self, relative_path: &str) -> Result<String, DomainError> {
        read_resource_text(self, relative_path)
    }
}

/// No page hears events, and no archive or LAN peer can be reached.
struct Detached;

fn unreachable_in_replay(what: &str) -> DomainError {
    DomainError::InternalError(format!("{what} is not available in the perf replay"))
}

impl DataArchiveFileGateway for Detached {
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        Err(unreachable_in_replay("Data archive import"))
    }

    fn prepare_import_archive(
        &self,
        _archive_path: &Path,
        _archive_is_temporary: bool,
        _job_id: &str,
    ) -> Result<ImportArchiveExecutionRequest, DomainError> {
        Err(unreachable_in_replay("Data archive import"))
    }

    fn prepare_export_archive(
        &self,
        _job_id: &str,
        _protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError> {
        Err(unreachable_in_replay("Data archive export"))
    }

    fn prepare_user_backup_archive(
        &self,
        _handle: &str,
        _include_secrets: bool,
        _protected_paths: &[PathBuf],
    ) -> Result<UserBackupArchiveTarget, DomainError> {
        Err(unreachable_in_replay("User backup"))
    }

    fn cleanup_directory(&self, _path: &Path) {}

    fn cleanup_export(&self, _archive_path: &Path) -> Result<(), DomainError> {
        Ok(())
    }
}

impl SyncJobEventPublisher for Detached {
    fn publish_sync_job(&self, _event: SyncJobEvent) {}
}

impl SyncAutomationEventPublisher for Detached {
    fn publish_status(&self, _status: SyncAutomationStatus) {}

    fn publish_toast(&self, _event: SyncAutomationToastEvent) {}
}

impl LanServerEvents for Detached {
    fn report_lan_server_error(&self, _message: String) {}

    fn pairing_completed(&self) {}
}

#[async_trait]
impl PairingApproval for Detached {
    async fn request(&self, _request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
        Err(unreachable_in_replay("LAN pairing"))
    }

    async fn confirm(&self, _request_id: &str, _accept: bool) -> Result<(), DomainError> {
        Err(unreachable_in_replay("LAN pairing"))
    }

    async fn cancel_all(&self) {}
}

#[async_trait]
impl LanDiscoveryHost for Detached {
    async fn set_enabled(&self, _enabled: bool) -> Result<(), DomainError> {
        Err(unreachable_in_replay("LAN discovery"))
    }

    fn publish_changed(&self, _devices: Vec<LanDiscoveredDevice>) {}
}
