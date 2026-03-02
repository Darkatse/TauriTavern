use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, oneshot};
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncPairedDevice, LanSyncPairRequestEvent, LanSyncSyncCompletedEvent,
    LanSyncSyncErrorEvent, LanSyncSyncProgressEvent,
};
use crate::infrastructure::lan_sync::store::LanSyncStore;

#[derive(Debug, Clone)]
pub struct LanSyncPairingSession {
    pub pair_code: String,
    pub expires_at_ms: u64,
}

pub struct LanSyncRuntime {
    app_handle: AppHandle,
    pub data_root: PathBuf,
    pub store: LanSyncStore,
    sync_permit: Arc<Semaphore>,
    pairing_session: Mutex<Option<LanSyncPairingSession>>,
    pending_pairings: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    paired_devices_cache: Mutex<Option<HashMap<String, LanSyncPairedDevice>>>,
}

impl LanSyncRuntime {
    pub fn new(app_handle: AppHandle, default_user_dir: PathBuf) -> Self {
        Self {
            app_handle,
            data_root: default_user_dir.clone(),
            store: LanSyncStore::new(default_user_dir),
            sync_permit: Arc::new(Semaphore::new(1)),
            pairing_session: Mutex::new(None),
            pending_pairings: Mutex::new(HashMap::new()),
            paired_devices_cache: Mutex::new(None),
        }
    }

    pub fn try_acquire_sync_permit(&self) -> Result<OwnedSemaphorePermit, DomainError> {
        self.sync_permit
            .clone()
            .try_acquire_owned()
            .map_err(|_| DomainError::InvalidData("LAN sync already running".to_string()))
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

    pub async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
        let mut cache = self.paired_devices_cache.lock().await;

        if let Some(devices) = cache.as_ref() {
            return Ok(devices.values().cloned().collect());
        }

        let devices = self.store.load_paired_devices().await?;
        let map = devices
            .iter()
            .cloned()
            .map(|device| (device.device_id.clone(), device))
            .collect::<HashMap<_, _>>();
        *cache = Some(map);

        Ok(devices)
    }

    pub async fn get_paired_device(
        &self,
        device_id: &str,
    ) -> Result<LanSyncPairedDevice, DomainError> {
        let mut cache = self.paired_devices_cache.lock().await;

        if let Some(map) = cache.as_ref() {
            return map
                .get(device_id)
                .cloned()
                .ok_or_else(|| DomainError::NotFound(format!("Paired device not found: {}", device_id)));
        }

        let devices = self.store.load_paired_devices().await?;
        let map = devices
            .into_iter()
            .map(|device| (device.device_id.clone(), device))
            .collect::<HashMap<_, _>>();

        let result = map
            .get(device_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("Paired device not found: {}", device_id)))?;

        *cache = Some(map);
        Ok(result)
    }

    pub async fn upsert_paired_device(&self, device: LanSyncPairedDevice) -> Result<(), DomainError> {
        self.store.upsert_paired_device(device.clone()).await?;

        let mut cache = self.paired_devices_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.insert(device.device_id.clone(), device);
        }

        Ok(())
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        self.store.remove_paired_device(device_id).await?;

        let mut cache = self.paired_devices_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.remove(device_id);
        }

        Ok(())
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
            pending
                .remove(request_id)
                .ok_or_else(|| DomainError::NotFound(format!("Pair request not found: {}", request_id)))?
        };

        tx.send(accept)
            .map_err(|_| DomainError::InternalError("Pairing decision receiver dropped".to_string()))
    }

    pub fn emit_sync_progress(&self, payload: LanSyncSyncProgressEvent) -> Result<(), DomainError> {
        self.app_handle
            .emit("lan_sync:progress", payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub fn emit_sync_completed(&self, payload: LanSyncSyncCompletedEvent) -> Result<(), DomainError> {
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
