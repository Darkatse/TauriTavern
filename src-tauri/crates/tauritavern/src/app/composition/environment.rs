use std::sync::Arc;

use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::infrastructure::assets::BundledResources;
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use tt_adapter_http::HttpClientPool;
use tt_adapter_storage_core::commit_stage::PageGeneration;
use tt_ports::data_archive::DataArchiveFileGateway;
use tt_ports::generation_background::GenerationBackgroundRuntime;
use tt_ports::lan_discovery::LanDiscoveryHost;
use tt_ports::lan_sync::{LanServerEvents, PairingApproval};
use tt_ports::sync::SyncJobEventPublisher;
use tt_ports::sync_automation::SyncAutomationEventPublisher;

/// Host values and adapters that composition wires into services. Managed
/// values are the same instances the host serves to commands.
pub(crate) struct HostEnvironment {
    pub(crate) http_client_pool: Arc<HttpClientPool>,
    pub(crate) page_generation: Arc<PageGeneration>,
    pub(crate) llm_api_log_store: Arc<LlmApiLogStore>,
    pub(crate) agent_extension_tools: Arc<AgentExtensionTools>,
    pub(crate) bundled_resources: Arc<dyn BundledResources>,
    pub(crate) generation_background_runtime: Option<Arc<dyn GenerationBackgroundRuntime>>,
    pub(crate) data_archive_file_gateway: Arc<dyn DataArchiveFileGateway>,
    pub(crate) sync_job_events: Arc<dyn SyncJobEventPublisher>,
    pub(crate) pairing_approval: Arc<dyn PairingApproval>,
    pub(crate) lan_discovery_host: Arc<dyn LanDiscoveryHost>,
    pub(crate) lan_server_events: Arc<dyn LanServerEvents>,
    pub(crate) sync_automation_events: Arc<dyn SyncAutomationEventPublisher>,
}
