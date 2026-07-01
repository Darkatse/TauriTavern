use std::sync::Arc;

use tauri::AppHandle;

use crate::application::services::lan_sync_service::{
    LanInboundService, LanSyncRuntimeState, LanSyncService,
};
use crate::application::services::sync_automation_service::SyncAutomationService;
use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::application::services::tt_sync_service::TtSyncService;
use crate::domain::ios_policy::IosPolicyActivationReport;
use crate::infrastructure::lan_sync::store::LanSyncStore;
use crate::infrastructure::persistence::file_system::DataDirectory;
use crate::infrastructure::sync::http_client::HttpTtPairingClient;
use crate::infrastructure::sync::job_executor::InfrastructureSyncJobExecutor;
use crate::infrastructure::sync::lan::client::HttpLanPairingClient;
use crate::infrastructure::sync::lan::control::AxumLanServerControl;
use crate::infrastructure::sync::lan::discovery::LocalLanAddressDiscovery;
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::sync_automation_store::SyncAutomationStore;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use tt_ports::lan_sync::{LanPeerRepository, LanServerControl, LanSyncSettingsRepository};
use tt_ports::sync::DataChangeReconciler;

use super::super::adapters;

pub(super) struct SyncServices {
    pub(super) lan_sync_service: Arc<LanSyncService>,
    pub(super) tt_sync_service: Arc<TtSyncService>,
    pub(super) sync_automation_service: Arc<SyncAutomationService>,
}

pub(super) fn build(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
    data_change_reconciler: Arc<dyn DataChangeReconciler>,
    ios_policy: &IosPolicyActivationReport,
) -> SyncServices {
    let lan_runtime_state = Arc::new(LanSyncRuntimeState::new());
    let lan_settings_store = Arc::new(LanSyncStore::new(
        data_directory.default_user().to_path_buf(),
    ));
    let lan_peer_store = LanPeerStore::new(data_directory.default_user().to_path_buf());
    let lan_settings_repository: Arc<dyn LanSyncSettingsRepository> = lan_settings_store.clone();
    let lan_peer_repository: Arc<dyn LanPeerRepository> = Arc::new(lan_peer_store.clone());
    let sync_job_events = adapters::sync_job_events(app_handle);
    let pairing_approval = adapters::pairing_approval(app_handle);
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
        data_change_reconciler,
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
    let lan_sync_allowed = ios_policy.capabilities.sync.lan;
    let sync_automation_service = Arc::new(SyncAutomationService::new(
        adapters::sync_automation_events(app_handle),
        Arc::new(SyncAutomationStore::new(
            data_directory.default_user().to_path_buf(),
        )),
        lan_settings_store,
        adapters::sync_automation_endpoint_catalog(
            lan_sync_service.clone(),
            tt_sync_service.clone(),
            lan_sync_allowed,
        ),
        adapters::sync_automation_lan_server(lan_sync_service.clone(), lan_sync_allowed),
        sync_job_coordinator,
    ));

    SyncServices {
        lan_sync_service,
        tt_sync_service,
        sync_automation_service,
    }
}
