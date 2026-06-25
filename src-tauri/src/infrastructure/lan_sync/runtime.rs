use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, oneshot};
use ttsync_contract::sync::SyncMode;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncPairRequestEvent, LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent,
    LanSyncSyncProgressEvent,
};
use crate::infrastructure::lan_sync::store::LanSyncStore;

#[derive(Debug, Clone)]
pub struct LanSyncPairingSession {
    pub token: String,
    pub expires_at_ms: u64,
}

pub struct LanSyncRuntime {
    app_handle: AppHandle,
    pub sync_root: PathBuf,
    pub store: LanSyncStore,
    pairing_session: Mutex<Option<LanSyncPairingSession>>,
    sync_mode_override: Mutex<Option<SyncMode>>,
    pending_pairings: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl LanSyncRuntime {
    pub fn new(app_handle: AppHandle, sync_root: PathBuf, store_root: PathBuf) -> Self {
        Self {
            app_handle,
            sync_root,
            store: LanSyncStore::new(store_root),
            pairing_session: Mutex::new(None),
            sync_mode_override: Mutex::new(None),
            pending_pairings: Mutex::new(HashMap::new()),
        }
    }

    pub fn app_handle(&self) -> &AppHandle {
        &self.app_handle
    }

    pub async fn set_pairing_session(&self, session: LanSyncPairingSession) {
        let mut pairing_session = self.pairing_session.lock().await;
        *pairing_session = Some(session);
    }

    pub async fn get_pairing_session(&self) -> Option<LanSyncPairingSession> {
        self.pairing_session.lock().await.clone()
    }

    pub async fn clear_pairing_session(&self) {
        let mut pairing_session = self.pairing_session.lock().await;
        *pairing_session = None;
    }

    pub async fn get_sync_mode_override(&self) -> Option<SyncMode> {
        self.sync_mode_override.lock().await.clone()
    }

    pub async fn set_sync_mode_override(&self, mode: Option<SyncMode>) {
        let mut sync_mode_override = self.sync_mode_override.lock().await;
        *sync_mode_override = mode;
    }

    pub async fn effective_sync_mode(&self) -> Result<SyncMode, DomainError> {
        let preferences = self.store.load_or_create_sync_preferences().await?;
        Ok(self
            .get_sync_mode_override()
            .await
            .unwrap_or(preferences.manual_default_mode))
    }

    pub async fn request_pairing_decision(
        &self,
        peer_device_id: String,
        peer_device_name: String,
        peer_ip: String,
    ) -> Result<bool, DomainError> {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_pairings.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        self.app_handle
            .emit(
                "lan_sync:pair_request",
                LanSyncPairRequestEvent {
                    request_id: request_id.clone(),
                    peer_device_id,
                    peer_device_name,
                    peer_ip,
                },
            )
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        rx.await
            .map_err(|_| DomainError::InternalError("Pairing decision dropped".to_string()))
    }

    pub async fn confirm_pairing(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        let tx = {
            let mut pending = self.pending_pairings.lock().await;
            pending.remove(request_id).ok_or_else(|| {
                DomainError::NotFound(format!("Pair request not found: {}", request_id))
            })?
        };

        tx.send(accept).map_err(|_| {
            DomainError::InternalError("Pairing decision receiver dropped".to_string())
        })
    }

    pub fn emit_sync_progress(&self, payload: LanSyncSyncProgressEvent) -> Result<(), DomainError> {
        self.app_handle
            .emit("lan_sync:progress", payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub fn emit_sync_completed(
        &self,
        payload: LanSyncSyncCompletedEvent,
    ) -> Result<(), DomainError> {
        self.app_handle
            .emit("lan_sync:completed", payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub fn emit_sync_error(&self, payload: LanSyncSyncErrorEvent) -> Result<(), DomainError> {
        self.app_handle
            .emit("lan_sync:error", payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }
}
