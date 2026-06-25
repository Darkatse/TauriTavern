use std::sync::Arc;

use async_trait::async_trait;
use tauri::Manager;
use ttsync_contract::peer::DeviceId;

use crate::app::AppState;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::LanSyncSyncErrorEvent;
use crate::domain::models::sync::SyncOperationOptions;
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::sync::http_client::ensure_dataset_scope_v1;
use crate::infrastructure::sync::lan::client::LanSyncClient;
use crate::infrastructure::sync::lan::pull::pull_from_device;
use crate::infrastructure::sync::lan::server::{
    LAN_PULL_REQUEST_SELECTION_FEATURE_V1, LanPullRequestHandler,
};
use crate::infrastructure::sync::lan::store::LanPeerStore;

pub struct LanSyncNotifyPullHandler {
    runtime: Arc<LanSyncRuntime>,
    store: LanPeerStore,
}

impl LanSyncNotifyPullHandler {
    pub fn new(runtime: Arc<LanSyncRuntime>, store: LanPeerStore) -> Self {
        Self { runtime, store }
    }
}

#[async_trait]
impl LanPullRequestHandler for LanSyncNotifyPullHandler {
    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError> {
        let permit = self.runtime.try_acquire_sync_permit()?;
        let runtime = self.runtime.clone();
        let store = self.store.clone();

        tokio::spawn(async move {
            let _permit = permit;
            match pull_from_device(runtime.clone(), store, &peer_device_id, options).await {
                Ok(completed) => {
                    let refresh_result = runtime
                        .app_handle()
                        .state::<Arc<AppState>>()
                        .refresh_after_external_data_change("lan_sync")
                        .await;
                    match refresh_result {
                        Ok(()) => {
                            if let Err(error) = runtime.emit_sync_completed(completed) {
                                tracing::error!("Failed to emit LAN Sync completion: {}", error);
                            }
                        }
                        Err(error) => emit_error(
                            &runtime,
                            format!(
                                "LAN Sync completed but failed to refresh runtime caches: {}",
                                error
                            ),
                        ),
                    }
                }
                Err(error) => emit_error(&runtime, error.to_string()),
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
    if let Err(error) = runtime.emit_sync_error(LanSyncSyncErrorEvent { message }) {
        tracing::error!("Failed to emit LAN Sync error: {}", error);
    }
}
