use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

use ttsync_contract::pair::{PairCompleteRequest, PairUri};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;

use crate::app::AppState;
use crate::domain::errors::DomainError;
use crate::domain::models::tt_sync::{TtSyncDirection, TtSyncErrorEvent, TtSyncPairedServer};
use crate::infrastructure::tt_sync::identity::device_pubkey_b64url;
use crate::infrastructure::tt_sync::push::push_to_server;
use crate::infrastructure::tt_sync::pull::pull_from_server;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use crate::infrastructure::tt_sync::v2_api::TtSyncV2Api;

pub struct TtSyncService {
    runtime: Arc<TtSyncRuntime>,
}

impl TtSyncService {
    pub fn new(
        app_handle: AppHandle,
        sync_root: PathBuf,
        store_root: PathBuf,
        sync_permit: Arc<Semaphore>,
    ) -> Self {
        Self {
            runtime: Arc::new(TtSyncRuntime::new(app_handle, sync_root, store_root, sync_permit)),
        }
    }

    pub async fn pair(&self, pair_uri: &str) -> Result<TtSyncPairedServer, DomainError> {
        let pair = PairUri::parse_uri_string(pair_uri)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        let now_ms = now_ms();
        if now_ms > pair.expires_at_ms {
            return Err(DomainError::InvalidData(format!(
                "Pair URI expired at {} (now {})",
                pair.expires_at_ms, now_ms
            )));
        }

        let identity = self.runtime.store.load_or_create_identity().await?;
        let device_pubkey = device_pubkey_b64url(&identity.ed25519_seed)?;

        let request = PairCompleteRequest {
            device_id: identity.device_id,
            device_name: identity.device_name,
            device_pubkey,
        };

        let api = TtSyncV2Api::new(pair.url.clone(), pair.spki_sha256.clone())?;
        let response = api.pair_complete(&pair.token, &request).await?;

        let server = TtSyncPairedServer {
            server_device_id: response.server_device_id,
            server_device_name: response.server_device_name,
            base_url: pair.url,
            spki_sha256: pair.spki_sha256,
            permissions: response.granted_permissions,
            paired_at_ms: now_ms,
            last_sync_ms: None,
        };

        self.runtime.upsert_paired_server(server.clone()).await?;
        Ok(server)
    }

    pub async fn list_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        self.runtime.load_paired_servers().await
    }

    pub async fn remove_server(&self, server_device_id: &str) -> Result<(), DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        self.runtime.remove_paired_server(&server_device_id).await
    }

    pub async fn pull(&self, server_device_id: &str, mode: SyncMode) -> Result<(), DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        let permit = match self.runtime.try_acquire_sync_permit() {
            Ok(permit) => permit,
            Err(error) => {
                self.runtime.emit_error(TtSyncErrorEvent {
                    direction: TtSyncDirection::Pull,
                    message: error.to_string(),
                })?;
                return Ok(());
            }
        };

        let result = pull_from_server(self.runtime.clone(), &server_device_id, mode).await;

        match result {
            Ok(completed) => {
                let refresh_result = self
                    .runtime
                    .app_handle()
                    .state::<Arc<AppState>>()
                    .refresh_after_external_data_change("tt_sync_pull")
                    .await;

                match refresh_result {
                    Ok(()) => {
                        drop(permit);
                        self.runtime.emit_completed(completed)?;
                    }
                    Err(error) => {
                        drop(permit);
                        let message = format!(
                            "TT-Sync pull completed but failed to refresh runtime caches: {}",
                            error
                        );
                        self.runtime.emit_error(TtSyncErrorEvent {
                            direction: TtSyncDirection::Pull,
                            message: message.clone(),
                        })?;
                    }
                }
            }
            Err(error) => {
                drop(permit);
                self.runtime.emit_error(TtSyncErrorEvent {
                    direction: TtSyncDirection::Pull,
                    message: error.to_string(),
                })?;
            }
        }

        Ok(())
    }

    pub async fn push(&self, server_device_id: &str, mode: SyncMode) -> Result<(), DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        let permit = match self.runtime.try_acquire_sync_permit() {
            Ok(permit) => permit,
            Err(error) => {
                self.runtime.emit_error(TtSyncErrorEvent {
                    direction: TtSyncDirection::Push,
                    message: error.to_string(),
                })?;
                return Ok(());
            }
        };

        let result = push_to_server(self.runtime.clone(), &server_device_id, mode).await;

        match result {
            Ok(completed) => {
                drop(permit);
                self.runtime.emit_completed(completed)?;
            }
            Err(error) => {
                drop(permit);
                self.runtime.emit_error(TtSyncErrorEvent {
                    direction: TtSyncDirection::Push,
                    message: error.to_string(),
                })?;
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
