use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::app::HostEnvironment;
use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::infrastructure::bundled_resources::BundledResourceStore;
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use crate::infrastructure::persistence::data_archive_adapters::TauriDataArchiveFileGateway;
use crate::infrastructure::sync_host;
use tt_adapter_http::HttpClientPool;
use tt_adapter_storage_core::commit_stage::PageGeneration;

pub(crate) fn host_environment(app_handle: &AppHandle) -> HostEnvironment {
    HostEnvironment {
        http_client_pool: app_handle.state::<Arc<HttpClientPool>>().inner().clone(),
        page_generation: app_handle.state::<Arc<PageGeneration>>().inner().clone(),
        llm_api_log_store: app_handle.state::<Arc<LlmApiLogStore>>().inner().clone(),
        agent_extension_tools: app_handle
            .state::<Arc<AgentExtensionTools>>()
            .inner()
            .clone(),
        bundled_resources: Arc::new(BundledResourceStore::new(app_handle.clone())),
        generation_background_runtime: crate::platform::generation_background::runtime(app_handle),
        data_archive_file_gateway: Arc::new(TauriDataArchiveFileGateway::new(app_handle.clone())),
        sync_job_events: sync_host::sync_job_events(app_handle),
        pairing_approval: sync_host::pairing_approval(app_handle),
        lan_discovery_host: sync_host::lan_discovery_host(app_handle),
        lan_server_events: sync_host::lan_server_events(app_handle),
        sync_automation_events: sync_host::sync_automation_events(app_handle),
    }
}
