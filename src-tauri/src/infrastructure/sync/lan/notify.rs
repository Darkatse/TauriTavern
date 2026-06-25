use std::sync::Arc;

use async_trait::async_trait;
use ttsync_contract::peer::DeviceId;

use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent};
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::sync::http_client::ensure_dataset_scope_v1;
use crate::infrastructure::sync::lan::client::LanSyncClient;
use crate::infrastructure::sync::lan::server::{
    LAN_PULL_REQUEST_SELECTION_FEATURE_V1, LanPullRequestHandler,
};
use crate::infrastructure::sync::lan::store::LanPeerStore;

pub struct LanSyncNotifyPullHandler {
    runtime: Arc<LanSyncRuntime>,
    coordinator: Arc<SyncJobCoordinator>,
}

impl LanSyncNotifyPullHandler {
    pub fn new(runtime: Arc<LanSyncRuntime>, coordinator: Arc<SyncJobCoordinator>) -> Self {
        Self {
            runtime,
            coordinator,
        }
    }
}

#[async_trait]
impl LanPullRequestHandler for LanSyncNotifyPullHandler {
    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError> {
        let mode = self.runtime.effective_sync_mode().await?;
        let request = SyncJobRequest {
            endpoint: SyncEndpointRef::LanPeer {
                device_id: peer_device_id.clone(),
            },
            intent: SyncIntent::PullToLocal,
            origin: SyncOrigin::RemoteRequest {
                peer_id: peer_device_id,
            },
            policy: ResolvedSyncPolicy::Transfer { mode, options },
        };
        let started = match self.coordinator.try_start(request) {
            Ok(started) => started,
            Err(report) => {
                return Err(DomainError::InvalidData(report_failure_message(&report)));
            }
        };

        let runtime = self.runtime.clone();

        tokio::spawn(async move {
            let report = started.execute().await.finish();

            if let Some(summary) = report.completed_summary() {
                runtime.emit_sync_completed(LanSyncSyncCompletedEvent {
                    files_total: summary.files_total,
                    bytes_total: summary.bytes_total,
                    files_deleted: summary.files_deleted,
                });
            } else if let Some(message) = report.failure_message() {
                emit_error(&runtime, message.to_string());
            }
        });

        Ok(())
    }
}

pub async fn request_peer_pull(
    store: LanPeerStore,
    device_id: &DeviceId,
    options: SyncOperationOptions,
) -> Result<(), DomainError> {
    let mut peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;

    let api = LanSyncClient::new(peer.base_url.clone(), peer.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "LAN Sync peer")?;
    if options.require_bundle_zstd
        && !status
            .features
            .iter()
            .any(|feature| feature == LAN_PULL_REQUEST_SELECTION_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(
            "LAN Sync peer does not support scoped pull requests".to_string(),
        ));
    }
    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;
    peer.grant.permissions = session.granted_permissions;
    store.upsert_paired_device(peer).await?;

    api.notify_pull_request(&session.session_token, &options)
        .await
}

fn emit_error(runtime: &LanSyncRuntime, message: String) {
    runtime.emit_sync_error(LanSyncSyncErrorEvent { message });
}

fn report_failure_message(report: &SyncJobReport) -> String {
    report
        .failure_message()
        .unwrap_or("LAN Sync job did not start")
        .to_string()
}
