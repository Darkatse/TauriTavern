use std::sync::Arc;

use async_trait::async_trait;
use tauri::Manager;
use ttsync_contract::peer::DeviceId;

use crate::app::AppState;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::LanSyncSyncErrorEvent;
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::lan_sync::v2::client::LanSyncV2Api;
use crate::infrastructure::lan_sync::v2::pull::pull_from_device;
use crate::infrastructure::lan_sync::v2::server::{
    LAN_PULL_REQUEST_SELECTION_FEATURE_V1, LanSyncV2PullRequestHandler,
};
use crate::infrastructure::lan_sync::v2::store::LanSyncV2Store;
use crate::infrastructure::sync_v2::SyncV2OperationOptions;
use crate::infrastructure::tt_sync::v2_api::ensure_dataset_scope_v1;

pub struct LanSyncV2NotifyPullHandler {
    runtime: Arc<LanSyncRuntime>,
    store: LanSyncV2Store,
}

impl LanSyncV2NotifyPullHandler {
    pub fn new(runtime: Arc<LanSyncRuntime>, store: LanSyncV2Store) -> Self {
        Self { runtime, store }
    }
}

#[async_trait]
impl LanSyncV2PullRequestHandler for LanSyncV2NotifyPullHandler {
    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncV2OperationOptions,
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
                                tracing::error!("Failed to emit LAN Sync v2 completion: {}", error);
                            }
                        }
                        Err(error) => emit_error(
                            &runtime,
                            format!(
                                "LAN Sync v2 completed but failed to refresh runtime caches: {}",
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
    store: LanSyncV2Store,
    device_id: &DeviceId,
    options: SyncV2OperationOptions,
) -> Result<(), DomainError> {
    let mut peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;

    let api = LanSyncV2Api::new(peer.base_url.clone(), peer.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "LAN Sync v2 peer")?;
    if options.require_bundle_zstd
        && !status
            .features
            .iter()
            .any(|feature| feature == LAN_PULL_REQUEST_SELECTION_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(
            "LAN Sync v2 peer does not support scoped pull requests".to_string(),
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
        tracing::error!("Failed to emit LAN Sync v2 error: {}", error);
    }
}
