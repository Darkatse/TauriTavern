use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use ttsync_contract::peer::DeviceId;

use crate::domain::errors::DomainError;
use crate::domain::models::tt_sync::{
    TtSyncCompletedEvent, TtSyncErrorEvent, TtSyncPairedServer, TtSyncProgressEvent,
};
use crate::infrastructure::tt_sync::store::TtSyncStore;

pub struct TtSyncRuntime {
    app_handle: AppHandle,
    pub sync_root: PathBuf,
    pub store: TtSyncStore,
    sync_permit: Arc<Semaphore>,
    auto_event_depth: Arc<AtomicUsize>,
    paired_servers_cache: Mutex<Option<HashMap<String, TtSyncPairedServer>>>,
}

impl TtSyncRuntime {
    pub fn new(
        app_handle: AppHandle,
        sync_root: PathBuf,
        store_root: PathBuf,
        sync_permit: Arc<Semaphore>,
    ) -> Self {
        Self {
            app_handle,
            sync_root,
            store: TtSyncStore::new(store_root),
            sync_permit,
            auto_event_depth: Arc::new(AtomicUsize::new(0)),
            paired_servers_cache: Mutex::new(None),
        }
    }

    pub fn app_handle(&self) -> &AppHandle {
        &self.app_handle
    }

    pub fn try_acquire_sync_permit(&self) -> Result<OwnedSemaphorePermit, DomainError> {
        self.sync_permit
            .clone()
            .try_acquire_owned()
            .map_err(|_| DomainError::InvalidData("TT-Sync already running".to_string()))
    }

    #[must_use]
    pub fn auto_event_guard(&self) -> TtSyncAutoEventGuard {
        // TT-Sync jobs are serialized by the shared sync permit, so this depth
        // marker is scoped to the single active transfer.
        self.auto_event_depth.fetch_add(1, Ordering::AcqRel);
        TtSyncAutoEventGuard {
            auto_event_depth: self.auto_event_depth.clone(),
        }
    }

    pub async fn load_paired_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        let cached = {
            let cache = self.paired_servers_cache.lock().await;
            cache
                .as_ref()
                .map(|servers| servers.values().cloned().collect::<Vec<_>>())
        };
        if let Some(servers) = cached {
            return Ok(servers);
        }

        let servers = self.store.load_paired_servers().await?;
        let map = servers
            .iter()
            .cloned()
            .map(|server| (server.server_device_id.to_string(), server))
            .collect::<HashMap<_, _>>();
        {
            let mut cache = self.paired_servers_cache.lock().await;
            if cache.is_none() {
                *cache = Some(map);
            }
        }

        Ok(servers)
    }

    pub async fn get_paired_server(
        &self,
        server_device_id: &DeviceId,
    ) -> Result<TtSyncPairedServer, DomainError> {
        let cached = {
            let cache = self.paired_servers_cache.lock().await;
            cache
                .as_ref()
                .and_then(|map| map.get(server_device_id.as_str()).cloned())
        };
        if let Some(server) = cached {
            return Ok(server);
        }

        let servers = self.store.load_paired_servers().await?;
        let map = servers
            .into_iter()
            .map(|server| (server.server_device_id.to_string(), server))
            .collect::<HashMap<_, _>>();

        let result = map.get(server_device_id.as_str()).cloned().ok_or_else(|| {
            DomainError::NotFound(format!(
                "Paired TT-Sync server not found: {}",
                server_device_id
            ))
        })?;

        {
            let mut cache = self.paired_servers_cache.lock().await;
            if cache.is_none() {
                *cache = Some(map);
            }
        }

        Ok(result)
    }

    pub async fn upsert_paired_server(
        &self,
        server: TtSyncPairedServer,
    ) -> Result<(), DomainError> {
        self.store.upsert_paired_server(server.clone()).await?;

        let mut cache = self.paired_servers_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.insert(server.server_device_id.to_string(), server);
        }

        Ok(())
    }

    pub async fn remove_paired_server(
        &self,
        server_device_id: &DeviceId,
    ) -> Result<(), DomainError> {
        self.store.remove_paired_server(server_device_id).await?;

        let mut cache = self.paired_servers_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.remove(server_device_id.as_str());
        }

        Ok(())
    }

    pub fn emit_progress(&self, payload: TtSyncProgressEvent) -> Result<(), DomainError> {
        self.emit_with_origin("tt_sync:progress", payload)
    }

    pub fn emit_completed(&self, payload: TtSyncCompletedEvent) -> Result<(), DomainError> {
        self.emit_with_origin("tt_sync:completed", payload)
    }

    pub fn emit_error(&self, payload: TtSyncErrorEvent) -> Result<(), DomainError> {
        self.emit_with_origin("tt_sync:error", payload)
    }

    fn emit_with_origin<T: Serialize>(&self, event: &str, payload: T) -> Result<(), DomainError> {
        let mut payload = serde_json::to_value(payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        if let serde_json::Value::Object(map) = &mut payload {
            map.insert(
                "origin".to_string(),
                serde_json::Value::String(self.current_event_origin().to_string()),
            );
        }

        self.app_handle
            .emit(event, payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    fn current_event_origin(&self) -> &'static str {
        if self.auto_event_depth.load(Ordering::Acquire) > 0 {
            "auto"
        } else {
            "manual"
        }
    }
}

pub struct TtSyncAutoEventGuard {
    auto_event_depth: Arc<AtomicUsize>,
}

impl Drop for TtSyncAutoEventGuard {
    fn drop(&mut self) {
        self.auto_event_depth.fetch_sub(1, Ordering::AcqRel);
    }
}
